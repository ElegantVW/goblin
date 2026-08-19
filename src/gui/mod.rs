//! egui mail UI — Windows desktop target; also runs on Linux.

use crate::config::{self, Account, Endpoint};
use crate::error::Error;
use crate::ops;
use crate::search;
use crate::store::{MailBox, MailMeta};
use eframe::egui::{self, Color32, RichText, Ui};
use std::sync::mpsc::{self, Receiver};
use std::thread;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Mail,
    Compose,
    AddAccount,
    EditAccount,
}

pub struct GoblinApp {
    account: String,
    accounts: Vec<String>,
    box_name: MailBox,
    mails: Vec<MailMeta>,
    filtered: Vec<usize>,
    sel: Option<usize>,
    query: String,
    status: String,
    panel: Panel,
    compose: ComposeState,
    add: AddState,
    edit: EditState,
    busy: bool,
    rx: Option<Receiver<UiMsg>>,
    confirm_remove: Option<String>,
}

struct ComposeState {
    to: String,
    cc: String,
    subject: String,
    body: String,
    #[allow(dead_code)]
    reply_to: Option<MailMeta>,
}

struct AddState {
    preset: usize,
    name: String,
    from: String,
    email: String,
    password: String,
    in_host: String,
    in_port: String,
    out_host: String,
    out_port: String,
}

struct EditState {
    old_name: String,
    name: String,
    from: String,
    email: String,
    password: String,
    in_host: String,
    in_port: String,
    out_host: String,
    out_port: String,
}

enum UiMsg {
    Stolen(Result<usize, String>),
    Sent(Result<(), String>),
}

impl Default for GoblinApp {
    fn default() -> Self {
        let mut app = Self {
            account: String::new(),
            accounts: Vec::new(),
            box_name: MailBox::Unread,
            mails: Vec::new(),
            filtered: Vec::new(),
            sel: None,
            query: String::new(),
            status: "welcome — the goblin guards your mail".into(),
            panel: Panel::Mail,
            compose: ComposeState {
                to: String::new(),
                cc: String::new(),
                subject: String::new(),
                body: String::new(),
                reply_to: None,
            },
            add: AddState {
                preset: 0,
                name: "work".into(),
                from: String::new(),
                email: String::new(),
                password: String::new(),
                in_host: String::new(),
                in_port: "993".into(),
                out_host: String::new(),
                out_port: "465".into(),
            },
            edit: EditState {
                old_name: String::new(),
                name: String::new(),
                from: String::new(),
                email: String::new(),
                password: String::new(),
                in_host: String::new(),
                in_port: "993".into(),
                out_host: String::new(),
                out_port: "465".into(),
            },
            busy: false,
            rx: None,
            confirm_remove: None,
        };
        app.reload_accounts();
        app.reload_mails();
        if app.accounts.is_empty() {
            app.panel = Panel::AddAccount;
            app.status = "summon a goblin — which sky do they watch?".into();
        }
        app
    }
}

impl GoblinApp {
    fn reload_accounts(&mut self) {
        match ops::accounts() {
            Ok(f) => {
                self.accounts = f.accounts.iter().map(|a| a.name.clone()).collect();
                if self.account.is_empty() || !self.accounts.iter().any(|a| a == &self.account) {
                    self.account = f.default;
                }
            }
            Err(e) => {
                self.accounts.clear();
                self.account.clear();
                self.status = format!("accounts: {e}");
            }
        }
    }

    fn reload_mails(&mut self) {
        match ops::list_mails(self.box_name) {
            Ok(m) => {
                self.mails = m;
                self.apply_filter();
                if let Some(i) = self.sel {
                    if i >= self.filtered.len() {
                        self.sel = None;
                    }
                }
            }
            Err(e) => self.status = format!("list: {e}"),
        }
    }

    fn apply_filter(&mut self) {
        let q = self.query.trim();
        if q.is_empty() {
            self.filtered = (0..self.mails.len()).collect();
        } else {
            self.filtered = self
                .mails
                .iter()
                .enumerate()
                .filter(|(_, m)| search::matches(m, q))
                .map(|(i, _)| i)
                .collect();
        }
    }

    fn selected(&self) -> Option<&MailMeta> {
        self.sel
            .and_then(|i| self.filtered.get(i).copied())
            .and_then(|j| self.mails.get(j))
    }

    fn poll_bg(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        let mut keep = true;
        while let Ok(msg) = rx.try_recv() {
            self.busy = false;
            keep = false;
            match msg {
                UiMsg::Stolen(Ok(n)) => {
                    self.status = format!("stole {n} letter(s)");
                    self.reload_mails();
                }
                UiMsg::Stolen(Err(e)) => self.status = format!("steal failed: {e}"),
                UiMsg::Sent(Ok(())) => {
                    self.status = "sent".into();
                    self.panel = Panel::Mail;
                    self.reload_mails();
                }
                UiMsg::Sent(Err(e)) => self.status = format!("send failed: {e}"),
            }
        }
        if keep && self.busy {
            self.rx = Some(rx);
        }
    }

    fn start_steal(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.status = "stealing…".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let acc = if self.account.is_empty() {
            None
        } else {
            Some(self.account.clone())
        };
        thread::spawn(move || {
            let r = ops::steal(acc.as_deref(), true, 50).map_err(|e| e.to_string());
            let _ = tx.send(UiMsg::Stolen(r));
        });
    }

    fn start_send(&mut self) {
        if self.busy {
            return;
        }
        let to = self.compose.to.trim().to_string();
        if to.is_empty() {
            self.status = "need a To address".into();
            return;
        }
        self.busy = true;
        self.status = "sending…".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let acc = if self.account.is_empty() {
            None
        } else {
            Some(self.account.clone())
        };
        let cc: Vec<String> = self
            .compose
            .cc
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let subject = self.compose.subject.clone();
        let body = self.compose.body.clone();
        thread::spawn(move || {
            let r = ops::send_mail(acc.as_deref(), &to, &cc, &subject, &body)
                .map_err(|e| e.to_string());
            let _ = tx.send(UiMsg::Sent(r));
        });
    }
}

impl eframe::App for GoblinApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_bg();
        if self.busy {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Goblin").color(PINK));
                ui.separator();
                if !self.accounts.is_empty() {
                    egui::ComboBox::from_id_salt("account")
                        .selected_text(&self.account)
                        .show_ui(ui, |ui| {
                            for a in self.accounts.clone() {
                                if ui.selectable_label(self.account == a, &a).clicked() {
                                    if let Err(e) = ops::set_default_account(&a) {
                                        self.status = format!("{e}");
                                    } else {
                                        self.account = a;
                                        self.status = format!("woke {}", self.account);
                                    }
                                }
                            }
                        });
                }
                if ui.button("Steal").clicked() {
                    self.start_steal();
                }
                if ui.button("Compose").clicked() {
                    self.compose = ComposeState {
                        to: String::new(),
                        cc: String::new(),
                        subject: String::new(),
                        body: String::new(),
                        reply_to: None,
                    };
                    self.panel = Panel::Compose;
                }
                if ui.button("Add account").clicked() {
                    self.panel = Panel::AddAccount;
                }
                if !self.account.is_empty() && ui.button("Edit").clicked() {
                    if let Ok(f) = ops::accounts() {
                        if let Ok(a) = f.account(&self.account) {
                            self.edit = EditState {
                                old_name: a.name.clone(),
                                name: a.name.clone(),
                                from: a.from.clone(),
                                email: a.imap.user.clone(),
                                password: String::new(),
                                in_host: a.imap.host.clone(),
                                in_port: a.imap.port.to_string(),
                                out_host: a.smtp.host.clone(),
                                out_port: a.smtp.port.to_string(),
                            };
                            self.panel = Panel::EditAccount;
                        }
                    }
                }
                if !self.account.is_empty() && ui.button("Remove").clicked() {
                    self.confirm_remove = Some(self.account.clone());
                }
                if self.busy {
                    ui.spinner();
                }
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(RichText::new(&self.status).color(SILVER));
        });

        if let Some(name) = self.confirm_remove.clone() {
            egui::Window::new("Dismiss goblin?")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Send {name} back to the dark?"));
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.confirm_remove = None;
                        }
                        if ui.button("Dismiss").clicked() {
                            match ops::remove_account(&name) {
                                Ok(_) => {
                                    self.status = format!("dismissed {name}");
                                    self.confirm_remove = None;
                                    self.reload_accounts();
                                    self.reload_mails();
                                    if self.accounts.is_empty() {
                                        self.panel = Panel::AddAccount;
                                    }
                                }
                                Err(e) => self.status = format!("{e}"),
                            }
                        }
                    });
                });
        }

        match self.panel {
            Panel::Mail => self.ui_mail(ctx),
            Panel::Compose => self.ui_compose(ctx),
            Panel::AddAccount => self.ui_add(ctx),
            Panel::EditAccount => self.ui_edit(ctx),
        }
    }
}

impl GoblinApp {
    fn ui_mail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("folders")
            .default_width(120.0)
            .show(ctx, |ui| {
                ui.heading("Piles");
                for (label, b) in [
                    ("Unread", MailBox::Unread),
                    ("Read", MailBox::Read),
                    ("Trash", MailBox::Trash),
                ] {
                    if ui.selectable_label(self.box_name == b, label).clicked() {
                        self.box_name = b;
                        self.sel = None;
                        self.reload_mails();
                    }
                }
                ui.separator();
                ui.label("Hunt");
                if ui.text_edit_singleline(&mut self.query).changed() {
                    self.apply_filter();
                    self.sel = None;
                }
            });

        egui::SidePanel::left("list")
            .default_width(280.0)
            .show(ctx, |ui| {
                ui.heading(self.box_name.as_str());
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (fi, &mi) in self.filtered.iter().enumerate() {
                        let m = &self.mails[mi];
                        let selected = self.sel == Some(fi);
                        let label = format!("{}\n{}", trunc(&m.from, 40), trunc(&m.subject, 48));
                        if ui
                            .add(egui::SelectableLabel::new(selected, label))
                            .clicked()
                        {
                            self.sel = Some(fi);
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(mail) = self.selected().cloned() else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("pick a letter").color(MUTED));
                });
                return;
            };
            ui.horizontal(|ui| {
                ui.heading(RichText::new(&mail.subject).color(PINK));
                if ui.button("Reply").clicked() {
                    let quote = crate::compose::quote_body(&mail);
                    self.compose = ComposeState {
                        to: extract_addr(&mail.from),
                        cc: String::new(),
                        subject: re_subject(&mail.subject),
                        body: format!("\n\n{quote}"),
                        reply_to: Some(mail.clone()),
                    };
                    self.panel = Panel::Compose;
                }
            });
            ui.label(RichText::new(format!("From: {}", mail.from)).color(SILVER));
            ui.label(RichText::new(format!("To: {}", mail.to)).color(MUTED));
            ui.label(RichText::new(format!("Date: {}", mail.date)).color(MUTED));
            if !mail.attachments.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("Parcels:");
                    for name in &mail.attachments {
                        ui.label(RichText::new(name).color(BLUSH));
                        if let Ok(paths) = ops::store().list_attachments(&mail.uid) {
                            if let Some(p) = paths.iter().find(|p| {
                                p.file_name()
                                    .map(|f| f.to_string_lossy() == name.as_str())
                                    .unwrap_or(false)
                            }) {
                                if ui.small_button("Open").clicked() {
                                    if let Err(e) = ops::open_path(p) {
                                        self.status = format!("{e}");
                                    }
                                }
                            }
                        }
                    }
                });
            }
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(egui::Label::new(&mail.body).wrap());
            });
        });
    }

    fn ui_compose(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Compose");
            ui.horizontal(|ui| {
                ui.label("To");
                ui.text_edit_singleline(&mut self.compose.to);
            });
            ui.horizontal(|ui| {
                ui.label("Cc");
                ui.text_edit_singleline(&mut self.compose.cc);
            });
            ui.horizontal(|ui| {
                ui.label("Subject");
                ui.text_edit_singleline(&mut self.compose.subject);
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.compose.body)
                    .desired_width(f32::INFINITY)
                    .desired_rows(16),
            );
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    self.panel = Panel::Mail;
                }
                if ui.button("Send").clicked() {
                    self.start_send();
                }
            });
        });
    }

    fn ui_add(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Summon a goblin");
            let presets: Vec<&str> = config::NEST_PRESETS.iter().map(|p| p.id).collect();
            let other = presets.len();
            egui::ComboBox::from_label("Sky")
                .selected_text(if self.add.preset < presets.len() {
                    presets[self.add.preset]
                } else {
                    "other"
                })
                .show_ui(ui, |ui| {
                    for (i, id) in presets.iter().enumerate() {
                        if ui.selectable_label(self.add.preset == i, *id).clicked() {
                            self.add.preset = i;
                        }
                    }
                    if ui
                        .selectable_label(self.add.preset == other, "other")
                        .clicked()
                    {
                        self.add.preset = other;
                    }
                });
            labeled(ui, "Name", &mut self.add.name);
            labeled(ui, "From", &mut self.add.from);
            labeled(ui, "Email / user", &mut self.add.email);
            ui.horizontal(|ui| {
                ui.label("Password");
                ui.add(egui::TextEdit::singleline(&mut self.add.password).password(true));
            });
            if self.add.preset >= presets.len() {
                labeled(ui, "IMAP host", &mut self.add.in_host);
                labeled(ui, "IMAP port", &mut self.add.in_port);
                labeled(ui, "SMTP host", &mut self.add.out_host);
                labeled(ui, "SMTP port", &mut self.add.out_port);
            }
            ui.horizontal(|ui| {
                if !self.accounts.is_empty() && ui.button("Cancel").clicked() {
                    self.panel = Panel::Mail;
                }
                if ui.button("Summon").clicked() {
                    match self.commit_add() {
                        Ok(()) => {
                            self.panel = Panel::Mail;
                            self.reload_accounts();
                            self.reload_mails();
                            self.status = format!("woke {}", self.add.name);
                        }
                        Err(e) => self.status = format!("{e}"),
                    }
                }
            });
        });
    }

    fn ui_edit(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!("Mend {}", self.edit.old_name));
            labeled(ui, "Name", &mut self.edit.name);
            labeled(ui, "From", &mut self.edit.from);
            labeled(ui, "Email / user", &mut self.edit.email);
            ui.horizontal(|ui| {
                ui.label("Password (blank = keep)");
                ui.add(egui::TextEdit::singleline(&mut self.edit.password).password(true));
            });
            labeled(ui, "IMAP host", &mut self.edit.in_host);
            labeled(ui, "IMAP port", &mut self.edit.in_port);
            labeled(ui, "SMTP host", &mut self.edit.out_host);
            labeled(ui, "SMTP port", &mut self.edit.out_port);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    self.panel = Panel::Mail;
                }
                if ui.button("Save").clicked() {
                    match self.commit_edit() {
                        Ok(()) => {
                            self.panel = Panel::Mail;
                            self.reload_accounts();
                            self.status = "mended".into();
                        }
                        Err(e) => self.status = format!("{e}"),
                    }
                }
            });
        });
    }

    fn commit_add(&self) -> Result<(), Error> {
        let email = self.add.email.trim();
        let name = self.add.name.trim();
        if name.is_empty() || email.is_empty() || self.add.password.is_empty() {
            return Err(Error::Usage("name, email, and password required".into()));
        }
        let from = if self.add.from.trim().is_empty() {
            email.to_string()
        } else {
            self.add.from.trim().to_string()
        };
        let acc = if self.add.preset < config::NEST_PRESETS.len() {
            let id = config::NEST_PRESETS[self.add.preset].id;
            config::apply_preset(id, name, &from, email)?
        } else {
            Account {
                name: name.into(),
                from,
                imap: Endpoint {
                    host: self.add.in_host.trim().into(),
                    port: self
                        .add
                        .in_port
                        .trim()
                        .parse()
                        .map_err(|_| Error::Usage("bad IMAP port".into()))?,
                    user: email.into(),
                },
                smtp: Endpoint {
                    host: self.add.out_host.trim().into(),
                    port: self
                        .add
                        .out_port
                        .trim()
                        .parse()
                        .map_err(|_| Error::Usage("bad SMTP port".into()))?,
                    user: email.into(),
                },
            }
        };
        crate::tls::imap_mode(acc.imap.port)?;
        crate::tls::smtp_mode(acc.smtp.port)?;
        ops::save_account(acc, &self.add.password, true)
    }

    fn commit_edit(&self) -> Result<(), Error> {
        let email = self.edit.email.trim();
        let name = self.edit.name.trim();
        if name.is_empty() || email.is_empty() {
            return Err(Error::Usage("name and email required".into()));
        }
        let acc = Account {
            name: name.into(),
            from: self.edit.from.trim().into(),
            imap: Endpoint {
                host: self.edit.in_host.trim().into(),
                port: self
                    .edit
                    .in_port
                    .trim()
                    .parse()
                    .map_err(|_| Error::Usage("bad IMAP port".into()))?,
                user: email.into(),
            },
            smtp: Endpoint {
                host: self.edit.out_host.trim().into(),
                port: self
                    .edit
                    .out_port
                    .trim()
                    .parse()
                    .map_err(|_| Error::Usage("bad SMTP port".into()))?,
                user: email.into(),
            },
        };
        crate::tls::imap_mode(acc.imap.port)?;
        crate::tls::smtp_mode(acc.smtp.port)?;
        let pw = self.edit.password.trim();
        ops::replace_account(
            &self.edit.old_name,
            acc,
            if pw.is_empty() { None } else { Some(pw) },
        )
    }
}

const PINK: Color32 = Color32::from_rgb(219, 112, 147);
const BLUSH: Color32 = Color32::from_rgb(255, 192, 203);
const SILVER: Color32 = Color32::from_rgb(220, 220, 220);
const MUTED: Color32 = Color32::from_rgb(140, 140, 140);

fn labeled(ui: &mut Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
    });
}

fn trunc(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

fn extract_addr(from: &str) -> String {
    if let (Some(a), Some(b)) = (from.find('<'), from.find('>')) {
        if b > a {
            return from[a + 1..b].trim().to_string();
        }
    }
    from.trim().to_string()
}

fn re_subject(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 3 && t[..3].eq_ignore_ascii_case("re:") {
        t.to_string()
    } else {
        format!("Re: {t}")
    }
}

pub fn run_gui() -> eframe::Result {
    crate::tls::install_crypto();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_title("Goblin"),
        ..Default::default()
    };
    eframe::run_native(
        "Goblin",
        opts,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.override_text_color = Some(SILVER);
            visuals.widgets.noninteractive.fg_stroke.color = SILVER;
            visuals.selection.bg_fill = Color32::from_rgb(90, 40, 70);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(GoblinApp::default()))
        }),
    )
}
