//! Goblin — mail client library (same crate as the binary).

pub mod args;
pub mod cli;
pub mod compose;
pub mod config;
pub mod error;
pub mod fsutil;
pub mod imap;
pub mod import_aerc;
pub mod notify;
pub mod paths;
pub mod search;
pub mod secrets;
pub mod server;
pub mod smtp;
pub mod store;
pub mod termart;
pub mod tls;
pub mod tui;

use clap::Parser;
use std::process::ExitCode;

pub use args::{AccountCmd, Args, AttachCmd, Cmd};

pub fn run_main() -> ExitCode {
    crate::tls::install_crypto();
    let args = Args::parse();
    let code = match run(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("goblin: {e}");
            if let Some(n) = e.hint_line() {
                eprintln!("  next:  {n}");
            }
            return ExitCode::from(1);
        }
    };
    ExitCode::from(code)
}

fn run(args: Args) -> Result<u8, error::Error> {
    match args.cmd {
        None => tui::run(),
        Some(cmd) => cli::dispatch(cmd),
    }
}
