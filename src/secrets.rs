//! Optional OS keyring (`keyring` feature), then 0600 secrets file.

use crate::config::check_secret_mode;
use crate::error::Error;
use crate::paths;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[cfg(feature = "keyring")]
const SERVICE: &str = "goblin";

pub fn store_password(id: &str, password: &str) -> Result<(), Error> {
    #[cfg(feature = "keyring")]
    {
        if try_keyring_set(id, password).is_ok() {
            return Ok(());
        }
    }
    store_password_in_file(&paths::secrets_file(), id, password)
}

pub fn delete_password(id: &str) -> Result<(), Error> {
    #[cfg(feature = "keyring")]
    {
        let _ = try_keyring_delete(id);
    }
    let path = paths::secrets_file();
    if path.is_file() {
        delete_password_from_file(&path, id)?;
    }
    Ok(())
}

#[cfg(feature = "keyring")]
fn try_keyring_delete(id: &str) -> Result<(), Error> {
    let e = keyring::Entry::new(SERVICE, id).map_err(|e| Error::Secret(format!("keyring: {e}")))?;
    e.delete_credential()
        .map_err(|e| Error::Secret(format!("keyring: {e}")))
}

pub fn load_password(id: &str) -> Result<String, Error> {
    #[cfg(feature = "keyring")]
    {
        if let Ok(p) = try_keyring_get(id) {
            if !p.is_empty() {
                return Ok(p);
            }
        }
    }
    load_password_from_file(&paths::secrets_file(), id)
}

#[cfg(feature = "keyring")]
fn try_keyring_set(id: &str, password: &str) -> Result<(), Error> {
    let e = keyring::Entry::new(SERVICE, id).map_err(|e| Error::Secret(format!("keyring: {e}")))?;
    e.set_password(password)
        .map_err(|e| Error::Secret(format!("keyring: {e}")))
}

#[cfg(feature = "keyring")]
fn try_keyring_get(id: &str) -> Result<String, Error> {
    let e = keyring::Entry::new(SERVICE, id).map_err(|e| Error::Secret(format!("keyring: {e}")))?;
    e.get_password()
        .map_err(|e| Error::Secret(format!("keyring: {e}")))
}

pub fn store_password_in_file(path: &Path, id: &str, password: &str) -> Result<(), Error> {
    if password.contains('\n') || password.contains('\t') || id.contains('\t') || id.contains('\n')
    {
        return Err(Error::Secret(
            "password/id must not contain tab or newline".into(),
        ));
    }
    let mut rows: Vec<(String, String)> = if path.is_file() {
        check_secret_mode(path)?;
        read_pairs(path)?
    } else {
        Vec::new()
    };
    if let Some(row) = rows.iter_mut().find(|(k, _)| k == id) {
        row.1 = password.to_string();
    } else {
        rows.push((id.to_string(), password.to_string()));
    }
    write_pairs(path, &rows)
}

pub fn delete_password_from_file(path: &Path, id: &str) -> Result<(), Error> {
    if !path.is_file() {
        return Ok(());
    }
    check_secret_mode(path)?;
    let rows: Vec<(String, String)> = read_pairs(path)?
        .into_iter()
        .filter(|(k, _)| k != id)
        .collect();
    if rows.is_empty() {
        let _ = fs::remove_file(path);
        return Ok(());
    }
    write_pairs(path, &rows)
}

pub fn load_password_from_file(path: &Path, id: &str) -> Result<String, Error> {
    if !path.is_file() {
        return Err(Error::Secret(format!(
            "no password for {id} (keyring empty and no secrets file)"
        )));
    }
    check_secret_mode(path)?;
    for (k, v) in read_pairs(path)? {
        if k == id {
            return Ok(v);
        }
    }
    Err(Error::Secret(format!("no password for {id}")))
}

fn write_pairs(path: &Path, rows: &[(String, String)]) -> Result<(), Error> {
    let mut text = String::new();
    for (k, v) in rows {
        text.push_str(k);
        text.push('\t');
        text.push_str(v);
        text.push('\n');
    }
    crate::fsutil::write_private(path, text.as_bytes())?;
    Ok(())
}

fn read_pairs(path: &Path) -> Result<Vec<(String, String)>, Error> {
    let f = fs::File::open(path)?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line
            .split_once('\t')
            .ok_or_else(|| Error::Secret("secrets file is malformed".into()))?;
        out.push((k.to_string(), v.to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn file_roundtrip_0600() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("secrets");
        store_password_in_file(&path, "ada@example.com", "hunter2").unwrap();
        #[cfg(unix)]
        {
            assert_eq!(crate::fsutil::file_mode(&path).unwrap(), Some(0o600));
        }
        let got = load_password_from_file(&path, "ada@example.com").unwrap();
        assert_eq!(got, "hunter2");
    }

    #[cfg(unix)]
    #[test]
    fn file_refuses_world_readable() {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let dir = tempdir().unwrap();
        let path = dir.path().join("secrets");
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).mode(0o644);
        let mut f = opts.open(&path).unwrap();
        writeln!(f, "ada@example.com\thunter2").unwrap();
        drop(f);
        let err = load_password_from_file(&path, "ada@example.com").unwrap_err();
        match err {
            Error::Perms { mode, .. } => assert_eq!(mode, 0o644),
            other => panic!("expected Perms, got {other}"),
        }
    }

    #[test]
    fn delete_removes_row() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("secrets");
        store_password_in_file(&path, "ada@x", "one").unwrap();
        store_password_in_file(&path, "bob@x", "two").unwrap();
        delete_password_from_file(&path, "ada@x").unwrap();
        assert!(load_password_from_file(&path, "ada@x").is_err());
        assert_eq!(load_password_from_file(&path, "bob@x").unwrap(), "two");
        delete_password_from_file(&path, "bob@x").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn update_existing_id() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("secrets");
        store_password_in_file(&path, "ada@example.com", "one").unwrap();
        store_password_in_file(&path, "ada@example.com", "two").unwrap();
        assert_eq!(
            load_password_from_file(&path, "ada@example.com").unwrap(),
            "two"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1);
    }
}
