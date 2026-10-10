//! Hire dashboard data: request subjects, deadlines, tiny local ledger.
//! The mailbox stays the source of truth; this file only parses it and
//! remembers per-request status on this machine. Nothing syncs anywhere.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use chrono::Datelike;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub uid: String,
    pub service: String,
    pub name: String,
    pub date: String,
    pub deadline: String,
    pub mailbox: String,
}

/// `hire: <service> | <name> | <date>` — anything else is not a request.
pub fn parse_subject(subject: &str) -> Option<(String, String, String)> {
    let body = subject.strip_prefix("hire:")?.trim();
    // Replies start with `re:` — never requests.
    if body.to_ascii_lowercase().starts_with("re:") {
        return None;
    }
    let mut parts = body.split('|').map(|p| p.trim().to_string());
    let (service, name, date) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || service.is_empty() || name.is_empty() {
        return None;
    }
    Some((service, name, date))
}

/// `deadline: <value>` line of a request body, or `"-"`.
pub fn deadline_of(body: &str) -> String {
    for line in body.lines() {
        if let Some(v) = line.strip_prefix("deadline:") {
            let v = v.trim();
            if !v.is_empty() && v != "-" {
                return v.to_string();
            }
        }
    }
    "-".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub note: String,
}

fn default_status() -> String {
    "new".to_string()
}

pub fn rank(status: &str) -> u8 {
    match status {
        "doing" => 0,
        "new" => 1,
        _ => 2,
    }
}

#[derive(Debug, Default)]
pub struct Ledger {
    pub path: PathBuf,
    pub entries: HashMap<String, Entry>,
}

impl Ledger {
    pub fn load(path: PathBuf) -> Ledger {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ledger { path, entries }
    }

    pub fn status(&self, uid: &str) -> &str {
        self.entries.get(uid).map(|e| e.status.as_str()).unwrap_or("new")
    }

    /// new → doing → done → new. Persists. Returns the new status.
    pub fn cycle(&mut self, uid: &str) -> &str {
        let next = match self.status(uid) {
            "new" => "doing",
            "doing" => "done",
            _ => "new",
        }
        .to_string();
        self.entries
            .entry(uid.to_string())
            .or_insert_with(Entry::default)
            .status = next;
        self.save();
        self.status(uid)
    }

    fn save(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(&self.entries) {
            let _ = std::fs::write(&self.path, s);
        }
    }
}

pub fn ledger_path() -> PathBuf {
    crate::paths::cache_dir().join("hire.json")
}

/// Short relative age for list rows: `now 4m 3h 6d`, else `Oct 8` / `Oct 8 25`.
/// Unparseable dates fall back to the old truncated-header look.
pub fn age_label(now_ts: i64, date_rfc3339: &str) -> String {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date_rfc3339) else {
        return date_rfc3339.chars().take(16).collect();
    };
    let diff = now_ts - dt.timestamp();
    if diff < 0 {
        return "soon".to_string();
    }
    if diff < 60 {
        return "now".to_string();
    }
    if diff < 3600 {
        return format!("{}m", diff / 60);
    }
    if diff < 86400 {
        return format!("{}h", diff / 3600);
    }
    if diff < 30 * 86400 {
        return format!("{}d", diff / 86400);
    }
    let local = dt.with_timezone(&chrono::Local);
    let now = chrono::DateTime::from_timestamp(now_ts, 0)
        .map(|u| u.with_timezone(&chrono::Local));
    match now {
        Some(n) if n.format("%Y").to_string() == local.format("%Y").to_string() => {
            format!("{} {}", local.format("%b"), local.day())
        }
        _ => format!("{} {} {}", local.format("%b"), local.day(), local.format("%y")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_parses() {
        assert_eq!(
            parse_subject("hire: teaser | Ada | 2026-10-10"),
            Some(("teaser".into(), "Ada".into(), "2026-10-10".into()))
        );
        assert_eq!(parse_subject("hire: x | y"), None);
        assert_eq!(parse_subject("re: hire: teaser | A | B"), None);
        assert_eq!(parse_subject("hire:  | A | B"), None);
        assert_eq!(parse_subject("hello world"), None);
    }

    #[test]
    fn deadline_line() {
        assert_eq!(deadline_of("service: x\ndeadline: 2026-11-01\n"), "2026-11-01");
        assert_eq!(deadline_of("deadline: -\n"), "-");
        assert_eq!(deadline_of("nope"), "-");
    }

    #[test]
    fn ages() {
        let now = 1_791_661_200; // 2026-10-10T19:40:00Z
        assert_eq!(age_label(now, "2026-10-10T19:40:00+00:00"), "now");
        assert_eq!(age_label(now, "2026-10-10T19:35:00+00:00"), "5m");
        assert_eq!(age_label(now, "2026-10-10T17:40:00+00:00"), "2h");
        assert_eq!(age_label(now, "2026-10-07T19:40:00+00:00"), "3d");
        assert_eq!(age_label(now, "2026-08-19T07:15:00+00:00"), "Aug 19");
        assert_eq!(age_label(now, "2024-01-05T00:00:00+00:00"), "Jan 5 24");
        assert_eq!(age_label(now, "garbage"), "garbage");
    }

    #[test]
    fn ledger_cycles_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hire.json");
        let mut l = Ledger::load(path.clone());
        assert_eq!(l.status("u1"), "new");
        assert_eq!(l.cycle("u1"), "doing");
        assert_eq!(l.cycle("u1"), "done");
        assert_eq!(l.cycle("u1"), "new");
        let l2 = Ledger::load(path);
        assert_eq!(l2.status("u1"), "new");
        assert_eq!(rank("doing"), 0);
        assert_eq!(rank("new"), 1);
        assert_eq!(rank("done"), 2);
    }
}
