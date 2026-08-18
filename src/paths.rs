//! Platform paths. Linux stays ~/.config/goblin and ~/.cache/goblin.

use std::path::PathBuf;

fn project() -> directories::ProjectDirs {
    directories::ProjectDirs::from("", "faeos", "goblin")
        .expect("cannot determine home directory")
}

pub fn config_dir() -> PathBuf {
    project().config_dir().to_path_buf()
}

pub fn cache_dir() -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_paths_use_goblin_leaf() {
        let cfg = config_dir();
        let cache = cache_dir();
        assert!(cfg.ends_with("goblin"), "{cfg:?}");
        assert!(cache.ends_with("goblin"), "{cache:?}");
        assert!(mail_root().ends_with("mail"));
    }
}
