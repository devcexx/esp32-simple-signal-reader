use std::{io::Read, process::ExitCode, time::Duration};

use crate::{
    ctrlc::{self, CtrlCIgnoredOutput},
    io,
};
use clap::Parser;
use hound::{WavSpec, WavWriter};
use nix::libc::SIGINT;

#[derive(Parser)]
pub struct ReadWavArgs {
    #[arg(short, long)]
    pub port: String,
    #[arg(short, long)]
    pub sampling_rate: u32,

    #[arg(short, long)]
    pub baud_rate: u32,

    #[arg(short, long)]
    pub output: String,
}

pub fn run_write_wav_command(args: &ReadWavArgs) -> anyhow::Result<ExitCode> {
    // Adjust the buffer size to the expected data flow, between a set
    // of limits. Default set to a quarter of the expected data to be
    // received in a second (Arbitrarily chosen number).
    let buf_size = usize::max(1024, args.sampling_rate as usize / (8 * 4));

    // buf_size will be set to half of the bytes required to read 1
    // second of recording, So a timeout of 1 second is enough.
    let mut serial = io::open_serial_port(&args.port, args.baud_rate, Duration::from_secs(1))?;
    let spec = WavSpec {
        channels: 1,
        sample_rate: args.sampling_rate,
        bits_per_sample: 8,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = WavWriter::create(&args.output, spec)?;

    let mut input_buf = vec![0; buf_size];
    let mut total_written_samples = 0;

    let result: CtrlCIgnoredOutput<anyhow::Result<()>> = ctrlc::ignoring_ctrlc(|context| {
        while !context.has_received_ctrlc() {
            io::recover_if_interrupted(|| serial.read_exact(&mut input_buf), || ())?;

            for i in 0..buf_size {
                for sample in io::decode_esp32_sample(input_buf[i]) {
                    io::retry_if_interrupted(
                        || writer.write_sample(sample),
                        |e| match e {
                            hound::Error::IoError(e) => Some(e),
                            _ => None,
                        },
                    )?;
                }
            }

            total_written_samples += buf_size * 8;
            let written_duration = total_written_samples as f32 / args.sampling_rate as f32;

            eprint!(
                "Total {} samples read; {:.2} seconds of recording...\r",
                total_written_samples, written_duration
            );
        }

        Ok(())
    })?;

    let exit_code = if result.has_received_ctrlc {
        eprintln!();
        eprintln!("Ctrl+C handled. Stopping...");
        writer.finalize()?;

        ExitCode::from((128 + SIGINT) as u8)
    } else {
        ExitCode::SUCCESS
    };

    result.output?;
    Ok(exit_code)
}
