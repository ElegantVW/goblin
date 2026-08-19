//! House TUI: stacked fae_termart boxes + Runes. No ratatui. No Python.

use crate::cli::{self, load_default_account};
use crate::compose;
use crate::error::Error;
use crate::imap::{self, SyncOpts};
use crate::smtp;
use crate::store::{MailBox, MailMeta, Store};
use crate::termart as art;
use crate::{notify, paths};
use std::io::IsTerminal;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    To,
    Cc,
    Subject,
    Body,
}

struct Draft {
    to: String,
    cc: String,
    subject: String,
    body: String,
    field: Field,
    reply_to: Option<MailMeta>,
}

enum Screen {
    List,
    Reader { scroll: usize },
    Compose { draft: Draft, confirm_quit: bool },
}

struct App {
    store: Store,
    box_name: MailBox,
    sel: usize,
    status: String,
    screen: Screen,
    mails: Vec<MailMeta>,
    acc_name: String,
}

pub fn run() -> Result<u8, Error> {
    if !std::io::stdout().is_terminal() {
        return cli::dispatch(crate::Cmd::List {
            box_name: "unread".into(),
            plain: false,
        });
    }
    std::env::set_var("PIXIE_UNICODE", "1");
    let Some(fd) = art::tui_open_tty() else {
        return cli::dispatch(crate::Cmd::List {
            box_name: "unread".into(),
            plain: false,
        });
    };
    art::tui_begin(fd, "goblin");
    let acc_name = load_default_account()
        .map(|(a, _)| a.name)
        .unwrap_or_else(|_| "goblin".into());
    let store = Store::default_store();
    let _ = store.ensure();
    let mut app = App {
        store,
        box_name: MailBox::Unread,
        sel: 0,
        status: "welcome, master — the goblin guards your mail".into(),
        screen: Screen::List,
        mails: Vec::new(),
        acc_name,
    };
    app.reload();
    let result = event_loop(&mut app, fd);
    art::tui_cleanup();
    result
}

fn event_loop(app: &mut App, fd: i32) -> Result<u8, Error> {
    loop {
        let frame = render(app);
        art::paint_frame(fd, &frame);
        let key = art::tui_read_key(fd, None);
        if handle_key(app, &key)? {
            break;
        }
    }
    Ok(0)
}

fn handle_key(app: &mut App, key: &str) -> Result<bool, Error> {
    match &app.screen {
        Screen::List => handle_list(app, key),
        Screen::Reader { .. } => handle_reader(app, key),
        Screen::Compose { .. } => handle_compose(app, key),
    }
}

fn handle_list(app: &mut App, key: &str) -> Result<bool, Error> {
    match key {
        "q" | "esc" | "ctrl-c" => return Ok(true),
        "j" | "down" => {
            if app.sel + 1 < app.mails.len() {
                app.sel += 1;
            }
        }
        "k" | "up" => app.sel = app.sel.saturating_sub(1),
        "home" => app.sel = 0,
        "end" => app.sel = app.mails.len().saturating_sub(1),
        "pgdn" | "space" => {
            app.sel = (app.sel + 10).min(app.mails.len().saturating_sub(1));
        }
        "pgup" => app.sel = app.sel.saturating_sub(10),
        "enter" | "o" => {
            if !app.mails.is_empty() {
                open_selected(app)?;
            }
        }
        "s" => sync_now(app),
        "m" => mark_selected(app, MailBox::Read)?,
        "t" => mark_selected(app, MailBox::Trash)?,
        "1" | "u" => switch_box(app, MailBox::Unread),
        "2" => switch_box(app, MailBox::Read),
        "3" | "b" => switch_box(app, MailBox::Trash),
        "c" => {
            app.screen = Screen::Compose {
                draft: Draft {
                    to: String::new(),
                    cc: String::new(),
                    subject: String::new(),
                    body: String::new(),
                    field: Field::To,
                    reply_to: None,
                },
                confirm_quit: false,
            };
        }
        "r" | "R" => start_reply(app),
        _ => {}
    }
    Ok(false)
}

fn handle_reader(app: &mut App, key: &str) -> Result<bool, Error> {
    let n = app.mails.len();
    match key {
        "q" | "esc" | "left" | "ctrl-c" => app.screen = Screen::List,
        "j" | "down" | "space" => {
            if let Screen::Reader { scroll } = &mut app.screen {
                *scroll = scroll.saturating_add(1);
            }
        }
        "k" | "up" => {
            if let Screen::Reader { scroll } = &mut app.screen {
                *scroll = scroll.saturating_sub(1);
            }
        }
        "pgdn" => {
            if let Screen::Reader { scroll } = &mut app.screen {
                *scroll = scroll.saturating_add(10);
            }
        }
        "pgup" => {
            if let Screen::Reader { scroll } = &mut app.screen {
                *scroll = scroll.saturating_sub(10);
            }
        }
        "home" => {
            if let Screen::Reader { scroll } = &mut app.screen {
                *scroll = 0;
            }
        }
        "t" if n > 0 => {
            app.screen = Screen::List;
            mark_selected(app, MailBox::Trash)?;
        }
        "m" if n > 0 => {
            app.screen = Screen::List;
            mark_selected(app, MailBox::Read)?;
        }
        "r" | "R" => start_reply(app),
        _ => {}
    }
    Ok(false)
}

fn handle_compose(app: &mut App, key: &str) -> Result<bool, Error> {
    let Screen::Compose {
        draft,
        confirm_quit,
    } = &mut app.screen
    else {
        return Ok(false);
    };
    if *confirm_quit {
        if key == "y" || key == "Y" {
            app.screen = Screen::List;
            app.status = "draft abandoned".into();
        } else {
            *confirm_quit = false;
        }
        return Ok(false);
    }
    match key {
        "esc" => {
            *confirm_quit = true;
        }
        "ctrl-c" => return Ok(true),
        "ctrl-s" => return send_draft(app),
        "e" if draft.field == Field::Body => {
            let body = draft.body.clone();
            match art::edit_temp(&body) {
                Ok(new) => {
                    if let Screen::Compose { draft, .. } = &mut app.screen {
                        draft.body = new;
                    }
                }
                Err(e) => app.status = format!("editor: {e}"),
            }
        }
        "tab" => {
            draft.field = match draft.field {
                Field::To => Field::Cc,
                Field::Cc => Field::Subject,
                Field::Subject => Field::Body,
                Field::Body => Field::To,
            };
        }
        "shift-tab" => {
            draft.field = match draft.field {
                Field::To => Field::Body,
                Field::Cc => Field::To,
                Field::Subject => Field::Cc,
                Field::Body => Field::Subject,
            };
        }
        "enter" if draft.field == Field::Body => draft.body.push('\n'),
        "backspace" => pop_char(draft),
        k if k.chars().count() == 1 && !k.starts_with("ctrl-") => {
            insert_char(draft, k.chars().next().unwrap());
        }
        _ => {}
    }
    Ok(false)
}

fn insert_char(draft: &mut Draft, c: char) {
    match draft.field {
        Field::To => draft.to.push(c),
        Field::Cc => draft.cc.push(c),
        Field::Subject => draft.subject.push(c),
        Field::Body => draft.body.push(c),
    }
}

fn pop_char(draft: &mut Draft) {
    match draft.field {
        Field::To => {
            draft.to.pop();
        }
        Field::Cc => {
            draft.cc.pop();
        }
        Field::Subject => {
            draft.subject.pop();
        }
        Field::Body => {
            draft.body.pop();
        }
    }
}

fn send_draft(app: &mut App) -> Result<bool, Error> {
    let Screen::Compose { draft, .. } = &app.screen else {
        return Ok(false);
    };
    if draft.to.trim().is_empty() {
        app.status = "need a To: address".into();
        return Ok(false);
    }
    let (acc, password) = match load_default_account() {
        Ok(v) => v,
        Err(e) => {
            app.status = format!("{e}");
            return Ok(false);
        }
    };
    if let Err(e) = crate::tls::smtp_mode(acc.smtp.port) {
        app.status = format!("{e}");
        return Ok(false);
    }
    let ccs: Vec<String> = draft
        .cc
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let raw = if let Some(orig) = &draft.reply_to {
        compose::reply(&acc.from, orig, &draft.body)
    } else {
        compose::compose(&acc.from, &draft.to, &ccs, &draft.subject, &draft.body)
    };
    let mut rcpts = vec![draft.to.clone()];
    rcpts.extend(ccs);
    match smtp::send(&acc, &password, &rcpts, &raw) {
        Ok(()) => {
            let meta = MailMeta {
                uid: format!("sent-{}", chrono::Utc::now().timestamp()),
                account: acc.name.clone(),
                folder: "SENT".into(),
                from: acc.from.clone(),
                to: draft.to.clone(),
                date: chrono::Utc::now().to_rfc2822(),
                subject: draft.subject.clone(),
                message_id: String::new(),
                path: None,
                body: String::new(),
            };
            let _ = app.store.write_mail(MailBox::Read, &meta, &draft.body);
            app.status = format!("sent to {}", draft.to);
            app.screen = Screen::List;
            app.reload();
        }
        Err(e) => app.status = format!("send failed: {e}"),
    }
    Ok(false)
}

fn open_selected(app: &mut App) -> Result<(), Error> {
    let Some(m) = app.mails.get(app.sel).cloned() else {
        return Ok(());
    };
    if app.box_name == MailBox::Unread {
        if let Some(path) = &m.path {
            if let Ok((acc, password)) = load_default_account() {
                if !m.uid.is_empty() {
                    let _ = imap::mark_seen(&acc, &password, &m.uid);
                }
            }
            let _ = app.store.move_mail(path, MailBox::Read);
        }
        app.status = format!("read → {}", m.name());
        app.box_name = MailBox::Read;
        app.reload();
        if let Some(i) = app.mails.iter().position(|x| x.uid == m.uid && !m.uid.is_empty()) {
            app.sel = i;
        }
    }
    app.screen = Screen::Reader { scroll: 0 };
    Ok(())
}

fn start_reply(app: &mut App) {
    let Some(m) = app.mails.get(app.sel).cloned() else {
        return;
    };
    let to = if let (Some(s), Some(e)) = (m.from.find('<'), m.from.find('>')) {
        m.from[s + 1..e].trim().to_string()
    } else {
        m.from.clone()
    };
    let subject = if m.subject.len() >= 3 && m.subject[..3].eq_ignore_ascii_case("re:") {
        m.subject.clone()
    } else {
        format!("Re: {}", m.subject)
    };
    let body = compose::quote_body(&m);
    app.screen = Screen::Compose {
        draft: Draft {
            to,
            cc: String::new(),
            subject,
            body,
            field: Field::Body,
            reply_to: Some(m),
        },
        confirm_quit: false,
    };
}

fn mark_selected(app: &mut App, dest: MailBox) -> Result<(), Error> {
    let Some(m) = app.mails.get(app.sel).cloned() else {
        return Ok(());
    };
    let Some(path) = m.path.clone() else {
        return Ok(());
    };
    if let Ok((acc, password)) = load_default_account() {
        if !m.uid.is_empty() {
            match dest {
                MailBox::Read => {
                    let _ = imap::mark_seen(&acc, &password, &m.uid);
                }
                MailBox::Trash => {
                    let _ = imap::trash(&acc, &password, &m.uid);
                }
                MailBox::Unread => {}
            }
        }
    }
    let _ = app.store.move_mail(&path, dest);
    app.status = format!("{} → {}", dest.as_str(), m.name());
    app.reload();
    if app.sel >= app.mails.len() {
        app.sel = app.mails.len().saturating_sub(1);
    }
    Ok(())
}

fn switch_box(app: &mut App, b: MailBox) {
    app.box_name = b;
    app.sel = 0;
    app.reload();
}

fn sync_now(app: &mut App) {
    match load_default_account() {
        Ok((acc, password)) => match imap::sync(
            &acc,
            &password,
            &app.store,
            SyncOpts {
                all: false,
                force: false,
                limit: 30,
                folder: None,
            },
        ) {
            Ok(r) => {
                if r.written > 0 {
                    let _ = notify::play();
                    app.status = format!("synced — {} new mail(s) squeaked in", r.written);
                } else {
                    app.status = "synced — nothing new".into();
                }
            }
            Err(e) => app.status = format!("sync failed: {e}"),
        },
        Err(e) => app.status = format!("sync failed: {e}"),
    }
    app.reload();
}

impl App {
    fn reload(&mut self) {
        self.mails = self.store.load_mails(self.box_name).unwrap_or_default();
        if self.sel >= self.mails.len() {
            self.sel = self.mails.len().saturating_sub(1);
        }
    }
}

fn last_sync() -> String {
    let path = paths::state_file();
    let Ok(text) = std::fs::read_to_string(path) else {
        return "never".into();
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("last_sync")?.as_str().map(str::to_string))
        .unwrap_or_else(|| "never".into())
}

fn from_short(from: &str) -> String {
    if let Some(i) = from.find('<') {
        let s = from[..i].trim().trim_matches('"');
        if !s.is_empty() {
            return s.to_string();
        }
    }
    from.to_string()
}

fn trunc(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    if art::vis_len(s) <= n {
        return s.to_string();
    }
    art::pad_vis(s, n).trim_end().to_string()
}

fn render(app: &App) -> String {
    let tw = art::term_width();
    let th = art::term_height();
    match &app.screen {
        Screen::List => render_list(app, tw, th),
        Screen::Reader { scroll } => render_reader(app, tw, th, *scroll),
        Screen::Compose {
            draft,
            confirm_quit,
        } => render_compose(app, tw, th, draft, *confirm_quit),
    }
}

fn render_list(app: &App, tw: usize, th: usize) -> String {
    let unread_n = app
        .store
        .load_mails(MailBox::Unread)
        .map(|m| m.len())
        .unwrap_or(0);
    let sub = format!(
        "{} · box: {} · unread: {unread_n} · last sync: {} · {}",
        app.acc_name,
        app.box_name.as_str(),
        last_sync(),
        app.status
    );
    let runes = art::box_frame(
        &["j/k move · enter open · s sync · m read · t trash · c compose · r reply · 1/2/3 box · q quit".into()],
        "Runes",
        "",
        tw,
    );
    let runes_h = art::line_count(&runes);
    // subtitle adds 2 lines (text + rule) inside the box
    let head_chrome = 4; // top + bottom + subtitle + rule
    let avail = th.saturating_sub(runes_h + 1 + head_chrome).max(3);
    let mut start = 0;
    if app.sel >= avail {
        start = app.sel + 1 - avail;
    }
    let mut rows: Vec<String> = Vec::new();
    if app.mails.is_empty() {
        rows.push(art::paint(
            "  (no messages in this box — press s to sync) ",
            &[art::MUTED],
        ));
    } else {
        for (i, m) in app.mails.iter().enumerate().skip(start).take(avail) {
            let frm = trunc(&from_short(&m.from), 18);
            let rest = tw.saturating_sub(36);
            let line = format!(
                "  [{:>2}] {:<18} │ {}  {}",
                i + 1,
                frm,
                trunc(&m.subject, rest.saturating_sub(18)),
                trunc(&m.date, 16)
            );
            if i == app.sel {
                rows.push(art::paint(&line, &[art::BOLD, art::BLUSH]));
            } else {
                rows.push(art::paint(&line, &[art::SILVER]));
            }
        }
    }
    while rows.len() < avail {
        rows.push(String::new());
    }
    let head = art::box_frame(&rows, "Goblin", &sub, tw);
    format!("{head}\n{runes}")
}

fn render_reader(app: &App, tw: usize, th: usize, scroll: usize) -> String {
    let Some(m) = app.mails.get(app.sel) else {
        return render_list(app, tw, th);
    };
    let sub = format!(
        "{} · box {} · {} · {}",
        app.acc_name,
        app.box_name.as_str(),
        m.uid,
        m.name()
    );
    let runes = art::box_frame(
        &["j/k scroll · t trash · m read · r reply · q back".into()],
        "Runes",
        "",
        tw,
    );
    let head = art::box_frame(
        &[
            format!("From:    {}", m.from),
            format!("Date:    {}", m.date),
            format!("Subject: {}", m.subject),
        ],
        "Goblin",
        &sub,
        tw,
    );
    let runes_h = art::line_count(&runes);
    let head_h = art::line_count(&head);
    let avail = th.saturating_sub(runes_h + head_h + 2).max(3);
    let wrapped = art::wrap_plain(&m.body, tw.saturating_sub(4));
    let slice: Vec<String> = wrapped.iter().skip(scroll).take(avail).cloned().collect();
    let mut body_rows = slice;
    while body_rows.len() < avail {
        body_rows.push(String::new());
    }
    let body = art::box_frame(&body_rows, "letter", "", tw);
    format!("{head}\n{body}\n{runes}")
}

fn render_compose(app: &App, tw: usize, th: usize, draft: &Draft, confirm_quit: bool) -> String {
    let sub = if confirm_quit {
        "abandon this draft? y / any other key = stay".into()
    } else {
        format!("{} · compose · {}", app.acc_name, app.status)
    };
    let mark = |on: bool, label: &str, val: &str| {
        let line = format!("{label:<8} {val}");
        if on {
            art::paint(&line, &[art::BOLD, art::BLUSH])
        } else {
            art::paint(&line, &[art::SILVER])
        }
    };
    let head = art::box_frame(
        &[
            mark(draft.field == Field::To, "To:", &draft.to),
            mark(draft.field == Field::Cc, "Cc:", &draft.cc),
            mark(draft.field == Field::Subject, "Subject:", &draft.subject),
        ],
        "Goblin",
        &sub,
        tw,
    );
    let runes = art::box_frame(
        &["tab field · type · ctrl-s send · e editor · esc abandon".into()],
        "Runes",
        "",
        tw,
    );
    let runes_h = art::line_count(&runes);
    let head_h = art::line_count(&head);
    let avail = th.saturating_sub(runes_h + head_h + 2).max(3);
    let wrapped = art::wrap_plain(&draft.body, tw.saturating_sub(4));
    let mut body_rows: Vec<String> = wrapped.into_iter().take(avail).collect();
    if body_rows.is_empty() {
        body_rows.push(if draft.field == Field::Body {
            art::paint(" ", &[art::BOLD, art::BLUSH])
        } else {
            String::new()
        });
    }
    while body_rows.len() < avail {
        body_rows.push(String::new());
    }
    if draft.field == Field::Body {
        if let Some(last) = body_rows.iter_mut().rev().find(|s| !s.is_empty()) {
            *last = art::paint(&art::strip_ansi(last), &[art::BOLD, art::BLUSH]);
        }
    }
    let body = art::box_frame(&body_rows, "letter", "", tw);
    format!("{head}\n{body}\n{runes}")
}
