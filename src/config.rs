//! accounts.json — no password fields.

use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

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
        apply_preset("purelymail", "x", "x", user).unwrap().imap
    }

    pub fn purelymail_smtp(user: &str) -> Self {
        apply_preset("purelymail", "x", "x", user).unwrap().smtp
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NestPreset {
    pub id: &'static str,
    pub imap_host: &'static str,
    pub imap_port: u16,
    pub smtp_host: &'static str,
    pub smtp_port: u16,
    pub wants_app_password: bool,
}

pub const NEST_PRESETS: &[NestPreset] = &[
    NestPreset {
        id: "purelymail",
        imap_host: "imap.purelymail.com",
        imap_port: 993,
        smtp_host: "smtp.purelymail.com",
        smtp_port: 465,
        wants_app_password: false,
    },
    NestPreset {
        id: "google",
        imap_host: "imap.gmail.com",
        imap_port: 993,
        smtp_host: "smtp.gmail.com",
        smtp_port: 465,
        wants_app_password: true,
    },
    NestPreset {
        id: "disroot",
        imap_host: "disroot.org",
        imap_port: 993,
        smtp_host: "disroot.org",
        smtp_port: 587,
        wants_app_password: false,
    },
    NestPreset {
        id: "outlook",
        imap_host: "outlook.office365.com",
        imap_port: 993,
        smtp_host: "smtp.office365.com",
        smtp_port: 587,
        wants_app_password: false,
    },
    NestPreset {
        id: "yahoo",
        imap_host: "imap.mail.yahoo.com",
        imap_port: 993,
        smtp_host: "smtp.mail.yahoo.com",
        smtp_port: 465,
        wants_app_password: true,
    },
];

pub fn find_preset(id: &str) -> Option<&'static NestPreset> {
    NEST_PRESETS.iter().find(|p| p.id == id)
}

pub fn apply_preset(id: &str, name: &str, from: &str, user: &str) -> Result<Account, Error> {
    let p = find_preset(id).ok_or_else(|| {
        Error::say(
            format!("unknown nest {id:?}"),
            "try: purelymail, google, disroot, outlook, yahoo",
        )
    })?;
    Ok(Account {
        name: name.into(),
        from: from.into(),
        imap: Endpoint {
            host: p.imap_host.into(),
            port: p.imap_port,
            user: user.into(),
        },
        smtp: Endpoint {
            host: p.smtp_host.into(),
            port: p.smtp_port,
            user: user.into(),
        },
    })
}

pub fn purelymail_preset(name: &str, from: &str, user: &str) -> Account {
    apply_preset("purelymail", name, from, user).expect("purelymail preset")
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

    pub fn account(&self, name: &str) -> Result<&Account, Error> {
        self.accounts
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| Error::Config(format!("no account named {name:?}")))
    }

    pub fn upsert(&mut self, acc: Account) {
        if let Some(slot) = self.accounts.iter_mut().find(|a| a.name == acc.name) {
            *slot = acc;
        } else {
            self.accounts.push(acc);
        }
    }

    pub fn set_default(&mut self, name: &str) -> Result<(), Error> {
        self.account(name)?;
        self.default = name.to_string();
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<Account, Error> {
        let idx = self
            .accounts
            .iter()
            .position(|a| a.name == name)
            .ok_or_else(|| Error::say(format!("no nest named {name:?}"), "goblin nest show"))?;
        let acc = self.accounts.remove(idx);
        if self.default == name {
            self.default = self
                .accounts
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_default();
        }
        Ok(acc)
    }
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
    fn remove_drops_nest_and_repicks_default() {
        let mut file = sample();
        file.upsert(purelymail_preset("home", "Ada <ada@home>", "ada@home"));
        file.set_default("work").unwrap();
        let gone = file.remove("work").unwrap();
        assert_eq!(gone.name, "work");
        assert_eq!(file.accounts.len(), 1);
        assert_eq!(file.default, "home");
        file.remove("home").unwrap();
        assert!(file.accounts.is_empty());
        assert!(file.default.is_empty());
        assert!(file.remove("ghost").is_err());
    }

    #[test]
    fn upsert_keeps_existing_and_can_switch_default() {
        let mut file = sample();
        file.upsert(purelymail_preset(
            "home",
            "Ada <ada@home>",
            "ada@home",
        ));
        assert_eq!(file.accounts.len(), 2);
        file.set_default("home").unwrap();
        assert_eq!(file.default, "home");
        file.upsert(purelymail_preset(
            "work",
            "Ada <ada@work>",
            "ada@work",
        ));
        assert_eq!(file.accounts.len(), 2);
        assert_eq!(file.account("work").unwrap().from, "Ada <ada@work>");
    }

    #[test]
    fn every_preset_uses_tls_ports() {
        for p in NEST_PRESETS {
            crate::tls::imap_mode(p.imap_port).unwrap_or_else(|e| panic!("{} imap: {e}", p.id));
            crate::tls::smtp_mode(p.smtp_port).unwrap_or_else(|e| panic!("{} smtp: {e}", p.id));
        }
        assert!(find_preset("nope").is_none());
        assert!(apply_preset("nope", "a", "b", "c").is_err());
        let a = apply_preset("google", "g", "Me <me@gmail.com>", "me@gmail.com").unwrap();
        assert_eq!(a.imap.host, "imap.gmail.com");
        assert_eq!(a.imap.port, 993);
        assert_eq!(a.smtp.port, 465);
        let d = apply_preset("disroot", "d", "x@disroot.org", "x@disroot.org").unwrap();
        assert_eq!(d.imap.host, "disroot.org");
        assert_eq!(d.smtp.port, 587);
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
