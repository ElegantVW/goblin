use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "goblin",
    about = "goblin — steals letters into his nest. ask him.",
    disable_help_subcommand = true
)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Summon a goblin (join a sky)
    #[command(name = "summon")]
    Summon {
        #[arg(long)]
        preset: Option<String>,
    },
    /// Name the horde
    Who,
    /// Wake a goblin
    Wake { name: String },
    /// Send a goblin back to the dark
    Dismiss { name: Option<String> },
    /// Mend a goblin’s name, sky, or secret (opens the TUI)
    Mend { name: Option<String> },
    /// Old nest/account words
    #[command(name = "nest", alias = "account", hide = true)]
    Nest {
        #[command(subcommand)]
        action: AccountCmd,
    },
    /// One-shot leftover: copy host/user from aerc
    #[command(hide = true)]
    ImportAerc {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Steal new letters from the sky
    #[command(name = "steal", alias = "sync", alias = "fetch")]
    Steal {
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
        #[arg(long)]
        account: Option<String>,
    },
    /// Watch the sky for new letters
    #[command(name = "watch", alias = "idle")]
    Watch,
    /// Peek at a pile
    #[command(name = "peek", alias = "list")]
    Peek {
        #[arg(default_value = "unread")]
        box_name: String,
        #[arg(long)]
        plain: bool,
    },
    /// Read one letter
    #[command(name = "read", alias = "show")]
    Read {
        file: String,
        #[arg(long)]
        plain: bool,
    },
    /// Unread pile for Pixie
    #[command(name = "pile", alias = "bundle")]
    Pile {
        #[arg(long, default_value_t = 280)]
        snippet: usize,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Mark letters read
    Keep {
        files: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        local_only: bool,
    },
    /// Bin letters
    Trash {
        files: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        local_only: bool,
    },
    /// Old form: move read|trash
    #[command(hide = true)]
    Move {
        dest: String,
        files: Vec<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        local_only: bool,
    },
    /// Send a letter
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
    /// The goblin voice
    #[command(name = "squeak", alias = "sound")]
    Squeak {
        #[arg(long)]
        set: Option<PathBuf>,
    },
    /// Hunt through the nest
    #[command(name = "hunt", alias = "search")]
    Hunt {
        query: Vec<String>,
        #[arg(long)]
        plain: bool,
    },
    /// Parcels on a letter
    #[command(name = "parcel", alias = "attach")]
    Parcel {
        #[command(subcommand)]
        action: AttachCmd,
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
    /// Make this nest the default
    Use { name: String },
    /// Forget a nest (password too)
    #[command(alias = "rm")]
    Remove {
        /// Nest name (default: the current one)
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AttachCmd {
    /// List attachments on a message (file, name, or unread #)
    List { mail: String },
    /// Save attachment N (1-based) to DEST or the current directory
    Save {
        mail: String,
        index: usize,
        dest: Option<PathBuf>,
    },
    /// Open attachment N with the system opener
    Open { mail: String, index: usize },
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
            "steal", "watch", "peek", "read", "pile", "keep", "trash", "send", "squeak", "hunt",
            "parcel", "summon", "who", "wake", "dismiss", "mend",
        ] {
            assert!(help.contains(needle), "missing {needle} in:\n{help}");
        }
        for old in ["sync", "list", "idle", "bundle", "account"] {
            assert!(
                !help.lines().any(|l| l.trim().starts_with(old)),
                "old name {old} should be a hidden alias, not a primary:\n{help}"
            );
        }
    }

    #[test]
    fn old_names_still_parse() {
        use clap::Parser;
        for argv in [
            vec!["goblin", "sync", "--quiet"],
            vec!["goblin", "list", "unread", "--plain"],
            vec!["goblin", "show", "1"],
            vec!["goblin", "bundle"],
            vec!["goblin", "idle"],
            vec!["goblin", "search", "hi"],
            vec!["goblin", "account", "show"],
            vec!["goblin", "sound"],
            vec!["goblin", "attach", "list", "1"],
            vec!["goblin", "move", "trash", "1"],
        ] {
            Args::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
        }
    }
}
