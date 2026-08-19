//! Disk queue for outbound mail. Retries only — no DSN / bounce generator.

use super::paths;
use crate::error::Error;
use crate::fsutil;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_ATTEMPTS: u32 = 8;
const BACKOFF_BASE_SECS: u64 = 60;
const BACKOFF_CAP_SECS: u64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    #[serde(default)]
    pub id: String,
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub next_attempt: u64,
    #[serde(default)]
    pub last_error: Option<String>,
}

pub fn enqueue(from: &str, to: &[String], user: Option<&str>, raw: &[u8]) -> Result<String, Error> {
    if to.is_empty() {
        return Err(Error::Smtp("no recipients".into()));
    }
    paths::ensure_layout()?;
    let id = unique_queue_id();
    let now = unix_now();
    let job = Job {
        id: id.clone(),
        from: from.to_string(),
        to: to.to_vec(),
        user: user.map(str::to_string),
        created: now,
        attempts: 0,
        next_attempt: now,
        last_error: None,
    };
    save_job(&job)?;
    fsutil::write_private(&eml_path(&id), raw)?;
    Ok(id)
}

pub fn list_due_jobs() -> Result<Vec<Job>, Error> {
    paths::ensure_layout()?;
    let now = unix_now();
    let mut jobs = Vec::new();
    let dir = paths::queue_dir();
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(jobs),
        Err(e) => return Err(e.into()),
    };
    for ent in rd {
        let ent = ent?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.ends_with(".json") {
            continue;
        }
        match load_job_file(&path) {
            Ok(job) if job.next_attempt <= now && !job.to.is_empty() => jobs.push(job),
            Ok(job) if job.to.is_empty() => {
                let _ = complete_job(&job.id);
            }
            Ok(_) => {}
            Err(e) => eprintln!("goblind: queue skip {}: {e}", path.display()),
        }
    }
    jobs.sort_by(|a, b| {
        a.next_attempt
            .cmp(&b.next_attempt)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(jobs)
}

pub fn save_job(job: &Job) -> Result<(), Error> {
    paths::ensure_layout()?;
    let json = serde_json::to_string_pretty(job)?;
    fsutil::write_private(&json_path(&job.id), format!("{json}\n").as_bytes())?;
    Ok(())
}

pub fn complete_job(id: &str) -> Result<(), Error> {
    let json = json_path(id);
    let eml = eml_path(id);
    if json.is_file() {
        fs::remove_file(&json)?;
    }
    if eml.is_file() {
        fs::remove_file(&eml)?;
    }
    Ok(())
}

pub fn move_to_failed(job: &Job) -> Result<(), Error> {
    paths::ensure_layout()?;
    let dest_dir = paths::queue_failed_dir();
    fsutil::mkdir_private(&dest_dir)?;
    let src_json = json_path(&job.id);
    let src_eml = eml_path(&job.id);
    let dest_json = dest_dir.join(format!("{}.json", job.id));
    let dest_eml = dest_dir.join(format!("{}.eml", job.id));
    let json = serde_json::to_string_pretty(job)?;
    fsutil::write_private(&dest_json, format!("{json}\n").as_bytes())?;
    if src_eml.is_file() {
        let raw = fs::read(&src_eml)?;
        fsutil::write_private(&dest_eml, &raw)?;
        fs::remove_file(&src_eml)?;
    }
    if src_json.is_file() {
        fs::remove_file(&src_json)?;
    }
    append_failed_log(job)?;
    Ok(())
}

pub fn read_eml(id: &str) -> Result<Vec<u8>, Error> {
    Ok(fs::read(eml_path(id))?)
}

pub fn write_eml(id: &str, raw: &[u8]) -> Result<(), Error> {
    fsutil::write_private(&eml_path(id), raw)?;
    Ok(())
}

pub fn backoff_secs(attempts: u32) -> u64 {
    let n = attempts.saturating_sub(1).min(16);
    BACKOFF_BASE_SECS
        .saturating_mul(1u64.checked_shl(n).unwrap_or(u64::MAX))
        .min(BACKOFF_CAP_SECS)
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn json_path(id: &str) -> PathBuf {
    paths::queue_dir().join(format!("{id}.json"))
}

pub fn eml_path(id: &str) -> PathBuf {
    paths::queue_dir().join(format!("{id}.eml"))
}

fn load_job_file(path: &std::path::Path) -> Result<Job, Error> {
    let text = fs::read_to_string(path)?;
    let mut job: Job = serde_json::from_str(&text)?;
    if job.id.is_empty() {
        job.id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
    }
    Ok(job)
}

fn unique_queue_id() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut id = format!("{t}.{}", std::process::id());
    let mut n = 0u32;
    while json_path(&id).exists() || eml_path(&id).exists() {
        n += 1;
        id = format!("{t}.{}.{}", std::process::id(), n);
    }
    id
}

fn append_failed_log(job: &Job) -> Result<(), Error> {
    let path = paths::queue_dir().join("failed.log");
    let line = format!(
        "{} id={} from={} to={} attempts={} error={}",
        unix_now(),
        job.id,
        job.from,
        job.to.join(","),
        job.attempts,
        job.last_error.as_deref().unwrap_or("-")
    );
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::paths::with_goblind_home;

    #[test]
    fn enqueue_lists_and_completes() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            let raw = b"Subject: x\r\n\r\nbody\r\n";
            let id = enqueue("a@ex.com", &["b@other.com".into()], Some("a@ex.com"), raw).unwrap();
            let due = list_due_jobs().unwrap();
            assert_eq!(due.len(), 1);
            assert_eq!(due[0].id, id);
            assert_eq!(due[0].from, "a@ex.com");
            assert_eq!(due[0].to, vec!["b@other.com"]);
            assert_eq!(due[0].user.as_deref(), Some("a@ex.com"));
            assert_eq!(due[0].attempts, 0);
            assert!(due[0].created > 0);
            assert!(due[0].last_error.is_none());
            assert!(!serde_json::to_string(&due[0]).unwrap().contains("password"));
            assert_eq!(read_eml(&id).unwrap(), raw);
            complete_job(&id).unwrap();
            assert!(list_due_jobs().unwrap().is_empty());
            assert!(!json_path(&id).exists());
            assert!(!eml_path(&id).exists());
        });
    }

    #[test]
    fn future_jobs_are_not_due() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            let id = enqueue("a@ex.com", &["b@other.com".into()], None, b"x\r\n").unwrap();
            let mut job = list_due_jobs().unwrap().pop().unwrap();
            job.next_attempt = unix_now().saturating_add(3600);
            save_job(&job).unwrap();
            assert!(list_due_jobs().unwrap().is_empty());
            complete_job(&id).unwrap();
        });
    }

    #[test]
    fn save_job_updates_attempts() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            let id = enqueue("a@ex.com", &["b@other.com".into()], None, b"x\r\n").unwrap();
            let mut job = list_due_jobs().unwrap().pop().unwrap();
            job.attempts = 2;
            job.last_error = Some("450 try later".into());
            job.next_attempt = 0;
            save_job(&job).unwrap();
            let loaded = list_due_jobs().unwrap().pop().unwrap();
            assert_eq!(loaded.id, id);
            assert_eq!(loaded.attempts, 2);
            assert_eq!(loaded.last_error.as_deref(), Some("450 try later"));
        });
    }

    #[test]
    fn move_to_failed_keeps_copy_and_log() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            let id = enqueue("a@ex.com", &["b@other.com".into()], None, b"raw\r\n").unwrap();
            let mut job = list_due_jobs().unwrap().pop().unwrap();
            job.attempts = 8;
            job.last_error = Some("550 nope".into());
            move_to_failed(&job).unwrap();
            assert!(list_due_jobs().unwrap().is_empty());
            assert!(!json_path(&id).exists());
            let failed_json = paths::queue_failed_dir().join(format!("{id}.json"));
            let failed_eml = paths::queue_failed_dir().join(format!("{id}.eml"));
            assert!(failed_json.is_file());
            assert!(failed_eml.is_file());
            assert_eq!(fs::read(&failed_eml).unwrap(), b"raw\r\n");
            let log = fs::read_to_string(paths::queue_dir().join("failed.log")).unwrap();
            assert!(log.contains(&id), "{log}");
            assert!(log.contains("550 nope"), "{log}");
            assert!(dir.path().join("queue").join("failed").is_dir());
        });
    }

    #[test]
    fn backoff_starts_at_60_and_caps() {
        assert_eq!(backoff_secs(1), 60);
        assert_eq!(backoff_secs(2), 120);
        assert_eq!(backoff_secs(3), 240);
        assert_eq!(backoff_secs(7), 3600);
        assert_eq!(backoff_secs(8), 3600);
    }
}
