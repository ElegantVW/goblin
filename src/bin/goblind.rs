//! goblind — company mail server binary.

use clap::Parser;
use goblin::server::args::Args;
use std::process::ExitCode;

fn main() -> ExitCode {
    goblin::tls::install_crypto();
    let args = Args::parse();
    match goblin::server::run(args) {
        Ok(c) => ExitCode::from(c),
        Err(e) => {
            eprintln!("goblind: {e}");
            if let Some(n) = e.hint_line() {
                eprintln!("  next:  {n}");
            }
            ExitCode::from(1)
        }
    }
}
