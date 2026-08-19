//! House chrome — port of faeos `fae_termart` box / tui_* (no Python at runtime).

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, terminal};
use std::io::{self, IsTerminal, Write};
use std::sync::Mutex;
use std::time::Duration;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const ITALIC: &str = "\x1b[3m";
pub const DIM: &str = "\x1b[2m";
pub const PINK: &str = "\x1b[38;5;175m";
pub const DARK: &str = "\x1b[38;5;168m";
pub const PINK_DIM: &str = "\x1b[38;5;132m";
pub const BLUSH: &str = "\x1b[38;5;218m";
pub const SILVER: &str = "\x1b[38;5;252m";
pub const MUTED: &str = "\x1b[38;5;245m";
pub const OK: &str = "\x1b[38;5;78m";
pub const WARN: &str = "\x1b[38;5;214m";
pub const ERR: &str = "\x1b[38;5;197m";

const TUI_HYGIENE: &str =
    "\x1b[0m\x1b[?25h\x1b[?7h\x1b[?2004l\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

static FORCE_COLOR: Mutex<bool> = Mutex::new(false);
static FORCE_UNICODE: Mutex<Option<bool>> = Mutex::new(None);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

struct Session {
    alt: bool,
}

pub fn set_force_color(on: bool) {
    *FORCE_COLOR.lock().unwrap() = on;
}

pub fn set_force_unicode(on: bool) {
    *FORCE_UNICODE.lock().unwrap() = Some(on);
}

pub fn stdout_is_tty() -> bool {
    io::stdout().is_terminal() || io::stderr().is_terminal()
}

pub fn color_ok() -> bool {
    if *FORCE_COLOR.lock().unwrap() {
        return true;
    }
    if std::env::var("NO_COLOR")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return false;
    }
    for k in ["FORCE_COLOR", "CLICOLOR_FORCE"] {
        if std::env::var(k)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
        {
            return true;
        }
    }
    stdout_is_tty()
}

pub fn unicode_ok() -> bool {
    if let Some(on) = *FORCE_UNICODE.lock().unwrap() {
        return on;
    }
    match std::env::var("PIXIE_UNICODE") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "on" | "true" | "yes" | "unicode"
        ),
        Err(_) => false,
    }
}

pub fn paint(text: &str, codes: &[&str]) -> String {
    if !color_ok() || codes.is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    for c in codes {
        out.push_str(c);
    }
    out.push_str(text);
    out.push_str(RESET);
    out
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() || n == 'm' || n == 'K' {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

pub fn char_width(ch: char) -> usize {
    let o = ch as u32;
    if o < 32 || (0x7F..0xA0).contains(&o) {
        return 0;
    }
    if unicodedata_combining(ch) {
        return 0;
    }
    if east_asian_wide(ch) {
        return 2;
    }
    if (0x1F300..=0x1FAFF).contains(&o) {
        return 2;
    }
    1
}

fn unicodedata_combining(ch: char) -> bool {
    matches!(unicode_general_category(ch), 'M')
}

fn unicode_general_category(ch: char) -> char {
    let o = ch as u32;
    // Combining marks (Mn/Mc/Me) — enough for vis_len.
    if (0x0300..=0x036F).contains(&o)
        || (0x1AB0..=0x1AFF).contains(&o)
        || (0x1DC0..=0x1DFF).contains(&o)
        || (0x20D0..=0x20FF).contains(&o)
        || (0xFE20..=0xFE2F).contains(&o)
    {
        return 'M';
    }
    'L'
}

fn east_asian_wide(ch: char) -> bool {
    let o = ch as u32;
    // Fullwidth / wide blocks commonly seen in terminals.
    (0x1100..=0x115F).contains(&o)
        || (0x2329..=0x232A).contains(&o)
        || (0x2E80..=0xA4CF).contains(&o)
        || (0xAC00..=0xD7A3).contains(&o)
        || (0xF900..=0xFAFF).contains(&o)
        || (0xFE10..=0xFE19).contains(&o)
        || (0xFE30..=0xFE6F).contains(&o)
        || (0xFF00..=0xFF60).contains(&o)
        || (0xFFE0..=0xFFE6).contains(&o)
        || (0x20000..=0x2FFFD).contains(&o)
        || (0x30000..=0x3FFFD).contains(&o)
}

pub fn vis_len(s: &str) -> usize {
    strip_ansi(s).chars().map(char_width).sum()
}

pub fn ascii_box_enabled() -> bool {
    !unicode_ok()
}

pub fn pad_vis(s: &str, width: usize) -> String {
    let n = vis_len(s);
    if n == width {
        return s.to_string();
    }
    if n < width {
        return format!("{s}{}", " ".repeat(width - n));
    }
    let plain = strip_ansi(s);
    let mut out = String::new();
    let mut w = 0;
    let limit = width.saturating_sub(3).max(1);
    for ch in plain.chars() {
        let cw = char_width(ch);
        if w + cw > limit {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push_str("...");
    let n = vis_len(&out);
    if n < width {
        out.push_str(&" ".repeat(width - n));
    }
    out
}

pub fn wrap_plain(text: &str, mut width: usize) -> Vec<String> {
    if width < 8 {
        width = 8;
    }
    let mut lines = Vec::new();
    let raws: Vec<&str> = if text.is_empty() {
        vec![""]
    } else {
        text.split('\n').collect()
    };
    for raw in raws {
        if raw.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let indent: String = raw.chars().take_while(|c| c.is_whitespace()).collect();
        let content = &raw[indent.len()..];
        let max_c = width.saturating_sub(indent.len()).max(8);
        if vis_len(raw) <= width {
            lines.push(raw.to_string());
            continue;
        }
        let words: Vec<&str> = content.split(' ').collect();
        let mut cur = String::new();
        for word in words {
            let trial = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if vis_len(&trial) <= max_c {
                cur = trial;
            } else {
                if !cur.is_empty() {
                    lines.push(format!("{indent}{cur}"));
                }
                let mut word = word.to_string();
                while vis_len(&word) > max_c {
                    let mut chunk = String::new();
                    let mut w = 0;
                    let mut take = 0;
                    for ch in word.chars() {
                        let cw = char_width(ch);
                        if w + cw > max_c {
                            break;
                        }
                        chunk.push(ch);
                        w += cw;
                        take += ch.len_utf8();
                    }
                    lines.push(format!("{indent}{chunk}"));
                    word = word[take..].to_string();
                }
                cur = word;
            }
        }
        if !cur.is_empty() {
            lines.push(format!("{indent}{cur}"));
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// `art.box` — house frame. `width` is outer columns.
pub fn box_frame(body: &[String], title: &str, subtitle: &str, width: usize) -> String {
    let outer = width.max(8);
    let inner = outer.saturating_sub(2);
    let body_w = inner.saturating_sub(2).max(1);
    let ascii = ascii_box_enabled();
    let (tl, tr, bl, br, hz, vt, mark, soft) = if ascii {
        ("+", "+", "+", "+", "-", "|", "*", "-")
    } else {
        ("╭", "╮", "╰", "╯", "─", "│", "✦", "·")
    };
    let acc = PINK_DIM;

    let mut out = Vec::new();
    if !title.is_empty() {
        let tplain = strip_ansi(title);
        let mut label = format!(" {mark} {tplain} {mark} ");
        if vis_len(&label) > inner.saturating_sub(2) {
            let keep = inner.saturating_sub(8).max(1);
            let cut: String = tplain.chars().take(keep).collect();
            let ell = if ascii { "..." } else { "…" };
            label = format!(" {mark} {cut}{ell} {mark} ");
        }
        let fill = inner.saturating_sub(vis_len(&label)).saturating_sub(1);
        let top = format!(
            "{}{}{}",
            paint(&format!("{tl}{hz}"), &[acc]),
            paint(&label, &[BOLD, PINK]),
            paint(&format!("{}{tr}", hz.repeat(fill)), &[acc])
        );
        out.push(top);
    } else {
        out.push(paint(&format!("{tl}{}{tr}", hz.repeat(inner)), &[acc]));
    }

    if !subtitle.is_empty() {
        for ln in wrap_plain(&strip_ansi(subtitle), body_w) {
            out.push(format!(
                "{} {} {}{}",
                paint(vt, &[acc]),
                paint(&pad_vis(&ln, body_w), &[ITALIC, DARK]),
                paint(vt, &[acc]),
                ""
            ));
        }
        out.push(format!(
            "{}{}{}",
            paint(vt, &[acc]),
            paint(&format!(" {} ", soft.repeat(body_w)), &[acc]),
            paint(vt, &[acc])
        ));
    }

    let mut lines_in: Vec<String> = Vec::new();
    if body.is_empty() {
        lines_in.push(String::new());
    } else {
        for para in body {
            if strip_ansi(para) != *para {
                lines_in.push(para.clone());
            } else {
                lines_in.extend(wrap_plain(para, body_w));
            }
        }
    }

    for ln in lines_in {
        if strip_ansi(&ln) != ln {
            let pad = body_w.saturating_sub(vis_len(&ln));
            out.push(format!(
                "{} {}{} {}{}",
                paint(vt, &[acc]),
                ln,
                " ".repeat(pad),
                paint(vt, &[acc]),
                ""
            ));
            continue;
        }
        let plain = pad_vis(&strip_ansi(&ln), body_w);
        let painted = if plain.trim().is_empty() {
            " ".repeat(body_w)
        } else {
            paint(&plain, &[SILVER])
        };
        out.push(format!(
            "{} {} {}{}",
            paint(vt, &[acc]),
            painted,
            paint(vt, &[acc]),
            ""
        ));
    }

    out.push(paint(&format!("{bl}{}{br}", hz.repeat(inner)), &[acc]));
    out.join("\n")
}

pub fn line_count(frame: &str) -> usize {
    if frame.is_empty() {
        0
    } else {
        frame.matches('\n').count() + 1
    }
}

pub fn winsize() -> (u16, u16) {
    terminal::size().unwrap_or((80, 24))
}

pub fn term_width() -> usize {
    let (cols, _) = winsize();
    let usable = (cols as usize).saturating_sub(1).max(1);
    usable.max(36)
}

pub fn term_height() -> usize {
    let (_, rows) = winsize();
    (rows as usize).max(12)
}

pub fn decode_csi(s: &str) -> String {
    if s.starts_with('A') {
        return "up".into();
    }
    if s.starts_with('B') {
        return "down".into();
    }
    if s.starts_with('C') {
        return "right".into();
    }
    if s.starts_with('D') {
        return "left".into();
    }
    if s.starts_with('Z') {
        return "shift-tab".into();
    }
    if s.starts_with("5~") {
        return "pgup".into();
    }
    if s.starts_with("6~") {
        return "pgdn".into();
    }
    if s.starts_with('H') || s.starts_with("1~") || s.starts_with("7~") {
        return "home".into();
    }
    if s.starts_with('F') || s.starts_with("4~") || s.starts_with("8~") {
        return "end".into();
    }
    if s.starts_with("3~") {
        return "delete".into();
    }
    format!("csi:{s}")
}

pub fn decode_byte(ch: u8) -> String {
    match ch {
        b'\r' | b'\n' => "enter".into(),
        b' ' => "space".into(),
        b'\t' => "tab".into(),
        0x03 => "ctrl-c".into(),
        0x04 => "ctrl-d".into(),
        0x7f | 0x08 => "backspace".into(),
        0x15 => "ctrl-u".into(),
        0x12 => "ctrl-r".into(),
        0x13 => "ctrl-s".into(),
        0x17 => "ctrl-w".into(),
        0x01 => "ctrl-a".into(),
        0x05 => "ctrl-e".into(),
        0x10 => "ctrl-p".into(),
        0x0c => "ctrl-l".into(),
        _ => String::from_utf8_lossy(&[ch]).into_owned(),
    }
}

#[cfg(unix)]
fn pixie_screen_hold(on: bool, name: &str) {
    let bin = directories::BaseDirs::new()
        .map(|b| b.home_dir().join("bin").join("pixie-screen"))
        .unwrap_or_else(|| std::path::PathBuf::from("pixie-screen"));
    if !bin.is_file() {
        return;
    }
    if on {
        let _ = std::process::Command::new(&bin)
            .args(["hold", name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    } else {
        let _ = std::process::Command::new(&bin)
            .args(["release", name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::process::Command::new(&bin)
            .args(["hold", "grace", "20"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(not(unix))]
fn pixie_screen_hold(_on: bool, _name: &str) {}

pub fn tui_available() -> bool {
    io::stdout().is_terminal()
}

pub fn paint_frame(body: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(b"\x1b[H\x1b[2J\x1b[?7l");
    let mut body = body.to_string();
    if !body.ends_with('\n') {
        let lines = body.matches('\n').count() + 1;
        if lines < term_height() {
            body.push('\n');
        }
    }
    body.push_str(RESET);
    body.push_str("\x1b[?7h");
    let _ = out.write_all(body.as_bytes());
    let _ = out.flush();
}

pub fn tui_begin(hold_name: &str) {
    set_force_color(true);
    pixie_screen_hold(true, hold_name);
    let _ = terminal::enable_raw_mode();
    let mut out = io::stdout();
    let _ = execute!(out, terminal::EnterAlternateScreen, cursor::Hide);
    *SESSION.lock().unwrap() = Some(Session { alt: true });
}

pub fn tui_cleanup() {
    let mut g = SESSION.lock().unwrap();
    let Some(sess) = g.take() else {
        return;
    };
    let mut out = io::stdout();
    if sess.alt {
        let _ = execute!(out, terminal::LeaveAlternateScreen, cursor::Show);
    }
    let _ = out.write_all(TUI_HYGIENE.as_bytes());
    let _ = out.flush();
    let _ = terminal::disable_raw_mode();
    pixie_screen_hold(false, "goblin");
    set_force_color(false);
}

fn map_key_event(ev: KeyEvent) -> String {
    if ev.kind == KeyEventKind::Release {
        return String::new();
    }
    match ev.code {
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Tab => {
            if ev.modifiers.contains(KeyModifiers::SHIFT) {
                "shift-tab".into()
            } else {
                "tab".into()
            }
        }
        KeyCode::BackTab => "shift-tab".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::PageUp => "pgup".into(),
        KeyCode::PageDown => "pgdn".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Char(c) if ev.modifiers.contains(KeyModifiers::CONTROL) => {
            match c.to_ascii_lowercase() {
                'c' => "ctrl-c".into(),
                's' => "ctrl-s".into(),
                'u' => "ctrl-u".into(),
                'a' => "ctrl-a".into(),
                'e' => "ctrl-e".into(),
                'r' => "ctrl-r".into(),
                'w' => "ctrl-w".into(),
                'p' => "ctrl-p".into(),
                'l' => "ctrl-l".into(),
                'd' => "ctrl-d".into(),
                other => format!("ctrl-{other}"),
            }
        }
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(c) => c.to_string(),
        _ => String::new(),
    }
}

pub fn tui_read_key(timeout_ms: Option<i32>) -> String {
    loop {
        if let Some(ms) = timeout_ms {
            if ms >= 0 {
                match event::poll(Duration::from_millis(ms as u64)) {
                    Ok(true) => {}
                    _ => return String::new(),
                }
            }
        }
        match event::read() {
            Ok(Event::Key(ev)) => {
                let key = map_key_event(ev);
                if key.is_empty() {
                    if timeout_ms.is_some() {
                        return String::new();
                    }
                    continue;
                }
                return key;
            }
            Ok(Event::Resize(_, _)) => return String::new(),
            Ok(_) => {
                if timeout_ms.is_some() {
                    return String::new();
                }
            }
            Err(_) => return "esc".into(),
        }
    }
}

pub fn tui_suspend() {
    if SESSION.lock().unwrap().is_none() {
        return;
    }
    let mut out = io::stdout();
    let _ = execute!(out, terminal::LeaveAlternateScreen, cursor::Show);
    let _ = out.write_all(TUI_HYGIENE.as_bytes());
    let _ = out.flush();
    let _ = terminal::disable_raw_mode();
}

pub fn tui_resume() {
    if SESSION.lock().unwrap().is_none() {
        return;
    }
    let _ = terminal::enable_raw_mode();
    let mut out = io::stdout();
    let _ = execute!(out, terminal::EnterAlternateScreen, cursor::Hide);
    set_force_color(true);
}

/// Read one secret line with echo off. Caller must already have a tty.
pub fn read_secret_line() -> io::Result<String> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "password prompt needs a tty",
        ));
    }
    terminal::enable_raw_mode()?;
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }
    let _guard = Guard;
    let mut line = String::new();
    loop {
        if let Event::Key(ev) = event::read()? {
            if ev.kind == KeyEventKind::Release {
                continue;
            }
            match ev.code {
                KeyCode::Enter => break,
                KeyCode::Esc => {
                    line.clear();
                    break;
                }
                KeyCode::Backspace => {
                    line.pop();
                }
                KeyCode::Char('c') if ev.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"));
                }
                KeyCode::Char('u') if ev.modifiers.contains(KeyModifiers::CONTROL) => {
                    line.clear();
                }
                KeyCode::Char('d') if ev.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char(c)
                    if !ev.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() =>
                {
                    line.push(c);
                }
                _ => {}
            }
        }
    }
    Ok(line)
}

/// Open $EDITOR on a temp file; restore TUI after.
pub fn edit_temp(body: &str) -> Result<String, String> {
    tui_suspend();
    let result = (|| {
        let path = std::env::temp_dir().join(format!("goblin-draft-{}.txt", std::process::id()));
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "nano".into()
            }
        });
        let status = std::process::Command::new(&editor)
            .arg(&path)
            .status()
            .map_err(|e| format!("{editor}: {e}"))?;
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        if !status.success() {
            return Err(format!("{editor} exited {status}"));
        }
        Ok(text)
    })();
    tui_resume();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV: Mutex<()> = Mutex::new(());

    fn with_unicode<T>(on: bool, f: impl FnOnce() -> T) -> T {
        let _g = ENV.lock().unwrap();
        let prev = *FORCE_UNICODE.lock().unwrap();
        set_force_unicode(on);
        set_force_color(true);
        let r = f();
        set_force_color(false);
        *FORCE_UNICODE.lock().unwrap() = prev;
        r
    }

    #[test]
    fn palette_codes() {
        assert_eq!(PINK, "\x1b[38;5;175m");
        assert_eq!(PINK_DIM, "\x1b[38;5;132m");
        assert_eq!(BLUSH, "\x1b[38;5;218m");
        assert_eq!(SILVER, "\x1b[38;5;252m");
        assert_eq!(MUTED, "\x1b[38;5;245m");
    }

    #[test]
    fn unicode_title_bar_shape() {
        with_unicode(true, || {
            let frame = box_frame(&["".into()], "Title", "", 40);
            let top = frame.lines().next().unwrap();
            let plain = strip_ansi(top);
            assert!(plain.starts_with("╭─ ✦ Title ✦ "), "{plain}");
            assert!(plain.ends_with("╮"), "{plain}");
            assert_eq!(vis_len(top), 40, "{plain}");
            let bot = frame.lines().last().unwrap();
            let bp = strip_ansi(bot);
            assert!(bp.starts_with('╰'), "{bp}");
            assert!(bp.ends_with('╯'), "{bp}");
        });
    }

    #[test]
    fn ascii_title_bar_shape() {
        with_unicode(false, || {
            let frame = box_frame(&["".into()], "Title", "", 40);
            let top = strip_ansi(frame.lines().next().unwrap());
            // fae_termart.box: tl+hz + " * Title * " → "+- * Title * "
            assert!(top.starts_with("+- * Title * "), "{top}");
            assert!(top.ends_with('+'), "{top}");
            assert_eq!(vis_len(&top), 40);
        });
    }

    #[test]
    fn vis_len_ignores_ansi() {
        let _g = ENV.lock().unwrap();
        set_force_color(true);
        let p = paint("hello", &[PINK, BOLD]);
        assert!(p.contains("\x1b[38;5;175m"), "{p:?}");
        assert_eq!(vis_len(&p), vis_len("hello"));
        assert!(pad_vis(&p, 10).contains(' '));
        set_force_color(false);
    }

    #[test]
    fn wrap_plain_respects_width() {
        let lines = wrap_plain("one two three four five six seven", 12);
        for ln in &lines {
            assert!(vis_len(ln) <= 12, "{ln}");
        }
        assert!(lines.len() > 1);
    }

    #[test]
    fn key_decode_matches_house() {
        assert_eq!(decode_byte(b'\r'), "enter");
        assert_eq!(decode_byte(0x13), "ctrl-s");
        assert_eq!(decode_byte(0x03), "ctrl-c");
        assert_eq!(decode_byte(0x7f), "backspace");
        assert_eq!(decode_csi("A"), "up");
        assert_eq!(decode_csi("B"), "down");
        assert_eq!(decode_csi("5~"), "pgup");
        assert_eq!(decode_csi("Z"), "shift-tab");
    }

    #[test]
    fn crossterm_keys_match_house() {
        let press = |code, mods| map_key_event(KeyEvent::new(code, mods));
        assert_eq!(press(KeyCode::Enter, KeyModifiers::NONE), "enter");
        assert_eq!(press(KeyCode::Esc, KeyModifiers::NONE), "esc");
        assert_eq!(press(KeyCode::Backspace, KeyModifiers::NONE), "backspace");
        assert_eq!(press(KeyCode::Tab, KeyModifiers::NONE), "tab");
        assert_eq!(press(KeyCode::Tab, KeyModifiers::SHIFT), "shift-tab");
        assert_eq!(press(KeyCode::BackTab, KeyModifiers::NONE), "shift-tab");
        assert_eq!(press(KeyCode::Up, KeyModifiers::NONE), "up");
        assert_eq!(press(KeyCode::Down, KeyModifiers::NONE), "down");
        assert_eq!(press(KeyCode::Left, KeyModifiers::NONE), "left");
        assert_eq!(press(KeyCode::Right, KeyModifiers::NONE), "right");
        assert_eq!(press(KeyCode::PageUp, KeyModifiers::NONE), "pgup");
        assert_eq!(press(KeyCode::PageDown, KeyModifiers::NONE), "pgdn");
        assert_eq!(press(KeyCode::Home, KeyModifiers::NONE), "home");
        assert_eq!(press(KeyCode::End, KeyModifiers::NONE), "end");
        assert_eq!(press(KeyCode::Delete, KeyModifiers::NONE), "delete");
        assert_eq!(press(KeyCode::Char('c'), KeyModifiers::CONTROL), "ctrl-c");
        assert_eq!(press(KeyCode::Char('s'), KeyModifiers::CONTROL), "ctrl-s");
        assert_eq!(press(KeyCode::Char('u'), KeyModifiers::CONTROL), "ctrl-u");
        assert_eq!(press(KeyCode::Char('a'), KeyModifiers::CONTROL), "ctrl-a");
        assert_eq!(press(KeyCode::Char('e'), KeyModifiers::CONTROL), "ctrl-e");
        assert_eq!(press(KeyCode::Char('r'), KeyModifiers::CONTROL), "ctrl-r");
        assert_eq!(press(KeyCode::Char('w'), KeyModifiers::CONTROL), "ctrl-w");
        assert_eq!(press(KeyCode::Char('p'), KeyModifiers::CONTROL), "ctrl-p");
        assert_eq!(press(KeyCode::Char('l'), KeyModifiers::CONTROL), "ctrl-l");
        assert_eq!(press(KeyCode::Char('d'), KeyModifiers::CONTROL), "ctrl-d");
        assert_eq!(press(KeyCode::Char(' '), KeyModifiers::NONE), "space");
        assert_eq!(press(KeyCode::Char('q'), KeyModifiers::NONE), "q");
        assert_eq!(press(KeyCode::Char('A'), KeyModifiers::SHIFT), "A");
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(map_key_event(release), "");
    }
}
