//! Best-effort notify sound. Missing player is not an error.

use crate::error::Error;
use crate::paths;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXTS: &[&str] = &[".mp3", ".wav", ".ogg", ".m4a", ".flac"];

pub fn find_sound() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("GOBLIN_SOUND") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Some(p);
        }
    }
    let dir = paths::notify_dir();
    for ext in EXTS {
        let p = dir.join(format!("notify{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn play() -> bool {
    let Some(path) = find_sound() else {
        return false;
    };
    play_path(&path)
}

pub fn play_path(path: &Path) -> bool {
    let players: &[&[&str]] = if cfg!(target_os = "macos") {
        &[&["afplay"]]
    } else {
        &[
            &["mpv", "--no-video", "--no-config", "--volume=85", "--really-quiet"],
            &["pw-play"],
            &["paplay"],
            &["aplay", "-q"],
        ]
    };
    for p in players {
        let mut cmd = Command::new(p[0]);
        for a in &p[1..] {
            cmd.arg(a);
        }
        cmd.arg(path);
        if cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

pub fn install(src: &Path) -> Result<PathBuf, Error> {
    if !src.is_file() {
        return Err(Error::Usage(format!("no such sound: {}", src.display())));
    }
    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{s}"))
        .filter(|e| EXTS.contains(&e.as_str()))
        .unwrap_or_else(|| ".mp3".into());
    let dir = paths::notify_dir();
    std::fs::create_dir_all(&dir)?;
    let dst = dir.join(format!("notify{ext}"));
    for e in EXTS {
        let old = dir.join(format!("notify{e}"));
        if old != dst && old.exists() {
            let _ = std::fs::remove_file(old);
        }
    }
    std::fs::copy(src, &dst)?;
    Ok(dst)
}

pub fn cmd_sound(set: Option<&Path>) -> Result<u8, Error> {
    if let Some(src) = set {
        let dst = install(src)?;
        println!("goblin voice installed → {}", dst.display());
        play_path(&dst);
        return Ok(0);
    }
    match find_sound() {
        None => {
            eprintln!("goblin has no voice yet — drop one at ~/.config/goblin/notify.mp3");
            eprintln!("or install a clip:  goblin sound --set /path/to/goblin.mp3");
            Ok(2)
        }
        Some(p) => {
            if play_path(&p) {
                Ok(0)
            } else {
                eprintln!("could not play {}", p.display());
                Ok(1)
            }
        }
    }
}
