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
            Error::Usage(_) => None,
            Error::Config(_) => Some("goblin summon"),
            _ => None,
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
