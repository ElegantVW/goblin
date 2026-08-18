//! Local unread/read/trash text cache.

use crate::error::Error;
use crate::paths;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailBox {
    Unread,
    Read,
    Trash,
}

impl MailBox {
    pub fn as_str(self) -> &'static str {
        match self {
            MailBox::Unread => "unread",
            MailBox::Read => "read",
            MailBox::Trash => "trash",
        }
    }

    pub fn parse(s: &str) -> Result<Self, Error> {
        match s {
            "unread" => Ok(MailBox::Unread),
            "read" => Ok(MailBox::Read),
            "trash" => Ok(MailBox::Trash),
            other => Err(Error::Usage(format!(
                "box must be unread|read|trash, not {other:?}"
            ))),
        }
    }

    pub fn all() -> [MailBox; 3] {
        [MailBox::Unread, MailBox::Read, MailBox::Trash]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailMeta {
    pub uid: String,
    pub account: String,
    pub folder: String,
    pub from: String,
    pub to: String,
    pub date: String,
    pub subject: String,
    pub message_id: String,
    pub path: Option<PathBuf>,
    pub body: String,
}

impl MailMeta {
    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_store() -> Self {
        Self::new(paths::mail_root())
    }

    pub fn ensure(&self) -> Result<(), Error> {
        for b in MailBox::all() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(self.box_dir(b))?;
        }
        Ok(())
    }

    pub fn box_dir(&self, box_name: MailBox) -> PathBuf {
        self.root.join(box_name.as_str())
    }

    pub fn write_mail(
        &self,
        box_name: MailBox,
        meta: &MailMeta,
        body: &str,
    ) -> Result<PathBuf, Error> {
        self.ensure()?;
        let mut path = self.box_dir(box_name).join(safe_filename(&meta.uid, &meta.subject));
        if path.exists() {
            let existing = fs::read_to_string(&path).unwrap_or_default();
            if !existing.contains(&format!("uid: {}", meta.uid)) {
                path = self.box_dir(box_name).join(format!("uid{}.txt", meta.uid));
            }
        }
        let text = format_mail(meta, body);
        atomic_write_0600(&path, &text)?;
        Ok(path)
    }

    pub fn load_mails(&self, box_name: MailBox) -> Result<Vec<MailMeta>, Error> {
        let dir = self.box_dir(box_name);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut mails = Vec::new();
        let mut entries: Vec<PathBuf> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("txt"))
            .collect();
        entries.sort();
        for p in entries {
            match parse_mail_file(&p) {
                Ok(m) => mails.push(m),
                Err(_) => continue,
            }
        }
        Ok(mails)
    }

    pub fn parse_path(&self, path: &Path) -> Result<MailMeta, Error> {
        parse_mail_file(path)
    }

    pub fn move_mail(&self, path: &Path, dest: MailBox) -> Result<PathBuf, Error> {
        self.ensure()?;
        let dest_dir = self.box_dir(dest);
        let name = path
            .file_name()
            .ok_or_else(|| Error::Usage("bad mail path".into()))?;
        let dest_path = dest_dir.join(name);
        fs::rename(path, &dest_path)?;
        Ok(dest_path)
    }

    pub fn known_uids(&self) -> Result<HashSet<String>, Error> {
        let mut uids = HashSet::new();
        for b in MailBox::all() {
            for m in self.load_mails(b)? {
                if !m.uid.is_empty() {
                    uids.insert(m.uid);
                }
            }
        }
        Ok(uids)
    }
}

pub fn parse_mail_file(path: &Path) -> Result<MailMeta, Error> {
    let text = fs::read_to_string(path)?;
    let (headers, body) = match text.split_once("\n---\n") {
        Some((h, b)) => (h, b.trim_end().to_string()),
        None => (text.as_str(), String::new()),
    };
    let mut meta = MailMeta {
        path: Some(path.to_path_buf()),
        body,
        folder: "INBOX".into(),
        ..MailMeta::default()
    };
    for line in headers.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().to_string();
            match k.trim().to_ascii_lowercase().as_str() {
                "uid" => meta.uid = v,
                "account" => meta.account = v,
                "folder" => meta.folder = v,
                "from" => meta.from = v,
                "to" => meta.to = v,
                "date" => meta.date = v,
                "subject" => meta.subject = v,
                "message-id" => meta.message_id = v,
                _ => {}
            }
        }
    }
    Ok(meta)
}

fn format_mail(meta: &MailMeta, body: &str) -> String {
    let body = if body.is_empty() {
        "(empty body)"
    } else {
        body
    };
    format!(
        "uid: {}\naccount: {}\nfolder: {}\nfrom: {}\nto: {}\ndate: {}\nsubject: {}\nmessage-id: {}\n---\n{}\n",
        meta.uid,
        meta.account,
        if meta.folder.is_empty() {
            "INBOX"
        } else {
            &meta.folder
        },
        meta.from,
        meta.to,
        meta.date,
        meta.subject,
        meta.message_id,
        body
    )
}

fn safe_filename(uid: &str, subject: &str) -> String {
    let mut sub: String = subject
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_'))
        .take(60)
        .collect();
    sub = sub.trim().to_string();
    if sub.is_empty() {
        sub = "no-subject".into();
    }
    let sub = sub.split_whitespace().collect::<Vec<_>>().join("_");
    format!("uid{uid}_{sub}.txt")
}

fn atomic_write_0600(path: &Path, text: &str) -> Result<(), Error> {
    let tmp = path.with_extension("txt.tmp");
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts.open(&tmp)?;
        f.write_all(text.as_bytes())?;
    }
    fs::rename(&tmp, path)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample() -> MailMeta {
        MailMeta {
            uid: "42".into(),
            account: "work".into(),
            folder: "INBOX".into(),
            from: "Ada <ada@example.com>".into(),
            to: "you@example.com".into(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".into(),
            subject: "Hello, goblin!".into(),
            message_id: "<x@y>".into(),
            path: None,
            body: String::new(),
        }
    }

    #[test]
    fn write_has_uid_separator_and_mode_0600() {
        let dir = tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let path = store
            .write_mail(MailBox::Unread, &sample(), "hi there")
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("uid: 42\n"), "{text}");
        assert!(text.contains("\n---\n"), "{text}");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let parsed = parse_mail_file(&path).unwrap();
        assert_eq!(parsed.uid, "42");
        assert_eq!(parsed.body, "hi there");
        assert_eq!(parsed.subject, "Hello, goblin!");
    }

    #[test]
    fn known_uids_unions_all_boxes() {
        let dir = tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let mut a = sample();
        a.uid = "1".into();
        store.write_mail(MailBox::Unread, &a, "a").unwrap();
        let mut b = sample();
        b.uid = "2".into();
        store.write_mail(MailBox::Read, &b, "b").unwrap();
        let mut c = sample();
        c.uid = "3".into();
        store.write_mail(MailBox::Trash, &c, "c").unwrap();
        let uids = store.known_uids().unwrap();
        assert_eq!(uids.len(), 3);
        assert!(uids.contains("1") && uids.contains("2") && uids.contains("3"));
    }

    #[test]
    fn move_between_boxes() {
        let dir = tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let path = store.write_mail(MailBox::Unread, &sample(), "x").unwrap();
        let dest = store.move_mail(&path, MailBox::Read).unwrap();
        assert!(dest.starts_with(store.box_dir(MailBox::Read)));
        assert!(!path.exists());
        assert_eq!(store.load_mails(MailBox::Unread).unwrap().len(), 0);
        assert_eq!(store.load_mails(MailBox::Read).unwrap().len(), 1);
    }
}
