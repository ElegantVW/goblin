//! clap surface for goblind.

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "goblind",
    about = "goblind — company mail spirit. owns the sky. no purelymail.",
    disable_help_subcommand = true
)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Run SMTP + IMAP listeners
    Run {
        /// Bind address
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,
        /// Inbound SMTP (MX) port
        #[arg(long, default_value_t = 25)]
        smtp_in: u16,
        /// Submission SMTP port
        #[arg(long, default_value_t = 587)]
        smtp_sub: u16,
        /// IMAP port
        #[arg(long, default_value_t = 993)]
        imap: u16,
    },
    /// Mailbox users
    User {
        #[command(subcommand)]
        action: UserCmd,
    },
    /// Domain / DNS helpers
    Sky {
        #[command(subcommand)]
        action: SkyCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum UserCmd {
    /// Add a mailbox (creates Maildir)
    Add {
        address: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// Set password
    Passwd {
        address: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// List users
    List,
}

#[derive(Subcommand, Debug)]
pub enum SkyCmd {
    /// Print Squarespace-ready DNS rows (do not apply automatically)
    PrintDns {
        domain: String,
        /// Hostname for MX / A (default: mail.DOMAIN)
        #[arg(long)]
        mail_host: Option<String>,
    },
}
