//! House TUI: stacked fae_termart boxes + Runes. No ratatui. No Python.

use crate::cli::{
    self, load_accounts_file, load_default_account, load_named_account, open_path, save_account,
};
use crate::config;
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum NestField {
    Name,
    Display,
    Email,
    Password,
    InHost,
    InPort,
    OutHost,
    OutPort,
}

struct NestForm {
    preset: Option<String>,
    name: String,
    display: String,
    email: String,
    password: String,
    in_host: String,
    in_port: String,
    out_host: String,
    out_port: String,
    field: NestField,
    confirm_quit: bool,
    status: String,
}

enum Screen {
    List,
    Reader { scroll: usize, attach_sel: usize },
    Compose { draft: Draft, confirm_quit: bool },
    NestPick { sel: usize },
    NestForm { form: NestForm },
}

struct App {
    store: Store,
    box_name: MailBox,
    sel: usize,
    status: String,
    screen: Screen,
    mails: Vec<MailMeta>,
    acc_name: String,
    acc_names: Vec<String>,
    query: String,
    searching: bool,
}

pub fn run() -> Result<u8, Error> {
    if !std::io::stdout().is_terminal() {
        return cli::dispatch(crate::Cmd::Peek {
            box_name: "unread".into(),
            plain: false,
        });
    }
    std::env::set_var("PIXIE_UNICODE", "1");
    let Some(fd) = art::tui_open_tty() else {
        return cli::dispatch(crate::Cmd::Peek {
            box_name: "unread".into(),
            plain: false,
        });
    };
    art::tui_begin(fd, "goblin");
    let (acc_name, acc_names) = match load_accounts_file() {
        Ok(f) => (
            f.default.clone(),
            f.accounts.into_iter().map(|a| a.name).collect(),
        ),
        Err(_) => ("goblin".into(), Vec::new()),
    };
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
        acc_names,
        query: String::new(),
        searching: false,
    };
    if app.acc_names.is_empty() {
        app.screen = Screen::NestPick { sel: 0 };
        app.status = "pick a sky to join".into();
    }
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
        Screen::NestPick { .. } => handle_nest_pick(app, key),
        Screen::NestForm { .. } => handle_nest_form(app, key),
    }
}

fn handle_list(app: &mut App, key: &str) -> Result<bool, Error> {
    if app.searching {
        match key {
            "esc" => {
                app.searching = false;
                app.query.clear();
                app.sel = 0;
            }
            "enter" => app.searching = false,
            "backspace" => {
                app.query.pop();
                app.sel = 0;
            }
            k if k.chars().count() == 1 && !k.starts_with("ctrl-") => {
                app.query.push_str(k);
                app.sel = 0;
            }
            _ => {}
        }
        return Ok(false);
    }
    match key {
        "q" | "ctrl-c" => return Ok(true),
        "esc" if !app.query.is_empty() => {
            app.query.clear();
            app.sel = 0;
            return Ok(false);
        }
        "esc" => return Ok(true),
        "/" => {
            app.searching = true;
            app.query.clear();
            app.sel = 0;
            return Ok(false);
        }
        "[" => {
            cycle_account(app, -1);
            return Ok(false);
        }
        "]" => {
            cycle_account(app, 1);
            return Ok(false);
        }
        "j" | "down" => {
            let n = visible(app).len();
            if app.sel + 1 < n {
                app.sel += 1;
            }
        }
        "k" | "up" => app.sel = app.sel.saturating_sub(1),
        "home" => app.sel = 0,
        "end" => app.sel = visible(app).len().saturating_sub(1),
        "pgdn" | "space" => {
            let n = visible(app).len();
            app.sel = (app.sel + 10).min(n.saturating_sub(1));
        }
        "pgup" => app.sel = app.sel.saturating_sub(10),
        "enter" | "o" => {
            if !visible(app).is_empty() {
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
        "N" => {
            app.screen = Screen::NestPick { sel: 0 };
        }
        _ => {}
    }
    Ok(false)
}

fn sky_ids() -> Vec<String> {
    let mut v: Vec<String> = config::NEST_PRESETS
        .iter()
        .map(|p| p.id.to_string())
        .collect();
    v.push("other".into());
    v
}

fn handle_nest_pick(app: &mut App, key: &str) -> Result<bool, Error> {
    let skies = sky_ids();
    let Screen::NestPick { sel } = &mut app.screen else {
        return Ok(false);
    };
    match key {
        "q" | "ctrl-c" if app.acc_names.is_empty() => return Ok(true),
        "esc" | "q" => {
            if app.acc_names.is_empty() {
                return Ok(true);
            }
            app.screen = Screen::List;
        }
        "j" | "down" => {
            if *sel + 1 < skies.len() {
                *sel += 1;
            }
        }
        "k" | "up" => *sel = sel.saturating_sub(1),
        "enter" | "o" => {
            let id = skies
                .get(*sel)
                .cloned()
                .unwrap_or_else(|| "purelymail".into());
            let preset = if id == "other" { None } else { Some(id) };
            app.screen = Screen::NestForm {
                form: NestForm {
                    preset,
                    name: "work".into(),
                    display: String::new(),
                    email: String::new(),
                    password: String::new(),
                    in_host: String::new(),
                    in_port: "993".into(),
                    out_host: String::new(),
                    out_port: "465".into(),
                    field: NestField::Name,
                    confirm_quit: false,
                    status: String::new(),
                },
            };
        }
        _ => {}
    }
    Ok(false)
}

fn nest_fields(other: bool) -> &'static [NestField] {
    if other {
        &[
            NestField::Name,
            NestField::Display,
            NestField::Email,
            NestField::Password,
            NestField::InHost,
            NestField::InPort,
            NestField::OutHost,
            NestField::OutPort,
        ]
    } else {
        &[
            NestField::Name,
            NestField::Display,
            NestField::Email,
            NestField::Password,
        ]
    }
}

fn handle_nest_form(app: &mut App, key: &str) -> Result<bool, Error> {
    let Screen::NestForm { form } = &mut app.screen else {
        return Ok(false);
    };
    if form.confirm_quit {
        if key == "y" || key == "Y" {
            if app.acc_names.is_empty() {
                app.screen = Screen::NestPick { sel: 0 };
            } else {
                app.screen = Screen::List;
                app.status = "nest add abandoned".into();
            }
        } else {
            form.confirm_quit = false;
        }
        return Ok(false);
    }
    let other = form.preset.is_none();
    let fields = nest_fields(other);
    match key {
        "esc" => form.confirm_quit = true,
        "ctrl-c" => return Ok(true),
        "ctrl-s" => return commit_nest(app),
        "tab" => {
            let i = fields.iter().position(|f| *f == form.field).unwrap_or(0);
            form.field = fields[(i + 1) % fields.len()];
        }
        "shift-tab" => {
            let i = fields.iter().position(|f| *f == form.field).unwrap_or(0);
            form.field = fields[(i + fields.len() - 1) % fields.len()];
        }
        "backspace" => nest_pop(form),
        k if k.chars().count() == 1 && !k.starts_with("ctrl-") => {
            nest_push(form, k.chars().next().unwrap());
        }
        _ => {}
    }
    Ok(false)
}

fn nest_push(form: &mut NestForm, c: char) {
    match form.field {
        NestField::Name => form.name.push(c),
        NestField::Display => form.display.push(c),
        NestField::Email => form.email.push(c),
        NestField::Password => form.password.push(c),
        NestField::InHost => form.in_host.push(c),
        NestField::InPort => form.in_port.push(c),
        NestField::OutHost => form.out_host.push(c),
        NestField::OutPort => form.out_port.push(c),
    }
}

fn nest_pop(form: &mut NestForm) {
    match form.field {
        NestField::Name => {
            form.name.pop();
        }
        NestField::Display => {
            form.display.pop();
        }
        NestField::Email => {
            form.email.pop();
        }
        NestField::Password => {
            form.password.pop();
        }
        NestField::InHost => {
            form.in_host.pop();
        }
        NestField::InPort => {
            form.in_port.pop();
        }
        NestField::OutHost => {
            form.out_host.pop();
        }
        NestField::OutPort => {
            form.out_port.pop();
        }
    }
}

pub fn mask_secret(s: &str) -> String {
    "•".repeat(s.chars().count())
}

fn commit_nest(app: &mut App) -> Result<bool, Error> {
    let Screen::NestForm { form } = &app.screen else {
        return Ok(false);
    };
    if form.email.trim().is_empty() {
        if let Screen::NestForm { form } = &mut app.screen {
            form.status = "need an email address".into();
        }
        return Ok(false);
    }
    if form.password.is_empty() {
        if let Screen::NestForm { form } = &mut app.screen {
            form.status = "need a password".into();
        }
        return Ok(false);
    }
    let name = if form.name.trim().is_empty() {
        "work".into()
    } else {
        form.name.trim().to_string()
    };
    let email = form.email.trim().to_string();
    let from = if form.display.trim().is_empty() {
        email.clone()
    } else {
        format!("{} <{}>", form.display.trim(), email)
    };
    let acc = if let Some(id) = form.preset.as_deref() {
        config::apply_preset(id, &name, &from, &email)?
    } else {
        let imap_port: u16 = form
            .in_port
            .parse()
            .map_err(|_| Error::say("bad incoming port", "use 993"))?;
        let smtp_port: u16 = form
            .out_port
            .parse()
            .map_err(|_| Error::say("bad outgoing port", "use 465 or 587"))?;
        crate::tls::imap_mode(imap_port)?;
        crate::tls::smtp_mode(smtp_port)?;
        crate::config::Account {
            name: name.clone(),
            from,
            imap: crate::config::Endpoint {
                host: form.in_host.trim().to_string(),
                port: imap_port,
                user: email.clone(),
            },
            smtp: crate::config::Endpoint {
                host: form.out_host.trim().to_string(),
                port: smtp_port,
                user: email,
            },
        }
    };
    crate::tls::imap_mode(acc.imap.port)?;
    crate::tls::smtp_mode(acc.smtp.port)?;
    let password = form.password.clone();
    save_account(acc, &password, true)?;
    let names = load_accounts_file()
        .map(|f| f.accounts.into_iter().map(|a| a.name).collect())
        .unwrap_or_default();
    app.acc_names = names;
    app.acc_name = name.clone();
    app.screen = Screen::List;
    app.status = format!("nest saved — {name}");
    app.reload();
    Ok(false)
}

fn handle_reader(app: &mut App, key: &str) -> Result<bool, Error> {
    let n = visible(app).len();
    match key {
        "q" | "esc" | "left" | "ctrl-c" => app.screen = Screen::List,
        "a" => open_selected_attachment(app)?,
        "n" => {
            let n = current_mail(app).map(|m| m.attachments.len()).unwrap_or(0);
            if let Screen::Reader { attach_sel, .. } = &mut app.screen {
                if n > 0 {
                    *attach_sel = (*attach_sel + 1) % n;
                }
            }
        }
        "j" | "down" | "space" => {
            if let Screen::Reader { scroll, .. } = &mut app.screen {
                *scroll = scroll.saturating_add(1);
            }
        }
        "k" | "up" => {
            if let Screen::Reader { scroll, .. } = &mut app.screen {
                *scroll = scroll.saturating_sub(1);
            }
        }
        "pgdn" => {
            if let Screen::Reader { scroll, .. } = &mut app.screen {
                *scroll = scroll.saturating_add(10);
            }
        }
        "pgup" => {
            if let Screen::Reader { scroll, .. } = &mut app.screen {
                *scroll = scroll.saturating_sub(10);
            }
        }
        "home" => {
            if let Screen::Reader { scroll, .. } = &mut app.screen {
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
                attachments: Vec::new(),
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

fn visible(app: &App) -> Vec<(MailBox, MailMeta)> {
    if app.query.trim().is_empty() {
        app.mails
            .iter()
            .cloned()
            .map(|m| (app.box_name, m))
            .collect()
    } else {
        crate::search::search_store(&app.store, &app.query).unwrap_or_default()
    }
}

fn current_mail(app: &App) -> Option<MailMeta> {
    match &app.screen {
        Screen::Reader { .. } | Screen::List => visible(app).get(app.sel).map(|(_, m)| m.clone()),
        Screen::Compose { draft, .. } => draft.reply_to.clone(),
        Screen::NestPick { .. } | Screen::NestForm { .. } => None,
    }
}

fn cycle_account(app: &mut App, dir: i32) {
    if app.acc_names.is_empty() {
        return;
    }
    let i = app
        .acc_names
        .iter()
        .position(|n| n == &app.acc_name)
        .unwrap_or(0);
    let n = app.acc_names.len() as i32;
    let next = ((i as i32 + dir).rem_euclid(n)) as usize;
    let name = app.acc_names[next].clone();
    let path = crate::paths::accounts_file();
    if path.exists() {
        if let Ok(mut file) = crate::config::load_accounts(&path) {
            if file.set_default(&name).is_ok() {
                let _ = crate::config::save_accounts(&path, &file);
            }
        }
    }
    app.acc_name = name;
    app.status = format!("account → {}", app.acc_name);
}

fn open_selected_attachment(app: &mut App) -> Result<(), Error> {
    let Some(m) = current_mail(app) else {
        return Ok(());
    };
    let files = app.store.list_attachments(&m.uid)?;
    let idx = match &app.screen {
        Screen::Reader { attach_sel, .. } => *attach_sel,
        _ => 0,
    };
    let Some(path) = files.get(idx) else {
        app.status = if m.attachments.is_empty() {
            "no attachments".into()
        } else {
            "attachments not on disk — sync --force".into()
        };
        return Ok(());
    };
    open_path(path)?;
    app.status = format!("opened {}", path.file_name().unwrap_or_default().to_string_lossy());
    Ok(())
}

fn open_selected(app: &mut App) -> Result<(), Error> {
    let Some((box_name, m)) = visible(app).get(app.sel).cloned() else {
        return Ok(());
    };
    app.box_name = box_name;
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
    app.screen = Screen::Reader {
        scroll: 0,
        attach_sel: 0,
    };
    Ok(())
}

fn start_reply(app: &mut App) {
    let Some(m) = current_mail(app) else {
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
    let Some((_, m)) = visible(app).get(app.sel).cloned() else {
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
    let loaded = if app.acc_name == "goblin" {
        load_default_account()
    } else {
        load_named_account(&app.acc_name).or_else(|_| load_default_account())
    };
    match loaded {
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
        let n = if self.query.trim().is_empty() {
            self.mails.len()
        } else {
            crate::search::search_store(&self.store, &self.query)
                .map(|v| v.len())
                .unwrap_or(0)
        };
        if self.sel >= n {
            self.sel = n.saturating_sub(1);
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
        Screen::Reader { scroll, attach_sel } => render_reader(app, tw, th, *scroll, *attach_sel),
        Screen::Compose {
            draft,
            confirm_quit,
        } => render_compose(app, tw, th, draft, *confirm_quit),
        Screen::NestPick { sel } => render_nest_pick(app, tw, th, *sel),
        Screen::NestForm { form } => render_nest_form(app, tw, th, form),
    }
}

fn render_list(app: &App, tw: usize, th: usize) -> String {
    let unread_n = app
        .store
        .load_mails(MailBox::Unread)
        .map(|m| m.len())
        .unwrap_or(0);
    let searching = if app.searching || !app.query.is_empty() {
        format!("search: {}{}", app.query, if app.searching { "█" } else { "" })
    } else {
        String::new()
    };
    let sub = format!(
        "{} · box: {} · unread: {unread_n} · last sync: {} · {}{sep}{searching}",
        app.acc_name,
        app.box_name.as_str(),
        last_sync(),
        app.status,
        sep = if searching.is_empty() { "" } else { " · " },
        searching = searching
    );
    let runes = art::box_frame(
        &["j/k move · / hunt · [] nest · N nest add · enter open · s steal · c compose · r reply · 1/2/3 box · q quit".into()],
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
    let vis = visible(app);
    let mut rows: Vec<String> = Vec::new();
    if vis.is_empty() {
        rows.push(art::paint(
            if app.query.is_empty() {
                "  (no messages in this box — press s to sync) "
            } else {
                "  (no matches) "
            },
            &[art::MUTED],
        ));
    } else {
        for (i, (b, m)) in vis.iter().enumerate().skip(start).take(avail) {
            let frm = trunc(&from_short(&m.from), 16);
            let box_tag = if app.query.is_empty() {
                String::new()
            } else {
                format!("{:<6} ", b.as_str())
            };
            let rest = tw.saturating_sub(38 + box_tag.len());
            let att = if m.attachments.is_empty() {
                ""
            } else {
                " ✎"
            };
            let line = format!(
                "  [{:>2}] {box_tag}{:<16} │ {}{att}  {}",
                i + 1,
                frm,
                trunc(&m.subject, rest.saturating_sub(16)),
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

fn render_reader(app: &App, tw: usize, th: usize, scroll: usize, attach_sel: usize) -> String {
    let Some(m) = current_mail(app) else {
        return render_list(app, tw, th);
    };
    let att = if m.attachments.is_empty() {
        String::new()
    } else {
        let mut parts = Vec::new();
        for (i, name) in m.attachments.iter().enumerate() {
            if i == attach_sel {
                parts.push(format!("[{name}]"));
            } else {
                parts.push(name.clone());
            }
        }
        format!(" · attach: {}", parts.join(" "))
    };
    let sub = format!(
        "{} · box {} · {} · {}{att}",
        app.acc_name,
        app.box_name.as_str(),
        m.uid,
        m.name()
    );
    let runes = art::box_frame(
        &["j/k scroll · a open attach · n next attach · t trash · m read · r reply · q back".into()],
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

fn render_nest_pick(app: &App, tw: usize, _th: usize, sel: usize) -> String {
    let skies = sky_ids();
    let mut rows = Vec::new();
    for (i, id) in skies.iter().enumerate() {
        let mark = if i == sel { "✦" } else { " " };
        let line = format!(" {mark}  {id}");
        if i == sel {
            rows.push(art::paint(&line, &[art::BOLD, art::BLUSH]));
        } else {
            rows.push(art::paint(&line, &[art::SILVER]));
        }
    }
    let sub = if app.acc_names.is_empty() {
        "pick a sky to join"
    } else {
        "add another nest"
    };
    let head = art::box_frame(&rows, "Goblin", sub, tw);
    let runes = art::box_frame(
        &["j/k move · enter choose · esc back".into()],
        "Runes",
        "",
        tw,
    );
    format!("{head}\n{runes}")
}

fn render_nest_form(_app: &App, tw: usize, _th: usize, form: &NestForm) -> String {
    let sky = form.preset.as_deref().unwrap_or("other");
    let app_pw = form
        .preset
        .as_deref()
        .and_then(config::find_preset)
        .map(|p| p.wants_app_password)
        .unwrap_or(false);
    let mut sub = format!("sky: {sky}");
    if app_pw {
        sub.push_str(" · app password if your mail house asks for one");
    }
    if form.confirm_quit {
        sub = "abandon this nest? y / any other key = stay".into();
    } else if !form.status.is_empty() {
        sub = format!("{sub} · {}", form.status);
    }
    let row = |on: bool, label: &str, val: &str| {
        let line = format!("{label:<14} {val}");
        if on {
            art::paint(&line, &[art::BOLD, art::BLUSH])
        } else {
            art::paint(&line, &[art::SILVER])
        }
    };
    let mut rows = vec![
        row(form.field == NestField::Name, "nest name", &form.name),
        row(form.field == NestField::Display, "your name", &form.display),
        row(form.field == NestField::Email, "email", &form.email),
        row(
            form.field == NestField::Password,
            "password",
            &mask_secret(&form.password),
        ),
    ];
    if form.preset.is_none() {
        rows.push(row(
            form.field == NestField::InHost,
            "incoming sky",
            &form.in_host,
        ));
        rows.push(row(
            form.field == NestField::InPort,
            "incoming port",
            &form.in_port,
        ));
        rows.push(row(
            form.field == NestField::OutHost,
            "outgoing sky",
            &form.out_host,
        ));
        rows.push(row(
            form.field == NestField::OutPort,
            "outgoing port",
            &form.out_port,
        ));
    }
    let head = art::box_frame(&rows, "Goblin", &sub, tw);
    let runes = art::box_frame(
        &["tab field · type · ctrl-s save · esc abandon".into()],
        "Runes",
        "",
        tw,
    );
    format!("{head}\n{runes}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_render_is_bullets_secret_stays() {
        let secret = "hunter2";
        let shown = mask_secret(secret);
        assert_eq!(shown.chars().count(), secret.chars().count());
        assert!(!shown.contains('h'));
        assert!(!shown.contains('2'));
        assert!(shown.chars().all(|c| c == '•'));
        assert_eq!(secret, "hunter2");
    }
}
