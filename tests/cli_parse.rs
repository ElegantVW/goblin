use clap::Parser;
use goblin::{Args, Cmd};

#[test]
fn primaries_parse() {
    for argv in [
        vec!["goblin", "summon", "--preset", "google"],
        vec!["goblin", "who"],
        vec!["goblin", "wake", "work"],
        vec!["goblin", "mend", "work"],
        vec!["goblin", "dismiss", "work"],
        vec!["goblin", "steal"],
        vec!["goblin", "peek", "unread", "--plain"],
        vec!["goblin", "hunt", "invoice"],
    ] {
        Args::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
    }
}

#[test]
fn hidden_aliases_still_parse() {
    Args::try_parse_from(["goblin", "sync", "--quiet"]).unwrap();
    Args::try_parse_from(["goblin", "account", "show"]).unwrap();
    Args::try_parse_from(["goblin", "nest", "show"]).unwrap();
}

#[test]
fn summon_is_not_hidden() {
    match Args::try_parse_from(["goblin", "summon"]).unwrap().cmd {
        Some(Cmd::Summon { .. }) => {}
        other => panic!("{other:?}"),
    }
}
