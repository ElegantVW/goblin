//! goblin — mail client. owns its accounts. no aerc.

mod cli;
mod compose;
mod config;
mod error;
mod imap;
mod import_aerc;
mod notify;
mod paths;
mod secrets;
mod smtp;
mod store;
mod termart;
mod tls;
mod tui;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "goblin",
    about = "goblin — mail client. owns its accounts. no aerc.",
    disable_help_subcommand = true
)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Add or show the default account
    Account {
        #[command(subcommand)]
        action: AccountCmd,
    },
    /// One-shot: copy host/user from aerc (never writes the password to JSON)
    ImportAerc {
        /// Path to aerc accounts.conf (default: ~/.config/aerc/accounts.conf)
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Fetch mail into the local unread box
    Sync {
        #[arg(long)]
        quiet: bool,
        #[arg(long)]
        no_notify: bool,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        folder: Option<String>,
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    /// IMAP IDLE watcher — instant new-mail push
    Idle,
    /// List a box
    List {
        #[arg(default_value = "unread")]
        box_name: String,
        #[arg(long)]
        plain: bool,
    },
    /// Print one mail file
    Show {
        file: String,
        #[arg(long)]
        plain: bool,
    },
    /// Compact unread digest (Pixie)
    Bundle {
        #[arg(long, default_value_t = 280)]
        snippet: usize,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Move local files and update IMAP
    Move {
        dest: String,
        files: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        local_only: bool,
    },
    /// Send a message
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        subject: String,
        #[arg(long)]
        cc: Option<String>,
        #[arg(long)]
        body_file: Option<PathBuf>,
    },
    /// Play or install the notify sound
    Sound {
        #[arg(long)]
        set: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AccountCmd {
    /// Prompt for account details and store the password
    Add {
        #[arg(long)]
        preset: Option<String>,
    },
    /// Print account hosts and user (never the password)
    Show,
}

fn main() -> ExitCode {
    crate::tls::install_crypto();
    let args = Args::parse();
    let code = match run(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("goblin: {e}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn clap_surface_includes_locked_commands() {
        let mut cmd = Args::command();
        let help = cmd.render_long_help().to_string();
        for needle in [
            "account",
            "import-aerc",
            "sync",
            "idle",
            "list",
            "show",
            "bundle",
            "move",
            "send",
            "sound",
        ] {
            assert!(help.contains(needle), "missing {needle} in:\n{help}");
        }
    }
}
