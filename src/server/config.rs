//! accounts.json for goblind — no password fields.

use super::paths;
use crate::error::Error;
use crate::fsutil;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AccountFile {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub users: Vec<User>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub address: String,
    /// Relative to GOBLIND_HOME, e.g. `mail/design`
    pub maildir: String,
}

impl AccountFile {
    pub fn user(&self, address: &str) -> Result<&User, Error> {
        self.users
            .iter()
            .find(|u| u.address == address)
            .ok_or_else(|| Error::say(format!("no user {address}"), "goblind user list"))
    }

    pub fn accepts_recipient(&self, rcpt: &str) -> bool {
        let rcpt = rcpt.trim().to_ascii_lowercase();
        self.users.iter().any(|u| u.address == rcpt)
    }
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

pub fn load() -> Result<AccountFile, Error> {
    let path = paths::accounts_file();
    if !path.is_file() {
        return Err(Error::say("no accounts yet", "goblind user add ADDR"));
    }
    if fsutil::is_world_readable(&path)? {
        let mode = fsutil::file_mode(&path)?.unwrap_or(0);
        return Err(Error::Perms { path, mode });
    }
    let text = std::fs::read_to_string(&path)?;
    let raw: serde_json::Value = serde_json::from_str(&text)?;
    if raw_contains_password(&raw) {
        return Err(Error::Config(
            "accounts file must not contain a password field".into(),
        ));
    }
    Ok(serde_json::from_value(raw)?)
}

pub fn load_or_empty() -> Result<AccountFile, Error> {
    let path = paths::accounts_file();
    if !path.is_file() {
        return Ok(AccountFile::default());
    }
    load()
}

pub fn save(file: &AccountFile) -> Result<(), Error> {
    paths::ensure_layout()?;
    let path = paths::accounts_file();
    let json = serde_json::to_string_pretty(file)?;
    fsutil::write_private(&path, format!("{json}\n").as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::paths::with_goblind_home;

    #[test]
    fn save_load_roundtrip_no_password() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            let file = AccountFile {
                domain: "vanguardaautomovel.com".into(),
                users: vec![User {
                    address: "design@vanguardaautomovel.com".into(),
                    maildir: "mail/design".into(),
                }],
            };
            save(&file).unwrap();
            let loaded = load().unwrap();
            assert_eq!(loaded, file);
            let text = std::fs::read_to_string(paths::accounts_file()).unwrap();
            assert!(!text.contains("password"));
        });
    }
}
