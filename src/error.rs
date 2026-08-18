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
