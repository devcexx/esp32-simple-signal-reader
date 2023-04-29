pub mod commands;
pub mod ctrlc;
pub mod io;

use clap::{Parser, Subcommand};
use commands::{pulse_stream::PulseStreamArgs, read_wav::ReadWavArgs};
use std::process::ExitCode;

#[derive(Subcommand)]
enum Commands {
    ReadWav(ReadWavArgs),
    PulseStream(PulseStreamArgs),
}

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn main() -> anyhow::Result<ExitCode> {
    let cli = <Cli as Parser>::parse();

    match &cli.command {
        Commands::ReadWav(args) => commands::read_wav::run_write_wav_command(args),
        Commands::PulseStream(args) => commands::pulse_stream::run_pulse_stream_command(args),
    }
}
