use anyhow::{anyhow, Context as _};
use clap::{Parser, ValueEnum};
use lazy_static::lazy_static;
use libpulse_binding::{
    callbacks::ListResult,
    context::{introspect::Introspector, Context, FlagSet as ContextFlagSet},
    def::{BufferAttr, Retval},
    mainloop::{standard::IterateResult, standard::Mainloop},
    sample::{Format, Spec},
    stream::Direction,
};
use libpulse_simple_binding::Simple;
use nix::libc::SIGINT;
use regex::{Captures, Regex};
use std::{
    borrow::Cow,
    cell::RefCell,
    fmt::Display,
    io::Read,
    panic::{catch_unwind, UnwindSafe},
    process::ExitCode,
    rc::Rc,
    time::Duration,
};

use crate::{
    ctrlc::{self, CtrlCIgnoredContext},
    io,
};

trait DecodeSampleUnsigned {
    fn decode_sample(input: u8) -> [u8; 8];
}

struct DecodeSampleUnsignedFullRange {}
impl DecodeSampleUnsigned for DecodeSampleUnsignedFullRange {
    #[inline(always)]
    fn decode_sample(input: u8) -> [u8; 8] {
        io::decode_esp32_sample_unsigned_full_range(input)
    }
}

struct DecodeSampleUnsignedHalfRange {}
impl DecodeSampleUnsigned for DecodeSampleUnsignedHalfRange {
    #[inline(always)]
    fn decode_sample(input: u8) -> [u8; 8] {
        io::decode_esp32_sample_unsigned_half_range(input)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum WaveAmplitude {
    Full,
    Half,
}

impl Display for WaveAmplitude {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_possible_value().unwrap().get_name())
    }
}

#[derive(Parser)]
pub struct PulseStreamArgs {
    #[arg(short, long)]
    pub port: String,
    #[arg(short, long)]
    pub sampling_rate: u32,

    #[arg(short, long)]
    pub baud_rate: u32,

    #[arg(short, long, default_value_t = WaveAmplitude::Full)]
    pub wave_amplitude: WaveAmplitude,
}

lazy_static! {
    static ref PA_ESCAPE_CHARS_REGEX: Regex = Regex::new(r#"('|"| |\\)"#).unwrap();
}
struct SinkSpec {
    sink_name: String,
    device_description: Option<String>,
    audio_format: Spec,
}

impl SinkSpec {
    fn pa_escape_string(input: &str) -> Cow<str> {
        PA_ESCAPE_CHARS_REGEX.replace_all(input, |capture: &Captures| {
            match capture.get(1).unwrap().as_str() {
                "'" => "\\'",
                "\"" => "\\\"",
                "\\" => "\\\\",
                " " => "\\ ",
                _ => panic!("Unexpected match content!"),
            }
        })
    }

    fn audio_format_to_string(format: &Format) -> &'static str {
        match format {
            Format::U8 => "u8",
            Format::S16le => "s16le",
            Format::S16be => "s16be",
            Format::F32le => "f32le",
            Format::F32be => "f32be",
            Format::S32le => "s32le",
            Format::S32be => "f32be",
            Format::S24le => "s24le",
            Format::S24be => "s24be",
            _ => panic!("Unsupported format mode: {:?}", format),
        }
    }

    fn build_sink_arguments(&self) -> String {
        let sink_properties = self
            .device_description
            .clone()
            .map(|description| {
                format!(
                    "sink_properties=device.description='{}'",
                    Self::pa_escape_string(&description).as_ref()
                )
            })
            .unwrap_or("".into());
        let sink_name = format!("sink_name={}", &self.sink_name);
        let sink_format = format!(
            "format={}",
            Self::audio_format_to_string(&self.audio_format.format)
        );
        let sink_rate = format!("rate={}", self.audio_format.rate);
        let sink_channels = format!("channels={}", self.audio_format.channels);

        [
            sink_name.as_str(),
            sink_properties.as_str(),
            sink_format.as_str(),
            sink_rate.as_str(),
            sink_channels.as_str(),
        ]
        .join(" ")
    }
}

struct PulseUtil {
    context: Context,
    mainloop: Mainloop,
}

impl PulseUtil {
    fn create(name: &str) -> anyhow::Result<PulseUtil> {
        let mut mainloop = Mainloop::new().context("Unable to create pulse main loop")?;
        let mut context =
            Context::new(&mainloop, name).context("Unable to create pulse context")?;
        context
            .connect(None, ContextFlagSet::NOFLAGS, None)
            .context("Unable to connect pulse context")?;

        loop {
            Self::iterate_mainloop(&mut mainloop, false)?;
            match context.get_state() {
                libpulse_binding::context::State::Ready => {
                    break;
                }
                libpulse_binding::context::State::Failed
                | libpulse_binding::context::State::Terminated => {
                    return Err(anyhow!("Pulse context failed terminated!"))
                }
                _ => {}
            }
        }

        Ok(PulseUtil { context, mainloop })
    }

    fn iterate_mainloop(mainloop: &mut Mainloop, block: bool) -> anyhow::Result<()> {
        match mainloop.iterate(block) {
            IterateResult::Quit(_) => Err(anyhow!("Pulse Mainloop exited unexpectedly!")),
            IterateResult::Err(code) => Err(anyhow!(
                "Pulse Mainloop iteration failed with code {}",
                code
            )),
            IterateResult::Success(_) => Ok(()),
        }
    }

    fn wait_next_event(&mut self) -> anyhow::Result<()> {
        Self::iterate_mainloop(&mut self.mainloop, true)
    }

    fn call_introspect_function<A: Eq + 'static, F: FnOnce(Introspector, Box<dyn FnMut(A)>)>(
        &mut self,
        caller: F,
    ) -> anyhow::Result<A> {
        let cell: Rc<RefCell<Option<A>>> = Rc::new(RefCell::new(None));

        let setter = cell.clone();
        let callback_fun = move |result: A| {
            if setter.borrow().is_none() {
                setter.replace(Some(result));
            }
        };

        caller(self.context.introspect(), Box::new(callback_fun));

        while &*cell.borrow() == &None {
            self.wait_next_event()?;
        }

        match Rc::try_unwrap(cell) {
            Ok(value) => Ok(value.into_inner().unwrap()),
            Err(_) => panic!("Rc::try_unwrap failed. This shouldn't happen at this point!"),
        }
    }

    fn get_sink_owner_module_by_name(&mut self, name: &str) -> anyhow::Result<Option<Option<u32>>> {
        // FIXME This is likely not the good way of implementing this,
        // as the callback is supposed to be called multiple times for
        // returning ListResult::Item, ListResult::End, etc. Hopefully
        // this simplification works anyway.
        self.call_introspect_function(|introspector, mut callback| {
            introspector.get_sink_info_by_name(name, move |result| match result {
                ListResult::Item(item) => callback(Some(item.owner_module)),
                ListResult::End | ListResult::Error => callback(None),
            });
        })
    }

    fn load_module(&mut self, name: &str, arg: &str) -> anyhow::Result<u32> {
        let result = self.call_introspect_function(|mut introspector, callback| {
            introspector.load_module(name, arg, callback);
        })?;

        if result == u32::MAX {
            // Error
            Err(anyhow!("Module initialization failed"))
        } else {
            Ok(result)
        }
    }

    fn unload_module(&mut self, index: u32) -> anyhow::Result<bool> {
        self.call_introspect_function(|mut introspector, callback| {
            introspector.unload_module(index, callback);
        })
    }

    fn using_null_sink<T, E, F: FnOnce() -> std::result::Result<T, E> + UnwindSafe>(
        &mut self,
        sink_spec: SinkSpec,
        f: F,
    ) -> anyhow::Result<std::result::Result<T, E>> {
        let module_index =
            self.load_module("module-null-sink", &sink_spec.build_sink_arguments())?;
        let result = catch_unwind(|| f());
        self.unload_module(module_index)?;
        result.map_err(|error| panic!("Program panick'ed while using Pulse module: {:?}", error))
    }
}

const PULSE_SINK_NAME: &'static str = "esp32-signal-device";
fn stream_samples_to_pulse<R: Read, S: DecodeSampleUnsigned>(
    input: &mut R,
    sampling_rate: u32,
    ctrlc_context: &CtrlCIgnoredContext,
    simple: &mut Simple,
) -> anyhow::Result<()> {
    // Adjust buffer size to hold approx 50 msecs of data, with a
    // minimum of 32 bytes.
    let buf_size = usize::max((sampling_rate / (8 * 20)) as usize, 32);

    let mut buf = vec![0; buf_size];
    let mut out_buf = vec![0; buf_size * 8];

    let mut total_written_samples: usize = 0;
    while !ctrlc_context.has_received_ctrlc() {
        io::recover_if_interrupted(|| input.read_exact(&mut buf), || ())?;

        for i in 0..buf.len() {
            (&mut out_buf[i * 8..(i + 1) * 8]).copy_from_slice(&S::decode_sample(buf[i])[..])
        }

        simple.write(&out_buf[..])?;
        total_written_samples += out_buf.len();
        let written_duration = total_written_samples as f32 / sampling_rate as f32;

        eprint!(
            "Total {} samples read; {:.2} seconds of recording...\r",
            total_written_samples, written_duration
        );
    }
    simple.drain()?;
    Ok(())
}

pub fn run_pulse_stream_command(args: &PulseStreamArgs) -> anyhow::Result<ExitCode> {
    let mut pulse_util = PulseUtil::create("esp32-pulse")?;
    if let Some(existing_dev_module) = pulse_util.get_sink_owner_module_by_name(PULSE_SINK_NAME)? {
        eprintln!("Sink '{}' already exists, probably because the program did not exit cleanly the last time.", PULSE_SINK_NAME);
        match existing_dev_module {
            Some(mod_number) => {
                eprintln!(
                    "Please remove it manually before proceeding with the following command:"
                );
                eprintln!();
                eprintln!("pactl unload-module {}", mod_number);
            }
            None => {
                eprintln!("Please remove it manually before proceeding.");
            }
        }

        return Ok(ExitCode::FAILURE);
    }

    let audio_spec = Spec {
        format: Format::U8,
        channels: 1,
        rate: args.sampling_rate,
    };

    let result = ctrlc::ignoring_ctrlc(|ctrlc_context| {
        let sink_spec = SinkSpec {
            sink_name: PULSE_SINK_NAME.into(),
            device_description: Some("ESP32 Signal Reader".into()),
            audio_format: audio_spec.clone(),
        };

        pulse_util.using_null_sink(sink_spec, || -> anyhow::Result<()> {
            let mut simple = Simple::new(
                None,
                "esp32-samples-reader",
                Direction::Playback,
                Some(PULSE_SINK_NAME),
                "ESP32 Reader Stream",
                &audio_spec,
                None,
                Some(&BufferAttr {
                    maxlength: u32::MAX,
                    tlength: u32::MAX,
                    prebuf: args.sampling_rate / 8, // A second of prebuf.
                    minreq: u32::MAX,
                    fragsize: 0,
                }),
            )?;

            // Make sure to open the serial after establishing
            // connection to pulse, for preventing delays while
            // reading data from the port.
            let mut serial =
                io::open_serial_port(&args.port, args.baud_rate, Duration::from_secs(1))?;

            (match args.wave_amplitude {
                WaveAmplitude::Full => stream_samples_to_pulse::<_, DecodeSampleUnsignedFullRange>(
                    &mut serial,
                    args.sampling_rate,
                    ctrlc_context,
                    &mut simple,
                ),
                WaveAmplitude::Half => stream_samples_to_pulse::<_, DecodeSampleUnsignedHalfRange>(
                    &mut serial,
                    args.sampling_rate,
                    ctrlc_context,
                    &mut simple,
                ),
            })?;
            Ok(())
        })
    })?;

    pulse_util.mainloop.quit(Retval(0));
    Ok(if result.has_received_ctrlc {
        ExitCode::from(128 + SIGINT as u8)
    } else {
        ExitCode::SUCCESS
    })
}
