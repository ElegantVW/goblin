use crate::{error::Error, AccountCmd, Cmd};

pub fn dispatch(cmd: Cmd) -> Result<u8, Error> {
    match cmd {
        Cmd::Account { action } => match action {
            AccountCmd::Add { .. } => Err(Error::NotImplemented("account add")),
            AccountCmd::Show => Err(Error::NotImplemented("account show")),
        },
        Cmd::ImportAerc { .. } => Err(Error::NotImplemented("import-aerc")),
        Cmd::Sync { .. } => Err(Error::NotImplemented("sync")),
        Cmd::Idle => Err(Error::NotImplemented("idle")),
        Cmd::List { .. } => Err(Error::NotImplemented("list")),
        Cmd::Show { .. } => Err(Error::NotImplemented("show")),
        Cmd::Bundle { .. } => Err(Error::NotImplemented("bundle")),
        Cmd::Move { .. } => Err(Error::NotImplemented("move")),
        Cmd::Send { .. } => Err(Error::NotImplemented("send")),
        Cmd::Sound { .. } => Err(Error::NotImplemented("sound")),
    }
}
