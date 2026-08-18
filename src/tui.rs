//! List, reader, composer. Pink house chrome. Not fae_termart.

use crate::cli::{self, load_default_account, resolve_mail, secret_id};
use crate::compose;
use crate::error::Error;
use crate::imap::{self, SyncOpts};
use crate::smtp;
use crate::store::{MailBox, MailMeta, Store};
use crate::{notify, secrets};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::time::Duration;

const PINK: Color = Color::Indexed(175);
const PINK_DIM: Color = Color::Indexed(132);
const BLUSH: Color = Color::Indexed(218);
const SILVER: Color = Color::Indexed(252);
const MUTED: Color = Color::Indexed(245);

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
    Reader { scroll: u16 },
    Compose { draft: Draft, confirm_quit: bool },
}

struct App {
    store: Store,
    box_name: MailBox,
    sel: usize,
    offset: usize,
    status: String,
    screen: Screen,
    mails: Vec<MailMeta>,
    acc_name: String,
    unicode: bool,
}

pub fn run() -> Result<u8, Error> {
    if !io::stdout().is_terminal() {
        return cli::dispatch(crate::Cmd::List {
            box_name: "unread".into(),
            plain: false,
        });
    }
    let unicode = unicode_on();
    let acc_name = load_default_account()
        .map(|(a, _)| a.name)
        .unwrap_or_else(|_| "goblin".into());
    let store = Store::default_store();
    store.ensure()?;
    let mut app = App {
        store,
        box_name: MailBox::Unread,
        sel: 0,
        offset: 0,
        status: "welcome, master — the goblin guards your mail".into(),
        screen: Screen::List,
        mails: Vec::new(),
        acc_name,
        unicode,
    };
    app.reload();

    enable_raw_mode().map_err(|e| Error::Io(io::Error::other(e.to_string())))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| Error::Io(io::Error::other(e.to_string())))?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend).map_err(|e| Error::Io(io::Error::other(e.to_string())))?;

    let result = event_loop(&mut term, &mut app);
    let _ = disable_raw_mode();
    let _ = execute!(term.backend_mut(), LeaveAlternateScreen);
    let _ = term.show_cursor();
    result
}

fn event_loop(
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<u8, Error> {
    loop {
        term.draw(|f| draw(f, app))
            .map_err(|e| Error::Io(io::Error::other(e.to_string())))?;
        if !event::poll(Duration::from_millis(200)).unwrap_or(false) {
            continue;
        }
        let ev = match event::read() {
            Ok(e) => e,
            Err(_) => break,
        };
        match ev {
            Event::Key(k) if k.kind == KeyEventKind::Press || k.kind == KeyEventKind::Repeat => {
                if handle_key(app, term, k)? {
                    break;
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
    Ok(0)
}

fn handle_key(
    app: &mut App,
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    k: KeyEvent,
) -> Result<bool, Error> {
    match &mut app.screen {
        Screen::List => handle_list(app, k),
        Screen::Reader { .. } => handle_reader(app, k),
        Screen::Compose { .. } => handle_compose(app, term, k),
    }
}

fn handle_list(app: &mut App, k: KeyEvent) -> Result<bool, Error> {
    match k.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            if k.modifiers.contains(KeyModifiers::CONTROL) || matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
                return Ok(true);
            }
        }
        KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Char('j') | KeyCode::Down => {
            if app.sel + 1 < app.mails.len() {
                app.sel += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.sel = app.sel.saturating_sub(1);
        }
        KeyCode::Home => app.sel = 0,
        KeyCode::End => app.sel = app.mails.len().saturating_sub(1),
        KeyCode::Enter | KeyCode::Char('o') => {
            if !app.mails.is_empty() {
                open_selected(app)?;
            }
        }
        KeyCode::Char('s') => sync_now(app),
        KeyCode::Char('m') => mark_selected(app, MailBox::Read)?,
        KeyCode::Char('t') => mark_selected(app, MailBox::Trash)?,
        KeyCode::Char('1') | KeyCode::Char('u') => switch_box(app, MailBox::Unread),
        KeyCode::Char('2') => switch_box(app, MailBox::Read),
        KeyCode::Char('3') | KeyCode::Char('b') => switch_box(app, MailBox::Trash),
        KeyCode::Char('c') => {
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
        KeyCode::Char('r') | KeyCode::Char('R') => start_reply(app),
        _ => {}
    }
    Ok(false)
}

fn handle_reader(app: &mut App, k: KeyEvent) -> Result<bool, Error> {
    let n = app.mails.len();
    let Screen::Reader { scroll } = &mut app.screen else {
        return Ok(false);
    };
    match k.code {
        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Left => {
            app.screen = Screen::List;
        }
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Char(' ') => *scroll = scroll.saturating_add(1),
        KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
        KeyCode::PageDown => *scroll = scroll.saturating_add(10),
        KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
        KeyCode::Home => *scroll = 0,
        KeyCode::Char('t') => {
            if n > 0 {
                app.screen = Screen::List;
                mark_selected(app, MailBox::Trash)?;
            }
        }
        KeyCode::Char('m') => {
            if n > 0 {
                app.screen = Screen::List;
                mark_selected(app, MailBox::Read)?;
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            start_reply(app);
        }
        _ => {}
    }
    let _ = n;
    Ok(false)
}

fn handle_compose(
    app: &mut App,
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    k: KeyEvent,
) -> Result<bool, Error> {
    let Screen::Compose {
        draft,
        confirm_quit,
    } = &mut app.screen
    else {
        return Ok(false);
    };
    if *confirm_quit {
        match k.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.screen = Screen::List;
                app.status = "draft abandoned".into();
            }
            _ => *confirm_quit = false,
        }
        return Ok(false);
    }
    match k.code {
        KeyCode::Esc => {
            *confirm_quit = true;
            return Ok(false);
        }
        KeyCode::Char('s') if k.modifiers.contains(KeyModifiers::CONTROL) => {
            return send_draft(app);
        }
        KeyCode::Char('e') if k.modifiers.contains(KeyModifiers::CONTROL) => {
            let body = draft.body.clone();
            match open_editor(term, &body) {
                Ok(new) => {
                    if let Screen::Compose { draft, .. } = &mut app.screen {
                        draft.body = new;
                    }
                }
                Err(e) => app.status = format!("editor: {e}"),
            }
            return Ok(false);
        }
        KeyCode::Tab => {
            draft.field = match draft.field {
                Field::To => Field::Cc,
                Field::Cc => Field::Subject,
                Field::Subject => Field::Body,
                Field::Body => Field::To,
            };
            return Ok(false);
        }
        KeyCode::BackTab => {
            draft.field = match draft.field {
                Field::To => Field::Body,
                Field::Cc => Field::To,
                Field::Subject => Field::Cc,
                Field::Body => Field::Subject,
            };
            return Ok(false);
        }
        KeyCode::Char('e') if draft.field == Field::Body => {
            let body = draft.body.clone();
            match open_editor(term, &body) {
                Ok(new) => {
                    if let Screen::Compose { draft, .. } = &mut app.screen {
                        draft.body = new;
                    }
                }
                Err(e) => app.status = format!("editor: {e}"),
            }
            return Ok(false);
        }
        KeyCode::Enter if draft.field == Field::Body => {
            draft.body.push('\n');
            return Ok(false);
        }
        KeyCode::Backspace => {
            pop_char(draft);
            return Ok(false);
        }
        KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
            insert_char(draft, c);
            return Ok(false);
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
        Err(e) => {
            app.status = format!("send failed: {e}");
        }
    }
    Ok(false)
}

fn open_editor(
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    body: &str,
) -> Result<String, Error> {
    let _ = disable_raw_mode();
    let _ = execute!(term.backend_mut(), LeaveAlternateScreen);
    let dir = std::env::temp_dir();
    let path: PathBuf = dir.join(format!("goblin-draft-{}.txt", std::process::id()));
    std::fs::write(&path, body)?;
    let editor = std::env::var("EDITOR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "nano".into()
            }
        });
    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| Error::Usage(format!("{editor}: {e}")))?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    let _ = enable_raw_mode();
    let _ = execute!(term.backend_mut(), EnterAlternateScreen);
    let _ = term.clear();
    if !status.success() {
        return Err(Error::Usage(format!("{editor} exited {}", status)));
    }
    Ok(text)
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
        app.reload();
        // find the mail in read box to display
        if let Some(i) = app.mails.iter().position(|x| x.uid == m.uid && !m.uid.is_empty()) {
            app.sel = i;
        } else if let Some(i) = app.mails.iter().position(|x| x.subject == m.subject && x.from == m.from)
        {
            app.sel = i;
        }
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
    app.offset = 0;
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

fn draw(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(f.area());
    match &app.screen {
        Screen::List => draw_list(f, chunks[0], app),
        Screen::Reader { scroll } => draw_reader(f, chunks[0], app, *scroll),
        Screen::Compose {
            draft,
            confirm_quit,
        } => draw_compose(f, chunks[0], app, draft, *confirm_quit),
    }
    let footer = match &app.screen {
        Screen::List => {
            "runes: ↑↓/j/k move · enter open · s sync · m read · t trash · c compose · r reply · 1/2/3 box · q quit"
        }
        Screen::Reader { .. } => "runes: j/k scroll · t trash · m read · r reply · q back",
        Screen::Compose { confirm_quit, .. } if *confirm_quit => "abandon draft? y / any other key = stay",
        Screen::Compose { .. } => {
            "runes: tab field · type · C-s send · e/$EDITOR · esc abandon"
        }
    };
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(PINK_DIM)),
        chunks[1],
    );
}

fn frame_block<'a>(app: &App, title: &'a str, subtitle: String) -> Block<'a> {
    let (tl, tr, bl, br) = if app.unicode {
        ("╭", "╮", "╰", "╯")
    } else {
        ("+", "+", "+", "+")
    };
    let mark = if app.unicode { "✦" } else { "*" };
    Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::Set {
            top_left: tl,
            top_right: tr,
            bottom_left: bl,
            bottom_right: br,
            vertical_left: if app.unicode { "│" } else { "|" },
            vertical_right: if app.unicode { "│" } else { "|" },
            horizontal_top: if app.unicode { "─" } else { "-" },
            horizontal_bottom: if app.unicode { "─" } else { "-" },
        })
        .border_style(Style::default().fg(PINK_DIM))
        .title(Span::styled(
            format!(" {mark} {title} "),
            Style::default().fg(PINK).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            format!(" {subtitle} "),
            Style::default().fg(MUTED),
        ))
}

fn draw_list(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let unread_n = app
        .store
        .load_mails(MailBox::Unread)
        .map(|m| m.len())
        .unwrap_or(0);
    let subtitle = format!(
        "{} · box: {} · unread: {unread_n} · last sync: {} · {}",
        app.acc_name,
        app.box_name.as_str(),
        last_sync(),
        app.status
    );
    let block = frame_block(app, "Goblin", subtitle);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if app.mails.is_empty() {
        f.render_widget(
            Paragraph::new("  (no messages in this box — press s to sync) ")
                .style(Style::default().fg(MUTED)),
            inner,
        );
        return;
    }
    let h = inner.height as usize;
    let mut start = app.offset;
    if app.sel >= start + h {
        start = app.sel + 1 - h;
    }
    if app.sel < start {
        start = app.sel;
    }
    let mut lines = Vec::new();
    for (i, m) in app.mails.iter().enumerate().skip(start).take(h) {
        let frm = from_short(&m.from);
        let line = format!(
            "  [{:>2}] {:<18} │ {}  {}",
            i + 1,
            trunc(&frm, 18),
            trunc(&m.subject, inner.width.saturating_sub(40) as usize),
            trunc(&m.date, 16)
        );
        let style = if i == app.sel {
            Style::default().fg(BLUSH).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(SILVER)
        };
        lines.push(Line::from(Span::styled(line, style)));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_reader(f: &mut ratatui::Frame, area: Rect, app: &App, scroll: u16) {
    let Some(m) = app.mails.get(app.sel) else {
        return;
    };
    let subtitle = format!(
        "{} · box {} · {} · {}",
        app.acc_name,
        app.box_name.as_str(),
        m.uid,
        m.name()
    );
    let block = frame_block(app, "Goblin", subtitle);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut text = format!("  From:    {}\n  Date:    {}\n  Subject: {}\n\n", m.from, m.date, m.subject);
    text.push_str(&m.body);
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(SILVER))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
}

fn draw_compose(
    f: &mut ratatui::Frame,
    area: Rect,
    app: &App,
    draft: &Draft,
    confirm_quit: bool,
) {
    let subtitle = if confirm_quit {
        "abandon this draft?".into()
    } else {
        format!("{} · compose · {}", app.acc_name, app.status)
    };
    let block = frame_block(app, "Goblin", subtitle);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let hi = Style::default().fg(BLUSH).add_modifier(Modifier::BOLD);
    let lo = Style::default().fg(SILVER);
    let line = |label: &str, val: &str, on: bool| {
        Line::from(vec![
            Span::styled(format!("  {label:<8}"), if on { hi } else { Style::default().fg(MUTED) }),
            Span::styled(val.to_string(), if on { hi } else { lo }),
        ])
    };
    let mut lines = vec![
        line("To:", &draft.to, draft.field == Field::To),
        line("Cc:", &draft.cc, draft.field == Field::Cc),
        line("Subject:", &draft.subject, draft.field == Field::Subject),
        Line::from(""),
        Line::from(Span::styled(
            "  Body:",
            if draft.field == Field::Body {
                hi
            } else {
                Style::default().fg(MUTED)
            },
        )),
    ];
    for l in draft.body.lines() {
        lines.push(Line::from(Span::styled(format!("  {l}"), lo)));
    }
    if draft.body.is_empty() {
        lines.push(Line::from(Span::styled("  ", lo)));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn last_sync() -> String {
    let path = crate::paths::state_file();
    let Ok(text) = std::fs::read_to_string(path) else {
        return "never".into();
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("last_sync")?.as_str().map(str::to_string))
        .unwrap_or_else(|| "never".into())
}

fn unicode_on() -> bool {
    match std::env::var("PIXIE_UNICODE") {
        Ok(v) => matches!(
            v.to_ascii_lowercase().as_str(),
            "1" | "on" | "true" | "yes" | "unicode"
        ),
        Err(_) => false,
    }
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
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[allow(dead_code)]
fn _use_resolve(store: &Store, spec: &str) -> Result<PathBuf, Error> {
    resolve_mail(store, spec)
}

#[allow(dead_code)]
fn _use_secret(acc: &crate::config::Account) -> String {
    let _ = secrets::load_password(&secret_id(acc));
    secret_id(acc)
}
