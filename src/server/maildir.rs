//! Maildir (tmp → new) we own. No Dovecot.

use crate::error::Error;
use crate::fsutil;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn ensure(root: &Path) -> Result<(), Error> {
    for sub in ["tmp", "new", "cur"] {
        fsutil::mkdir_private(&root.join(sub))?;
    }
    Ok(())
}

/// Deliver raw RFC5322 bytes into `new/`.
pub fn deliver(root: &Path, raw: &[u8]) -> Result<PathBuf, Error> {
    ensure(root)?;
    let name = unique_name();
    let tmp = root.join("tmp").join(&name);
    let neu = root.join("new").join(&name);
    fsutil::write_private(&tmp, raw)?;
    std::fs::rename(&tmp, &neu)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&neu)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&neu, perms)?;
    }
    Ok(neu)
}

pub fn list_new_and_cur(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut out = Vec::new();
    for sub in ["new", "cur"] {
        let dir = root.join(sub);
        if !dir.is_dir() {
            continue;
        }
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let p = e.path();
            if p.is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn unique_name() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "localhost".into());
    let host: String = host
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{t}.{}.{}", std::process::id(), host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deliver_lands_in_new() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("mail");
        let path = deliver(&root, b"Subject: hi\r\n\r\nbody\r\n").unwrap();
        assert!(path.starts_with(root.join("new")));
        assert_eq!(std::fs::read(&path).unwrap(), b"Subject: hi\r\n\r\nbody\r\n");
        let listed = list_new_and_cur(&root).unwrap();
        assert_eq!(listed.len(), 1);
    }
}
