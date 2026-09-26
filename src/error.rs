use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    Perms { path: PathBuf, mode: u32 },
    TlsPolicy(String),
    Config(String),
    Secret(String),
    Usage(String),
    Io(io::Error),
    Json(serde_json::Error),
    Imap(String),
    Smtp(String),
    Hint { msg: String, next: String },
}

impl Error {
    pub fn hint_line(&self) -> Option<&str> {
        match self {
            Error::Hint { next, .. } => Some(next.as_str()),
            Error::Usage(_) => Some("goblin --help"),
            Error::Config(_) => Some("goblin summon"),
            Error::Perms { .. } => Some("chmod 600 the accounts file"),
            Error::TlsPolicy(_) => Some("use 993 for IMAP or 465/587 for SMTP"),
            Error::Secret(_) => Some("goblin summon"),
            Error::Io(_) => Some("check paths and permissions"),
            Error::Json(_) => Some("goblin summon"),
            Error::Imap(_) => Some("goblin steal --help"),
            Error::Smtp(_) => Some("goblin send --help"),
        }
    }

    pub fn detail(&self) -> Option<String> {
        if std::env::var("FAE_DEBUG").ok().as_deref() != Some("1") {
            return None;
        }
        match self {
            Error::Imap(s)
            | Error::Smtp(s)
            | Error::TlsPolicy(s)
            | Error::Config(s)
            | Error::Secret(s)
            | Error::Usage(s) => Some(s.clone()),
            Error::Io(e) => Some(e.to_string()),
            Error::Json(e) => Some(e.to_string()),
            Error::Perms { path, mode } => {
                Some(format!("{}: {mode:04o}", path.display()))
            }
            Error::Hint { msg, .. } => Some(msg.clone()),
        }
    }

    pub fn hint(self) -> Option<String> {
        self.hint_line().map(str::to_string)
    }

    pub fn say(msg: impl Into<String>, next: impl Into<String>) -> Self {
        Error::Hint {
            msg: msg.into(),
            next: next.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Perms { path, mode } => write!(
                f,
                "accounts file is readable by others ({}: {mode:04o}); chmod 600",
                path.display()
            ),
            Error::TlsPolicy(s) => write!(f, "{s}"),
            Error::Config(s) | Error::Secret(s) | Error::Usage(s) | Error::Imap(s) | Error::Smtp(s) => {
                write!(f, "{s}")
            }
            Error::Hint { msg, .. } => write!(f, "{msg}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Json(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}
