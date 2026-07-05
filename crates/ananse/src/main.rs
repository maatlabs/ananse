//! The `ananse` command-line interface.
//!
//! Wraps the decode -> lift -> execute pipeline behind `ananse run`. The
//! `prove` / `verify` subcommands arrive with the prover in a later release and
//! slot into the same [`cmd`] shape.

#![forbid(unsafe_code)]

mod cmd;

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ananse",
    version,
    about = "Ananse: a WebAssembly-native zero-knowledge virtual machine"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Decode, lift, and execute a WebAssembly module, journaling its output.
    Run {
        /// Path to the `.wasm` or `.wat` module.
        module: PathBuf,
        /// Export to invoke; defaults to `_start`, else the first exported function.
        #[arg(long)]
        invoke: Option<String>,
        /// Print an execution summary (step count and elapsed time) to standard error.
        #[arg(short, long)]
        verbose: bool,
        /// Integer arguments passed to the invoked function.
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn main() {
    match Cli::parse().command {
        Some(Command::Run {
            module,
            invoke,
            verbose,
            args,
        }) => cmd::run(&module, invoke.as_deref(), verbose, &args),
        None => {
            eprintln!(
                "Ananse {} ({} {})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH,
            );
            let _ = Cli::command().print_long_help();
        }
    }
}
