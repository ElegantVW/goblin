//! accounts.json — no password fields.

use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountFile {
    pub default: String,
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub from: String,
    pub imap: Endpoint,
    pub smtp: Endpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl Endpoint {
    pub fn purelymail_imap(user: &str) -> Self {
        Self {
            host: "imap.purelymail.com".into(),
            port: 993,
            user: user.into(),
        }
    }

    pub fn purelymail_smtp(user: &str) -> Self {
        Self {
            host: "smtp.purelymail.com".into(),
            port: 465,
            user: user.into(),
        }
    }
}

pub fn purelymail_preset(name: &str, from: &str, user: &str) -> Account {
    Account {
        name: name.into(),
        from: from.into(),
        imap: Endpoint::purelymail_imap(user),
        smtp: Endpoint::purelymail_smtp(user),
    }
}

/// Fail closed if group/other have any permission bits.
pub fn check_secret_mode(path: &Path) -> Result<(), Error> {
    let meta = fs::metadata(path)?;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(Error::Perms {
            path: path.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

fn raw_contains_password(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(map) => {
            if map.contains_key("password") {
                return true;
            }
            map.values().any(raw_contains_password)
        }
        serde_json::Value::Array(items) => items.iter().any(raw_contains_password),
        _ => false,
    }
}

fn decrypt_gpg(path: &Path) -> Result<String, Error> {
    let out = std::process::Command::new("gpg")
        .args(["--decrypt", "--quiet", "--batch"])
        .arg(path)
        .output()
        .map_err(|e| Error::Config(format!("gpg: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Config(format!("gpg decrypt failed: {err}")));
    }
    String::from_utf8(out.stdout).map_err(|e| Error::Config(format!("gpg utf8: {e}")))
}

pub fn load_accounts(path: &Path) -> Result<AccountFile, Error> {
    let text = if path.extension().and_then(|s| s.to_str()) == Some("gpg") {
        decrypt_gpg(path)?
    } else {
        check_secret_mode(path)?;
        fs::read_to_string(path)?
    };
    let raw: serde_json::Value = serde_json::from_str(&text)?;
    if raw_contains_password(&raw) {
        return Err(Error::Config(
            "accounts file must not contain a password field".into(),
        ));
    }
    let file: AccountFile = serde_json::from_value(raw)?;
    if file.accounts.is_empty() {
        return Err(Error::Config("no accounts".into()));
    }
    if !file.accounts.iter().any(|a| a.name == file.default) {
        return Err(Error::Config(format!(
            "default account {:?} not found",
            file.default
        )));
    }
    Ok(file)
}

pub fn save_accounts(path: &Path, file: &AccountFile) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(parent)?.permissions();
            perms.set_mode(0o700);
            fs::set_permissions(parent, perms)?;
        }
    }
    let json = serde_json::to_string_pretty(file)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        use std::io::Write;
        let mut f = opts.open(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.write_all(b"\n")?;
    }
    fs::rename(&tmp, path)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

impl AccountFile {
    pub fn default_account(&self) -> Result<&Account, Error> {
        self.accounts
            .iter()
            .find(|a| a.name == self.default)
            .ok_or_else(|| Error::Config(format!("default account {:?} not found", self.default)))
    }
}

pub fn default_accounts_path() -> PathBuf {
    crate::paths::accounts_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn sample() -> AccountFile {
        AccountFile {
            default: "work".into(),
            accounts: vec![purelymail_preset(
                "work",
                "Ada <ada@example.com>",
                "ada@example.com",
            )],
        }
    }

    fn write_mode(path: &Path, text: &str, mode: u32) {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(mode);
        let mut f = opts.open(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn refuses_group_or_other_readable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        write_mode(&path, &serde_json::to_string(&sample()).unwrap(), 0o644);
        let err = load_accounts(&path).unwrap_err();
        match err {
            Error::Perms { mode, .. } => assert_eq!(mode, 0o644),
            other => panic!("expected Perms, got {other}"),
        }
    }

    #[test]
    fn loads_mode_0600() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        let file = sample();
        write_mode(&path, &serde_json::to_string_pretty(&file).unwrap(), 0o600);
        let loaded = load_accounts(&path).unwrap();
        assert_eq!(loaded, file);
    }

    #[test]
    fn rejects_password_field() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        let mut v = serde_json::to_value(sample()).unwrap();
        v["accounts"][0]["password"] = serde_json::json!("s3cret");
        write_mode(&path, &serde_json::to_string(&v).unwrap(), 0o600);
        let err = load_accounts(&path).unwrap_err();
        match err {
            Error::Config(s) => assert!(s.contains("password"), "{s}"),
            other => panic!("expected Config, got {other}"),
        }
    }

    #[test]
    fn save_never_writes_password_and_is_0600() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        save_accounts(&path, &sample()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("password"));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
