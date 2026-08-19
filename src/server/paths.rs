//! goblind data home. Override with GOBLIND_HOME.

use crate::error::Error;
use crate::fsutil;
use std::path::{Path, PathBuf};

fn home_root() -> PathBuf {
    if let Some(v) = std::env::var_os("GOBLIND_HOME") {
        let p = PathBuf::from(&v);
        if !v.to_string_lossy().trim().is_empty() {
            return p;
        }
    }
    directories::ProjectDirs::from("", "faeos", "goblind")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("goblind-data"))
}

pub fn home() -> PathBuf {
    home_root()
}

pub fn accounts_file() -> PathBuf {
    home().join("accounts.json")
}

pub fn secrets_file() -> PathBuf {
    home().join("secrets")
}

pub fn maildir_root() -> PathBuf {
    home().join("mail")
}

pub fn queue_dir() -> PathBuf {
    home().join("queue")
}

pub fn queue_failed_dir() -> PathBuf {
    queue_dir().join("failed")
}

pub fn dkim_dir() -> PathBuf {
    home().join("dkim")
}

pub fn tls_dir() -> PathBuf {
    home().join("tls")
}

pub fn ensure_layout() -> Result<(), Error> {
    let h = home();
    fsutil::mkdir_private(&h)?;
    for sub in ["mail", "queue", "queue/failed", "dkim", "tls"] {
        fsutil::mkdir_private(&h.join(sub))?;
    }
    Ok(())
}

pub fn user_maildir(rel_or_abs: &str) -> PathBuf {
    let p = Path::new(rel_or_abs);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        home().join(p)
    }
}

#[cfg(test)]
pub fn with_goblind_home<R>(home: Option<&Path>, f: impl FnOnce() -> R) -> R {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev = std::env::var_os("GOBLIND_HOME");
    unsafe {
        match home {
            Some(p) => std::env::set_var("GOBLIND_HOME", p),
            None => std::env::remove_var("GOBLIND_HOME"),
        }
    }
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    unsafe {
        match prev {
            Some(v) => std::env::set_var("GOBLIND_HOME", v),
            None => std::env::remove_var("GOBLIND_HOME"),
        }
    }
    drop(guard);
    match out {
        Ok(v) => v,
        Err(p) => std::panic::resume_unwind(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goblind_home_redirects() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            assert_eq!(home(), dir.path());
            assert_eq!(accounts_file(), dir.path().join("accounts.json"));
            assert_eq!(maildir_root(), dir.path().join("mail"));
            ensure_layout().unwrap();
            assert!(dir.path().join("queue").is_dir());
            assert!(dir.path().join("queue").join("failed").is_dir());
        });
    }
}
