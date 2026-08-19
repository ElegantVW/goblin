//! Maildir (tmp → new) we own. No Dovecot.
//!
//! Stable IMAP UIDs: `.uidvalidity` + `.uidnext` in the mailbox root, and `,U={uid}`
//! in the unique filename (`new/…,U=1` or `cur/…,U=1:2,S`).

use crate::error::Error;
use crate::fsutil;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static UID_LOCK: Mutex<()> = Mutex::new(());

pub fn ensure(root: &Path) -> Result<(), Error> {
    for sub in ["tmp", "new", "cur"] {
        fsutil::mkdir_private(&root.join(sub))?;
    }
    Ok(())
}

/// Deliver raw RFC5322 bytes into `new/`, assigning a stable UID.
pub fn deliver(root: &Path, raw: &[u8]) -> Result<PathBuf, Error> {
    ensure(root)?;
    let _g = uid_lock();
    ensure_uidvalidity_locked(root)?;
    let uid = take_uid_locked(root)?;
    let name = format!("{},U={uid}", unique_name());
    let tmp = root.join("tmp").join(&name);
    let neu = root.join("new").join(&name);
    fsutil::write_private(&tmp, raw)?;
    std::fs::rename(&tmp, &neu)?;
    chmod_600(&neu)?;
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
            if p.is_file() && is_mail_file(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub uid: u32,
    pub flags: String,
    pub recent: bool,
}

impl Entry {
    pub fn seen(&self) -> bool {
        self.flags.contains('S')
    }

    pub fn deleted(&self) -> bool {
        self.flags.contains('T')
    }

    pub fn imap_flags(&self) -> String {
        let mut f = Vec::new();
        if self.flags.contains('R') {
            f.push("\\Answered");
        }
        if self.flags.contains('F') {
            f.push("\\Flagged");
        }
        if self.flags.contains('T') {
            f.push("\\Deleted");
        }
        if self.flags.contains('S') {
            f.push("\\Seen");
        }
        if self.flags.contains('D') {
            f.push("\\Draft");
        }
        if self.recent {
            f.push("\\Recent");
        }
        f.join(" ")
    }
}

/// UIDVALIDITY for this mailbox (created on first use).
pub fn uidvalidity(root: &Path) -> Result<u32, Error> {
    ensure(root)?;
    let _g = uid_lock();
    ensure_uidvalidity_locked(root)
}

/// Next UID that will be assigned.
pub fn uidnext(root: &Path) -> Result<u32, Error> {
    ensure(root)?;
    let _g = uid_lock();
    ensure_uidvalidity_locked(root)?;
    Ok(read_num(&root.join(".uidnext")).unwrap_or(1).max(1))
}

/// Messages in `new/` + `cur/` with stable UIDs, sorted by UID.
/// Assigns UIDs (and rewrites names) for files that lack `,U=`.
pub fn indexed(root: &Path) -> Result<Vec<Entry>, Error> {
    ensure(root)?;
    let _g = uid_lock();
    ensure_uidvalidity_locked(root)?;
    let mut entries = collect_locked(root)?;
    let mut dirty = false;
    for e in &mut entries {
        if e.uid == 0 {
            let uid = take_uid_locked(root)?;
            *e = rename_locked(e, uid, &e.flags, e.recent)?;
            dirty = true;
        }
    }
    if dirty {
        entries.sort_by_key(|e| e.uid);
    }
    bump_uidnext_locked(root, &entries)?;
    Ok(entries)
}

pub fn read_bytes(entry: &Entry) -> Result<Vec<u8>, Error> {
    Ok(std::fs::read(&entry.path)?)
}

/// Move `new/` → `cur/` with `:2,{flags}` (keeps UID). Used on FETCH.
pub fn to_cur(entry: &Entry) -> Result<Entry, Error> {
    if !entry.recent {
        return Ok(entry.clone());
    }
    let _g = uid_lock();
    rename_locked(entry, entry.uid, &entry.flags, false)
}

/// Apply IMAP flag changes (`+` / `-` / replace) and persist via the filename.
pub fn store_flags(entry: &Entry, mode: FlagMode, imap_flags: &[&str]) -> Result<Entry, Error> {
    let mut set: Vec<char> = entry.flags.chars().collect();
    match mode {
        FlagMode::Replace => set.clear(),
        FlagMode::Add | FlagMode::Remove => {}
    }
    for f in imap_flags {
        if let Some(c) = maildir_flag(f) {
            match mode {
                FlagMode::Add | FlagMode::Replace => {
                    if !set.contains(&c) {
                        set.push(c);
                    }
                }
                FlagMode::Remove => set.retain(|x| *x != c),
            }
        }
    }
    set.sort_unstable();
    set.dedup();
    let flags: String = set.into_iter().collect();
    let _g = uid_lock();
    rename_locked(entry, entry.uid, &flags, false)
}

pub fn expunge_deleted(root: &Path) -> Result<Vec<u32>, Error> {
    let entries = indexed(root)?;
    let mut gone = Vec::new();
    for e in entries {
        if e.deleted() {
            std::fs::remove_file(&e.path)?;
            gone.push(e.uid);
        }
    }
    Ok(gone)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagMode {
    Add,
    Remove,
    Replace,
}

fn uid_lock() -> std::sync::MutexGuard<'static, ()> {
    UID_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn uidvalidity_path(root: &Path) -> PathBuf {
    root.join(".uidvalidity")
}

fn uidnext_path(root: &Path) -> PathBuf {
    root.join(".uidnext")
}

fn ensure_uidvalidity_locked(root: &Path) -> Result<u32, Error> {
    if let Some(n) = read_num(&uidvalidity_path(root)).filter(|n| *n > 0) {
        return Ok(n);
    }
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(1)
        .max(1);
    write_num(&uidvalidity_path(root), n)?;
    if read_num(&uidnext_path(root)).is_none() {
        write_num(&uidnext_path(root), 1)?;
    }
    Ok(n)
}

fn take_uid_locked(root: &Path) -> Result<u32, Error> {
    let p = uidnext_path(root);
    let n = read_num(&p).unwrap_or(1).max(1);
    write_num(&p, n.saturating_add(1))?;
    Ok(n)
}

fn bump_uidnext_locked(root: &Path, entries: &[Entry]) -> Result<(), Error> {
    let max = entries.iter().map(|e| e.uid).max().unwrap_or(0);
    let cur = read_num(&uidnext_path(root)).unwrap_or(1);
    if cur <= max {
        write_num(&uidnext_path(root), max.saturating_add(1))?;
    }
    Ok(())
}

fn collect_locked(root: &Path) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    for (sub, recent) in [("new", true), ("cur", false)] {
        let dir = root.join(sub);
        if !dir.is_dir() {
            continue;
        }
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let p = e.path();
            if !p.is_file() || !is_mail_file(&p) {
                continue;
            }
            let name = match p.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            let (uid, flags) = parse_name(name);
            entries.push(Entry {
                path: p,
                uid,
                flags,
                recent,
            });
        }
    }
    entries.sort_by_key(|e| e.uid);
    Ok(entries)
}

fn parse_name(name: &str) -> (u32, String) {
    let uniq = name.split(":2,").next().unwrap_or(name);
    let flags = name
        .split_once(":2,")
        .map(|(_, f)| f.chars().filter(|c| c.is_ascii_alphabetic()).collect())
        .unwrap_or_default();
    let uid = uniq
        .rsplit_once(",U=")
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0);
    (uid, flags)
}

fn base_unique(name: &str) -> String {
    let uniq = name.split(":2,").next().unwrap_or(name);
    match uniq.rsplit_once(",U=") {
        Some((b, _)) => b.to_string(),
        None => uniq.to_string(),
    }
}

fn rename_locked(entry: &Entry, uid: u32, flags: &str, recent: bool) -> Result<Entry, Error> {
    let name = entry
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::Io(std::io::Error::other("maildir name")))?;
    let base = base_unique(name);
    let dest_name = if recent {
        format!("{base},U={uid}")
    } else {
        format!("{base},U={uid}:2,{flags}")
    };
    let parent = entry
        .path
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| Error::Io(std::io::Error::other("maildir parent")))?;
    let dest_dir = if recent {
        parent.join("new")
    } else {
        parent.join("cur")
    };
    fsutil::mkdir_private(&dest_dir)?;
    let dest = dest_dir.join(dest_name);
    if dest != entry.path {
        std::fs::rename(&entry.path, &dest)?;
        chmod_600(&dest)?;
    }
    Ok(Entry {
        path: dest,
        uid,
        flags: flags.to_string(),
        recent,
    })
}

fn maildir_flag(imap: &str) -> Option<char> {
    match imap
        .trim()
        .trim_start_matches('\\')
        .to_ascii_lowercase()
        .as_str()
    {
        "seen" => Some('S'),
        "flagged" => Some('F'),
        "deleted" => Some('T'),
        "answered" => Some('R'),
        "draft" => Some('D'),
        _ => None,
    }
}

fn is_mail_file(p: &Path) -> bool {
    match p.file_name().and_then(|s| s.to_str()) {
        Some(n) if n.starts_with('.') => false,
        Some(_) => true,
        None => false,
    }
}

fn read_num(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn write_num(path: &Path, n: u32) -> Result<(), Error> {
    fsutil::write_private(path, format!("{n}\n").as_bytes())?;
    Ok(())
}

fn chmod_600(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
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
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"Subject: hi\r\n\r\nbody\r\n"
        );
        let listed = list_new_and_cur(&root).unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn uid_assignment_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("mail");
        deliver(&root, b"a\r\n").unwrap();
        deliver(&root, b"b\r\n").unwrap();
        let first = indexed(&root).unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].uid, 1);
        assert_eq!(first[1].uid, 2);
        assert!(first.iter().all(|e| e.recent));
        let uv = uidvalidity(&root).unwrap();
        assert!(uv > 0);
        assert_eq!(uidnext(&root).unwrap(), 3);

        let again = indexed(&root).unwrap();
        assert_eq!(again[0].uid, 1);
        assert_eq!(again[1].uid, 2);
        assert_eq!(again[0].path, first[0].path);
        assert_eq!(uidvalidity(&root).unwrap(), uv);

        deliver(&root, b"c\r\n").unwrap();
        let three = indexed(&root).unwrap();
        assert_eq!(
            three.iter().map(|e| e.uid).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(uidnext(&root).unwrap(), 4);
        assert_eq!(uidvalidity(&root).unwrap(), uv);
    }

    #[test]
    fn assigns_uid_to_legacy_filename() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("mail");
        ensure(&root).unwrap();
        let legacy = root.join("new").join("1234.1.host");
        std::fs::write(&legacy, b"old\r\n").unwrap();
        let idx = indexed(&root).unwrap();
        assert_eq!(idx.len(), 1);
        assert_eq!(idx[0].uid, 1);
        assert!(idx[0]
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains(",U=1"));
        let again = indexed(&root).unwrap();
        assert_eq!(again[0].uid, 1);
        assert_eq!(again[0].path, idx[0].path);
    }

    #[test]
    fn fetch_moves_new_to_cur_and_store_sets_seen() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("mail");
        deliver(&root, b"x\r\n").unwrap();
        let e = indexed(&root).unwrap().into_iter().next().unwrap();
        assert!(e.recent);
        let e = to_cur(&e).unwrap();
        assert!(!e.recent);
        assert!(e.path.starts_with(root.join("cur")));
        assert!(!e.seen());
        let e = store_flags(&e, FlagMode::Add, &["\\Seen"]).unwrap();
        assert!(e.seen());
        assert!(e.path.file_name().unwrap().to_str().unwrap().contains("S"));
        let e = store_flags(&e, FlagMode::Add, &["\\Deleted"]).unwrap();
        assert!(e.deleted());
        let gone = expunge_deleted(&root).unwrap();
        assert_eq!(gone, vec![1]);
        assert!(indexed(&root).unwrap().is_empty());
    }
}
