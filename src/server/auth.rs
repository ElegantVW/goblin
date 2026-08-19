//! Password store for goblind users (0600 file). No passwords in JSON.

use super::paths;
use crate::error::Error;
use crate::fsutil;
use std::io::{BufRead, BufReader};

pub fn store_password(address: &str, password: &str) -> Result<(), Error> {
    if password.contains('\n')
        || password.contains('\t')
        || address.contains('\t')
        || address.contains('\n')
    {
        return Err(Error::Secret(
            "password/address must not contain tab or newline".into(),
        ));
    }
    paths::ensure_layout()?;
    let path = paths::secrets_file();
    let mut rows = if path.is_file() {
        if fsutil::is_world_readable(&path)? {
            let mode = fsutil::file_mode(&path)?.unwrap_or(0);
            return Err(Error::Perms { path, mode });
        }
        read_pairs(&path)?
    } else {
        Vec::new()
    };
    if let Some(row) = rows.iter_mut().find(|(k, _)| k == address) {
        row.1 = password.to_string();
    } else {
        rows.push((address.to_string(), password.to_string()));
    }
    write_pairs(&path, &rows)
}

pub fn load_password(address: &str) -> Result<String, Error> {
    let path = paths::secrets_file();
    if !path.is_file() {
        return Err(Error::Secret(format!("no password for {address}")));
    }
    if fsutil::is_world_readable(&path)? {
        let mode = fsutil::file_mode(&path)?.unwrap_or(0);
        return Err(Error::Perms { path, mode });
    }
    for (k, v) in read_pairs(&path)? {
        if k == address {
            return Ok(v);
        }
    }
    Err(Error::Secret(format!("no password for {address}")))
}

pub fn verify(address: &str, password: &str) -> Result<bool, Error> {
    match load_password(address) {
        Ok(stored) => Ok(stored == password),
        Err(Error::Secret(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Lowercase; if `user` has no `@`, append the configured domain.
pub fn canonicalize(user: &str) -> String {
    let a = user.trim().to_ascii_lowercase();
    if a.contains('@') {
        return a;
    }
    match super::config::load_or_empty() {
        Ok(file) if !file.domain.is_empty() => format!("{a}@{}", file.domain),
        _ => a,
    }
}

/// Returns the stored address on success.
pub fn verify_login(user: &str, password: &str) -> Result<Option<String>, Error> {
    let addr = canonicalize(user);
    if verify(&addr, password)? {
        Ok(Some(addr))
    } else {
        Ok(None)
    }
}

fn read_pairs(path: &std::path::Path) -> Result<Vec<(String, String)>, Error> {
    let f = std::fs::File::open(path)?;
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

fn write_pairs(path: &std::path::Path, rows: &[(String, String)]) -> Result<(), Error> {
    let mut text = String::new();
    for (k, v) in rows {
        text.push_str(k);
        text.push('\t');
        text.push_str(v);
        text.push('\n');
    }
    fsutil::write_private(path, text.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::paths::with_goblind_home;

    #[test]
    fn password_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            store_password("design@vanguardaautomovel.com", "s3cret").unwrap();
            assert_eq!(
                load_password("design@vanguardaautomovel.com").unwrap(),
                "s3cret"
            );
            assert!(verify("design@vanguardaautomovel.com", "s3cret").unwrap());
            assert!(!verify("design@vanguardaautomovel.com", "nope").unwrap());
        });
    }
}
