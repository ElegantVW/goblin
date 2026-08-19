//! Platform paths. Linux stays ~/.config/goblin and ~/.cache/goblin.
//! `GOBLIN_HOME` redirects those to `<home>/config` and `<home>/cache`.

use std::path::PathBuf;

fn project() -> directories::ProjectDirs {
    directories::ProjectDirs::from("", "faeos", "goblin").expect("cannot determine home directory")
}

fn home_root() -> Option<PathBuf> {
    std::env::var_os("GOBLIN_HOME")
        .filter(|v| !v.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

pub fn config_dir() -> PathBuf {
    if let Some(root) = home_root() {
        return root.join("config");
    }
    project().config_dir().to_path_buf()
}

pub fn cache_dir() -> PathBuf {
    if let Some(root) = home_root() {
        return root.join("cache");
    }
    project().cache_dir().to_path_buf()
}

pub fn accounts_file() -> PathBuf {
    let gpg = config_dir().join("accounts.json.gpg");
    if gpg.is_file() {
        gpg
    } else {
        config_dir().join("accounts.json")
    }
}

pub fn secrets_file() -> PathBuf {
    config_dir().join("secrets")
}

pub fn state_file() -> PathBuf {
    cache_dir().join("state.json")
}

pub fn mail_root() -> PathBuf {
    cache_dir().join("mail")
}

pub fn notify_dir() -> PathBuf {
    config_dir()
}

static GOBLIN_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serialize `GOBLIN_HOME` mutation and restore it even if `f` panics.
/// Always public: `tests/*.rs` compile the lib without `cfg(test)`.
pub fn with_goblin_home<R>(home: Option<&std::path::Path>, f: impl FnOnce() -> R) -> R {
    let guard = GOBLIN_HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev = std::env::var_os("GOBLIN_HOME");
    // SAFETY: serialized by GOBLIN_HOME_LOCK; restored before return or unwind.
    unsafe {
        match home {
            Some(p) => std::env::set_var("GOBLIN_HOME", p),
            None => std::env::remove_var("GOBLIN_HOME"),
        }
    }
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    unsafe {
        match prev {
            Some(v) => std::env::set_var("GOBLIN_HOME", v),
            None => std::env::remove_var("GOBLIN_HOME"),
        }
    }
    drop(guard);
    match out {
        Ok(v) => v,
        Err(p) => std::panic::resume_unwind(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_paths_use_goblin_leaf() {
        with_goblin_home(None, || {
            let cfg = config_dir();
            let cache = cache_dir();
            assert!(cfg.ends_with("goblin"), "{cfg:?}");
            assert!(cache.ends_with("goblin"), "{cache:?}");
            assert!(mail_root().ends_with("mail"));
        });
    }

    #[test]
    fn empty_or_whitespace_goblin_home_is_unset() {
        let unset = with_goblin_home(None, || (config_dir(), cache_dir()));
        for raw in ["", "   ", "\t", "\n"] {
            with_goblin_home(Some(std::path::Path::new(raw)), || {
                assert_eq!(config_dir(), unset.0, "GOBLIN_HOME={raw:?}");
                assert_eq!(cache_dir(), unset.1, "GOBLIN_HOME={raw:?}");
            });
        }
    }

    #[test]
    fn goblin_home_redirects_config_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        with_goblin_home(Some(dir.path()), || {
            let cfg = config_dir();
            let cache = cache_dir();
            assert_eq!(cfg, dir.path().join("config"));
            assert_eq!(cache, dir.path().join("cache"));
            assert_eq!(mail_root(), dir.path().join("cache").join("mail"));
            assert_eq!(secrets_file(), dir.path().join("config").join("secrets"));
        });
    }
}
