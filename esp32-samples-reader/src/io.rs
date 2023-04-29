use std::{
    io::ErrorKind,
    time::Duration,
};

use serialport::TTYPort;

#[inline(always)]
pub fn bit_sample_to_signed8(sample: bool) -> i8 {
    if sample {
        127
    } else {
        -128
    }
}

#[inline(always)]
pub fn bit_sample_to_unsigned8_full_range(sample: bool) -> u8 {
    if sample {
        255
    } else {
        0
    }
}

#[inline(always)]
pub fn bit_sample_to_unsigned8_half_range(sample: bool) -> u8 {
    if sample {
        255
    } else {
        127
    }
}

#[inline(always)]
pub fn decode_esp32_sample(input: u8) -> [i8; 8] {
    [
        bit_sample_to_signed8(((input >> 7) & 1) != 0),
        bit_sample_to_signed8(((input >> 6) & 1) != 0),
        bit_sample_to_signed8(((input >> 5) & 1) != 0),
        bit_sample_to_signed8(((input >> 4) & 1) != 0),
        bit_sample_to_signed8(((input >> 3) & 1) != 0),
        bit_sample_to_signed8(((input >> 2) & 1) != 0),
        bit_sample_to_signed8(((input >> 1) & 1) != 0),
        bit_sample_to_signed8(((input >> 0) & 1) != 0),
    ]
}

#[inline(always)]
pub fn decode_esp32_sample_unsigned_full_range(input: u8) -> [u8; 8] {
    [
        bit_sample_to_unsigned8_full_range(((input >> 7) & 1) != 0),
        bit_sample_to_unsigned8_full_range(((input >> 6) & 1) != 0),
        bit_sample_to_unsigned8_full_range(((input >> 5) & 1) != 0),
        bit_sample_to_unsigned8_full_range(((input >> 4) & 1) != 0),
        bit_sample_to_unsigned8_full_range(((input >> 3) & 1) != 0),
        bit_sample_to_unsigned8_full_range(((input >> 2) & 1) != 0),
        bit_sample_to_unsigned8_full_range(((input >> 1) & 1) != 0),
        bit_sample_to_unsigned8_full_range(((input >> 0) & 1) != 0),
    ]
}

#[inline(always)]
pub fn decode_esp32_sample_unsigned_half_range(input: u8) -> [u8; 8] {
    [
        bit_sample_to_unsigned8_half_range(((input >> 7) & 1) != 0),
        bit_sample_to_unsigned8_half_range(((input >> 6) & 1) != 0),
        bit_sample_to_unsigned8_half_range(((input >> 5) & 1) != 0),
        bit_sample_to_unsigned8_half_range(((input >> 4) & 1) != 0),
        bit_sample_to_unsigned8_half_range(((input >> 3) & 1) != 0),
        bit_sample_to_unsigned8_half_range(((input >> 2) & 1) != 0),
        bit_sample_to_unsigned8_half_range(((input >> 1) & 1) != 0),
        bit_sample_to_unsigned8_half_range(((input >> 0) & 1) != 0),
    ]
}

pub fn open_serial_port(path: &str, baud_rate: u32, timeout: Duration) -> anyhow::Result<TTYPort> {
    Ok(serialport::new(path, baud_rate)
        .data_bits(serialport::DataBits::Eight)
        .stop_bits(serialport::StopBits::One)
        .parity(serialport::Parity::None)
        .flow_control(serialport::FlowControl::None)
        .timeout(timeout)
        .open_native()?)
}

pub fn recover_if_interrupted<A, F: FnOnce() -> std::io::Result<A>, R: FnOnce() -> A>(
    f: F,
    r: R,
) -> std::io::Result<A> {
    match f() {
        Ok(result) => Ok(result),
        Err(error) => match error.kind() {
            ErrorKind::Interrupted => Ok(r()),
            _ => Err(error),
        },
    }
}

pub fn retry_if_interrupted<
    A,
    E,
    F: FnMut() -> std::result::Result<A, E>,
    M: Fn(&E) -> Option<&std::io::Error>,
>(
    mut f: F,
    error_mapper: M,
) -> std::result::Result<A, E> {
    let mut result: std::result::Result<A, E>;
    while {
        result = f();

        let should_retry = match &result {
            Ok(_) => false,
            Err(e) => error_mapper(e)
                .map(|e| e.kind() == ErrorKind::Interrupted)
                .unwrap_or(false),
        };

        should_retry
    } {}

    result
}
