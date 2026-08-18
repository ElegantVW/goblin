//! OS keyring first, then 0600 secrets file.

use crate::config::check_secret_mode;
use crate::error::Error;
use crate::paths;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

const SERVICE: &str = "goblin";

pub fn store_password(id: &str, password: &str) -> Result<(), Error> {
    if try_keyring_set(id, password).is_ok() {
        return Ok(());
    }
    store_password_in_file(&paths::secrets_file(), id, password)
}

pub fn load_password(id: &str) -> Result<String, Error> {
    if let Ok(p) = try_keyring_get(id) {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    load_password_from_file(&paths::secrets_file(), id)
}

fn try_keyring_set(id: &str, password: &str) -> Result<(), Error> {
    let e = keyring::Entry::new(SERVICE, id)
        .map_err(|e| Error::Secret(format!("keyring: {e}")))?;
    e.set_password(password)
        .map_err(|e| Error::Secret(format!("keyring: {e}")))
}

fn try_keyring_get(id: &str) -> Result<String, Error> {
    let e = keyring::Entry::new(SERVICE, id)
        .map_err(|e| Error::Secret(format!("keyring: {e}")))?;
    e.get_password()
        .map_err(|e| Error::Secret(format!("keyring: {e}")))
}

pub fn store_password_in_file(path: &Path, id: &str, password: &str) -> Result<(), Error> {
    if password.contains('\n') || password.contains('\t') || id.contains('\t') || id.contains('\n') {
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        let mut perms = fs::metadata(parent)?.permissions();
        perms.set_mode(0o700);
        let _ = fs::set_permissions(parent, perms);
    }
    let tmp = path.with_extension("secrets.tmp");
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts.open(&tmp)?;
        for (k, v) in &rows {
            writeln!(f, "{k}\t{v}")?;
        }
    }
    fs::rename(&tmp, path)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
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
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn file_roundtrip_0600() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("secrets");
        store_password_in_file(&path, "ada@example.com", "hunter2").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let got = load_password_from_file(&path, "ada@example.com").unwrap();
        assert_eq!(got, "hunter2");
    }

    #[test]
    fn file_refuses_world_readable() {
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
