//! Thin engine API for goblin-gui (and tests). No new product behavior.

use crate::cli::{self, load_accounts_file, load_default_account, load_named_account};
use crate::compose;
use crate::config::{Account, AccountFile};
use crate::error::Error;
use crate::imap::{self, SyncOpts};
use crate::smtp;
use crate::store::{MailBox, MailMeta, Store};
use crate::tls;
use std::path::Path;

pub fn accounts() -> Result<AccountFile, Error> {
    load_accounts_file()
}

pub fn default_account_name() -> Result<String, Error> {
    Ok(load_accounts_file()?.default)
}

pub fn list_mails(box_name: MailBox) -> Result<Vec<MailMeta>, Error> {
    let store = Store::default_store();
    let _ = store.ensure();
    store.load_mails(box_name)
}

pub fn steal(account: Option<&str>, force: bool, limit: usize) -> Result<usize, Error> {
    let (acc, password) = match account {
        Some(name) => load_named_account(name)?,
        None => load_default_account()?,
    };
    let store = Store::default_store();
    let result = imap::sync(
        &acc,
        &password,
        &store,
        SyncOpts {
            all: false,
            force,
            limit,
            folder: None,
        },
    )?;
    Ok(result.written)
}

pub fn send_mail(
    account: Option<&str>,
    to: &str,
    cc: &[String],
    subject: &str,
    body: &str,
) -> Result<(), Error> {
    let (acc, password) = match account {
        Some(name) => load_named_account(name)?,
        None => load_default_account()?,
    };
    tls::smtp_mode(acc.smtp.port)?;
    let mut rcpts = vec![to.to_string()];
    rcpts.extend(cc.iter().cloned());
    let raw = compose::compose(&acc.from, to, cc, subject, body);
    smtp::send(&acc, &password, &rcpts, &raw)?;
    let meta = MailMeta {
        uid: format!("sent-{}", chrono::Utc::now().timestamp()),
        account: acc.name.clone(),
        folder: "SENT".into(),
        from: acc.from.clone(),
        to: to.into(),
        date: chrono::Utc::now().to_rfc2822(),
        subject: subject.into(),
        message_id: String::new(),
        attachments: Vec::new(),
        path: None,
        body: String::new(),
    };
    Store::default_store().write_mail(crate::store::MailBox::Read, &meta, body)?;
    Ok(())
}

pub fn save_account(acc: Account, password: &str, make_default: bool) -> Result<(), Error> {
    cli::save_account(acc, password, make_default).map(|_| ())
}

pub fn replace_account(old: &str, acc: Account, password: Option<&str>) -> Result<(), Error> {
    cli::replace_account(old, acc, password).map(|_| ())
}

pub fn remove_account(name: &str) -> Result<String, Error> {
    cli::remove_account(name)
}

pub fn set_default_account(name: &str) -> Result<(), Error> {
    let path = crate::paths::accounts_file();
    let path = if path.extension().and_then(|s| s.to_str()) == Some("gpg") {
        crate::paths::config_dir().join("accounts.json")
    } else {
        path
    };
    let mut file = crate::config::load_accounts(&path)?;
    file.set_default(name)?;
    crate::config::save_accounts(&path, &file)?;
    Ok(())
}

pub fn open_path(path: &Path) -> Result<(), Error> {
    cli::open_path(path)
}

pub fn store() -> Store {
    Store::default_store()
}
