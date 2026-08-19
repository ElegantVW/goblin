use crate::compose;
use crate::config::{self, Account, AccountFile, Endpoint};
use crate::error::Error;
use crate::imap::{self, SyncOpts};
use crate::import_aerc;
use crate::notify;
use crate::paths;
use crate::smtp;
use crate::store::{MailBox, MailMeta, Store};
use crate::{secrets, AccountCmd, AttachCmd, Cmd};
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

pub fn dispatch(cmd: Cmd) -> Result<u8, Error> {
    match cmd {
        Cmd::Summon { preset } => cmd_account_add(preset.as_deref()),
        Cmd::Who => cmd_account_show(),
        Cmd::Wake { name } => cmd_account_use(&name),
        Cmd::Dismiss { name } => cmd_account_remove(name.as_deref()),
        Cmd::Mend { name } => crate::tui::run_mend(name),
        Cmd::Nest { action } => match action {
            AccountCmd::Add { preset } => cmd_account_add(preset.as_deref()),
            AccountCmd::Show => cmd_account_show(),
            AccountCmd::Use { name } => cmd_account_use(&name),
            AccountCmd::Remove { name } => cmd_account_remove(name.as_deref()),
        },
        Cmd::ImportAerc { file } => cmd_import_aerc(file),
        Cmd::Steal {
            quiet,
            no_notify,
            all,
            force,
            folder,
            limit,
            account,
        } => cmd_sync(quiet, no_notify, all, force, folder, limit, account),
        Cmd::Watch => cmd_idle(),
        Cmd::Peek { box_name, plain } => cmd_list(&box_name, plain),
        Cmd::Read { file, plain } => cmd_show(&file, plain),
        Cmd::Pile { snippet, limit } => cmd_bundle(snippet, limit),
        Cmd::Keep {
            files,
            all,
            local_only,
        } => cmd_move("read", files, all, local_only),
        Cmd::Trash {
            files,
            all,
            local_only,
        } => cmd_move("trash", files, all, local_only),
        Cmd::Move {
            dest,
            files,
            all,
            local_only,
        } => cmd_move(&dest, files, all, local_only),
        Cmd::Send {
            to,
            subject,
            cc,
            body_file,
        } => cmd_send(&to, &subject, cc.as_deref(), body_file.as_deref()),
        Cmd::Squeak { set } => notify::cmd_sound(set.as_deref()),
        Cmd::Hunt { query, plain } => cmd_search(&query.join(" "), plain),
        Cmd::Parcel { action } => cmd_attach(action),
    }
}

pub fn load_accounts_file() -> Result<config::AccountFile, Error> {
    let path = paths::accounts_file();
    if !path.exists() {
        return Err(Error::say("no goblins yet", "goblin summon"));
    }
    config::load_accounts(&path)
}

pub fn load_default_account() -> Result<(Account, String), Error> {
    let file = load_accounts_file()?;
    let acc = file.default_account()?.clone();
    let password = secrets::load_password(&secret_id(&acc))?;
    Ok((acc, password))
}

pub fn load_named_account(name: &str) -> Result<(Account, String), Error> {
    let file = load_accounts_file()?;
    let acc = file.account(name)?.clone();
    let password = secrets::load_password(&secret_id(&acc))?;
    Ok((acc, password))
}

pub fn secret_id(acc: &Account) -> String {
    acc.imap.user.clone()
}

pub fn store() -> Store {
    Store::default_store()
}

fn cmd_account_show() -> Result<u8, Error> {
    let path = paths::accounts_file();
    if !path.exists() {
        return Err(Error::say("no goblins yet", "goblin summon"));
    }
    let file = config::load_accounts(&path)?;
    for acc in &file.accounts {
        let mark = if acc.name == file.default { "*" } else { " " };
        println!("{mark} {:<12}  {}", acc.name, acc.from);
    }
    Ok(0)
}

pub fn remove_account(name: &str) -> Result<String, Error> {
    let path = writable_accounts_path();
    if !path.exists() {
        return Err(Error::say("no goblins yet", "goblin summon"));
    }
    let mut file = config::load_accounts(&path)?;
    let acc = file.remove(name)?;
    let secret = secret_id(&acc);
    let _ = secrets::delete_password(&secret);
    if file.accounts.is_empty() {
        let _ = std::fs::remove_file(&path);
    } else {
        config::save_accounts(&path, &file)?;
    }
    Ok(acc.name)
}

fn cmd_account_remove(name: Option<&str>) -> Result<u8, Error> {
    let file = load_accounts_file()?;
    let name = match name {
        Some(n) => n.to_string(),
        None => file.default.clone(),
    };
    if name.is_empty() {
        return Err(Error::say("no goblins yet", "goblin summon"));
    }
    let gone = remove_account(&name)?;
    println!("{gone} returned to the dark");
    Ok(0)
}

fn cmd_account_use(name: &str) -> Result<u8, Error> {
    let path = writable_accounts_path();
    let mut file = config::load_accounts(&path)?;
    file.set_default(name)?;
    config::save_accounts(&path, &file)?;
    println!("default account → {name}");
    Ok(0)
}

fn cmd_account_add(preset: Option<&str>) -> Result<u8, Error> {
    if !stdin_is_tty() {
        return Err(Error::say(
            "summon needs a real terminal",
            "run: goblin summon",
        ));
    }
    let preset = match preset {
        Some(id) => Some(id.to_string()),
        None => Some(pick_nest()?),
    };
    let preset = preset.filter(|s| s != "other");

    let name = prompt("goblin name", Some("work"))?;
    let from = prompt("your name and email (Ada <ada@example.com>)", None)?;
    let user = prompt("email address", extract_addr(&from).as_deref())?;

    let acc = if let Some(id) = preset.as_deref() {
        config::apply_preset(id, &name, &from, &user)?
    } else {
        let imap_host = prompt("incoming sky (host)", None)?;
        let imap_port = prompt("incoming port", Some("993"))?
            .parse::<u16>()
            .map_err(|_| Error::say("bad incoming port", "use 993"))?;
        let smtp_host = prompt("outgoing sky (host)", None)?;
        let smtp_port = prompt("outgoing port", Some("465"))?
            .parse::<u16>()
            .map_err(|_| Error::say("bad outgoing port", "use 465 or 587"))?;
        Account {
            name: name.clone(),
            from,
            imap: Endpoint {
                host: imap_host,
                port: imap_port,
                user: user.clone(),
            },
            smtp: Endpoint {
                host: smtp_host,
                port: smtp_port,
                user: user.clone(),
            },
        }
    };
    crate::tls::imap_mode(acc.imap.port)?;
    crate::tls::smtp_mode(acc.smtp.port)?;

    let pw_hint = preset
        .as_deref()
        .and_then(config::find_preset)
        .filter(|p| p.wants_app_password)
        .map(|_| "password (app password if your mail house asks for one): ")
        .unwrap_or("password: ");
    let password = read_password(pw_hint)?;
    if password.is_empty() {
        return Err(Error::say("empty password", "try goblin summon again"));
    }
    let n = acc.name.clone();
    save_account(acc, &password, file_is_empty())?;
    println!("woke {n}");
    Ok(0)
}

fn file_is_empty() -> bool {
    let path = writable_accounts_path();
    !path.exists()
        || config::load_accounts(&path)
            .map(|f| f.accounts.is_empty())
            .unwrap_or(true)
}

fn pick_nest() -> Result<String, Error> {
    eprintln!("which sky?");
    for (i, p) in config::NEST_PRESETS.iter().enumerate() {
        eprintln!("  {}  {}", i + 1, p.id);
    }
    eprintln!("  {}  other", config::NEST_PRESETS.len() + 1);
    let raw = prompt("pick", Some("1"))?;
    if let Ok(n) = raw.parse::<usize>() {
        if n >= 1 && n <= config::NEST_PRESETS.len() {
            return Ok(config::NEST_PRESETS[n - 1].id.to_string());
        }
        if n == config::NEST_PRESETS.len() + 1 {
            return Ok("other".into());
        }
    }
    if raw == "other" || config::find_preset(&raw).is_some() {
        return Ok(raw);
    }
    Err(Error::say(
        "unknown sky",
        "pick 1-5, or: purelymail, google, disroot, outlook, yahoo",
    ))
}

/// Rewrite a goblin. `password` None = keep the old secret (move it if the email changed).
pub fn replace_account(old_name: &str, acc: Account, password: Option<&str>) -> Result<u8, Error> {
    let path = writable_accounts_path();
    if !path.exists() {
        return Err(Error::say("no goblins yet", "goblin summon"));
    }
    let mut file = config::load_accounts(&path)?;
    let old = file
        .accounts
        .iter()
        .find(|a| a.name == old_name)
        .cloned()
        .ok_or_else(|| Error::say(format!("no goblin named {old_name:?}"), "goblin who"))?;
    let was_default = file.default == old_name;
    let old_secret = secret_id(&old);
    let _ = file.remove(old_name);
    if file.accounts.iter().any(|a| a.name == acc.name) && acc.name != old_name {
        return Err(Error::say(
            format!("a goblin named {:?} already watches", acc.name),
            "pick another name",
        ));
    }
    file.upsert(acc.clone());
    if was_default || file.default.is_empty() {
        file.default = acc.name.clone();
    }
    config::save_accounts(&path, &file)?;
    let new_secret = secret_id(&acc);
    match password {
        Some(p) if !p.is_empty() => {
            secrets::store_password(&new_secret, p)?;
            if new_secret != old_secret {
                let _ = secrets::delete_password(&old_secret);
            }
        }
        _ => {
            if new_secret != old_secret {
                if let Ok(old_pw) = secrets::load_password(&old_secret) {
                    secrets::store_password(&new_secret, &old_pw)?;
                    let _ = secrets::delete_password(&old_secret);
                }
            }
        }
    }
    Ok(0)
}

pub fn save_account(acc: Account, password: &str, make_default: bool) -> Result<u8, Error> {
    let path = writable_accounts_path();
    let mut file = if path.exists() {
        config::load_accounts(&path).unwrap_or(AccountFile {
            default: acc.name.clone(),
            accounts: Vec::new(),
        })
    } else {
        AccountFile {
            default: acc.name.clone(),
            accounts: Vec::new(),
        }
    };
    file.upsert(acc.clone());
    if make_default || file.default.is_empty() {
        file.default = acc.name.clone();
    }
    config::save_accounts(&path, &file)?;
    secrets::store_password(&secret_id(&acc), password)?;
    Ok(0)
}

fn writable_accounts_path() -> PathBuf {
    let path = paths::accounts_file();
    if path.extension().and_then(|s| s.to_str()) == Some("gpg") {
        paths::config_dir().join("accounts.json")
    } else {
        path
    }
}

fn cmd_import_aerc(file: Option<PathBuf>) -> Result<u8, Error> {
    let path = file.unwrap_or_else(|| {
        directories::BaseDirs::new()
            .map(|b| b.config_dir().join("aerc").join("accounts.conf"))
            .unwrap_or_else(|| PathBuf::from("/nonexistent"))
    });
    if !path.is_file() {
        return Err(Error::Usage(format!(
            "no aerc config at {}",
            path.display()
        )));
    }
    let imported = import_aerc::import_aerc(&path)?;
    let acc = imported.account;
    println!(
        "imported {}  imap {}@{}:{}  smtp {}:{}",
        acc.name, acc.imap.user, acc.imap.host, acc.imap.port, acc.smtp.host, acc.smtp.port
    );
    crate::tls::imap_mode(acc.imap.port)?;
    crate::tls::smtp_mode(acc.smtp.port)?;
    let mut password = imported.password;
    if password.is_empty() {
        password = read_password("password: ")?;
    }
    save_account(acc, &password, file_is_empty())?;
    println!("imported goblin");
    eprintln!("note: password stored in the keyring/secrets file, not in accounts.json");
    Ok(0)
}

fn cmd_sync(
    quiet: bool,
    no_notify: bool,
    all: bool,
    force: bool,
    folder: Option<String>,
    limit: usize,
    account: Option<String>,
) -> Result<u8, Error> {
    let (acc, password) = match account {
        Some(name) => load_named_account(&name)?,
        None => load_default_account()?,
    };
    if !quiet {
        eprintln!(
            "stealing {} for {} …",
            if all { "recent letters" } else { "new letters" },
            acc.imap.user
        );
    }
    let store = store();
    let result = imap::sync(
        &acc,
        &password,
        &store,
        SyncOpts {
            all,
            force,
            limit,
            folder,
        },
    )?;
    if !quiet {
        println!(
            "stole {} letter(s) → {}",
            result.written,
            store.box_dir(MailBox::Unread).display()
        );
        if result.skipped > 0 {
            println!("(skipped {} already stored)", result.skipped);
        }
    }
    write_state(result.written)?;
    if result.written > 0 && !no_notify && !notify::play() {
        eprintln!(
            "(new mail! goblin wants to squeak — drop a sound at ~/.config/goblin/notify.mp3)"
        );
    }
    Ok(0)
}

fn write_state(last_new: usize) -> Result<(), Error> {
    let path = paths::state_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let v = serde_json::json!({
        "last_sync": now,
        "last_new": last_new,
    });
    std::fs::write(path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
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

fn cmd_idle() -> Result<u8, Error> {
    let (acc, password) = load_default_account()?;
    crate::tls::imap_mode(acc.imap.port)?;
    eprintln!(
        "idle: watching INBOX on {} — instant push on new mail",
        acc.imap.host
    );
    let mut backoff = 5u64;
    loop {
        match imap::idle_once(&acc, &password) {
            Ok(()) => {
                backoff = 5;
                let _ = cmd_sync(false, false, false, false, None, 30, None);
            }
            Err(e) => {
                eprintln!("idle: {e} — reconnecting in {backoff}s");
                std::thread::sleep(std::time::Duration::from_secs(backoff));
                backoff = (backoff * 2).min(60);
            }
        }
    }
}

fn cmd_list(box_name: &str, plain: bool) -> Result<u8, Error> {
    let store = store();
    let box_name = MailBox::parse(box_name)?;
    let mails = store.load_mails(box_name)?;
    if mails.is_empty() {
        println!("(no messages in {})", box_name.as_str());
        return Ok(0);
    }
    if plain {
        for m in &mails {
            println!(
                "{}\tuid={}\tfrom={}\tsubject={}",
                m.name(),
                m.uid,
                trunc(&m.from, 40),
                trunc(&m.subject, 50)
            );
        }
        return Ok(0);
    }
    println!(
        "Goblin · {} in {} · last sync {}",
        mails.len(),
        box_name.as_str(),
        last_sync()
    );
    for (i, m) in mails.iter().enumerate() {
        println!(
            "[{:>2}] {:<20} │ {}  {}",
            i + 1,
            trunc(&from_short(&m.from), 20),
            m.subject,
            trunc(&m.date, 17)
        );
    }
    Ok(0)
}

fn cmd_show(file: &str, plain: bool) -> Result<u8, Error> {
    let store = store();
    let path = resolve_mail(&store, file)?;
    let m = store.parse_path(&path)?;
    if plain {
        print!("{}", std::fs::read_to_string(&path)?);
        return Ok(0);
    }
    println!("From:    {}", m.from);
    println!("Date:    {}", m.date);
    println!("Subject: {}", m.subject);
    println!();
    println!("{}", m.body);
    Ok(0)
}

fn cmd_bundle(snippet: usize, limit: usize) -> Result<u8, Error> {
    let store = store();
    let mut mails = store.load_mails(MailBox::Unread)?;
    if mails.is_empty() {
        println!("(no unread messages on disk — run sync first)");
        return Ok(0);
    }
    let snippet_n = snippet.clamp(80, 1200);
    let max = limit.clamp(1, 50);
    mails.truncate(max);
    println!(
        "UNREAD DIGEST — {} message(s) (use filename for mail_move)\n",
        mails.len()
    );
    for (i, m) in mails.iter().enumerate() {
        let mut body: String = m.body.split_whitespace().collect::<Vec<_>>().join(" ");
        if body.len() > snippet_n {
            body.truncate(snippet_n);
            body.push('…');
        }
        println!(
            "{}. file={}\n   From: {}\n   Date: {}\n   Subject: {}\n   Snippet: {}\n",
            i + 1,
            m.name(),
            m.from,
            m.date,
            m.subject,
            body
        );
    }
    Ok(0)
}

fn cmd_move(dest: &str, files: Vec<String>, all: bool, local_only: bool) -> Result<u8, Error> {
    let dest = match dest {
        "read" => MailBox::Read,
        "trash" => MailBox::Trash,
        other => {
            return Err(Error::Usage(format!(
                "dest must be read|trash, not {other:?}"
            )));
        }
    };
    let store = store();
    let mut targets = files;
    if all {
        targets = store
            .load_mails(MailBox::Unread)?
            .into_iter()
            .map(|m| m.name())
            .collect();
    }
    if targets.is_empty() {
        println!("nothing to move");
        return Ok(0);
    }
    let acc_pass = if local_only {
        None
    } else {
        Some(load_default_account()?)
    };
    for name in targets {
        let path = resolve_mail(&store, &name)?;
        let m = store.parse_path(&path)?;
        let mut server_msg = String::from("local only");
        if let Some((ref acc, ref password)) = acc_pass {
            if !m.uid.is_empty() {
                server_msg = match dest {
                    MailBox::Read => {
                        imap::mark_seen(acc, password, &m.uid)?;
                        format!("uid {} marked Seen on server", m.uid)
                    }
                    MailBox::Trash => {
                        let folder = imap::trash(acc, password, &m.uid)?;
                        format!("uid {} moved to {folder}", m.uid)
                    }
                    MailBox::Unread => unreachable!(),
                };
            }
        }
        store.move_mail(&path, dest)?;
        println!(
            "moved {} → {}/  ({server_msg})",
            path.file_name().unwrap().to_string_lossy(),
            dest.as_str()
        );
    }
    Ok(0)
}

fn cmd_send(
    to: &str,
    subject: &str,
    cc: Option<&str>,
    body_file: Option<&Path>,
) -> Result<u8, Error> {
    let (acc, password) = load_default_account()?;
    crate::tls::smtp_mode(acc.smtp.port)?;
    let body = read_body(body_file)?;
    let ccs: Vec<String> = cc
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut rcpts = vec![to.to_string()];
    rcpts.extend(ccs.iter().cloned());
    let raw = compose::compose(&acc.from, to, &ccs, subject, &body);
    smtp::send(&acc, &password, &rcpts, &raw)?;
    let meta = MailMeta {
        uid: format!("sent-{}", chrono::Utc::now().timestamp()),
        account: acc.name.clone(),
        folder: "SENT".into(),
        from: acc.from.clone(),
        to: to.into(),
        date: chrono::Utc::now().to_rfc2822(),
        subject: subject.into(),
        message_id: String::new(),
        attachments: Vec::new(),
        path: None,
        body: String::new(),
    };
    let store = store();
    store.write_mail(MailBox::Read, &meta, &body)?;
    println!("sent to {to}");
    Ok(0)
}

fn read_body(path: Option<&Path>) -> Result<String, Error> {
    match path {
        None => {
            if stdin_is_tty() {
                return Err(Error::Usage(
                    "pass --body-file PATH or --body-file - for stdin".into(),
                ));
            }
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
        Some(p) if p.as_os_str() == "-" => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
        Some(p) => Ok(std::fs::read_to_string(p)?),
    }
}

fn cmd_search(query: &str, plain: bool) -> Result<u8, Error> {
    if query.trim().is_empty() {
        return Err(Error::Usage("goblin search <query>".into()));
    }
    let store = store();
    let hits = crate::search::search_store(&store, query)?;
    if hits.is_empty() {
        println!("(no matches for {query:?})");
        return Ok(0);
    }
    if plain {
        for (b, m) in &hits {
            println!(
                "{}\t{}\tuid={}\tfrom={}\tsubject={}",
                b.as_str(),
                m.name(),
                m.uid,
                trunc(&m.from, 40),
                trunc(&m.subject, 50)
            );
        }
        return Ok(0);
    }
    println!("Goblin · {} match(es) for {query:?}", hits.len());
    for (i, (b, m)) in hits.iter().enumerate() {
        println!(
            "[{:>2}] {:<6}  {:<18} │ {}",
            i + 1,
            b.as_str(),
            trunc(&from_short(&m.from), 18),
            m.subject
        );
    }
    Ok(0)
}

fn cmd_attach(action: AttachCmd) -> Result<u8, Error> {
    let store = store();
    match action {
        AttachCmd::List { mail } => {
            let m = store.parse_path(&resolve_mail(&store, &mail)?)?;
            let files = store.list_attachments(&m.uid)?;
            if files.is_empty() && m.attachments.is_empty() {
                println!("(no attachments on {})", m.name());
                return Ok(0);
            }
            if files.is_empty() {
                for (i, name) in m.attachments.iter().enumerate() {
                    println!(
                        "[{:>2}] {name}  (not on disk — re-sync with --force)",
                        i + 1
                    );
                }
                return Ok(0);
            }
            for (i, p) in files.iter().enumerate() {
                let sz = fs_size(p);
                println!(
                    "[{:>2}] {}  ({sz} bytes)",
                    i + 1,
                    p.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            Ok(0)
        }
        AttachCmd::Save { mail, index, dest } => {
            let m = store.parse_path(&resolve_mail(&store, &mail)?)?;
            let src = pick_attachment(&store, &m.uid, index)?;
            let dest = match dest {
                Some(d) if d.is_dir() => d.join(src.file_name().unwrap()),
                Some(d) => d,
                None => PathBuf::from(src.file_name().unwrap()),
            };
            std::fs::copy(&src, &dest)?;
            println!("saved {}", dest.display());
            Ok(0)
        }
        AttachCmd::Open { mail, index } => {
            let m = store.parse_path(&resolve_mail(&store, &mail)?)?;
            let src = pick_attachment(&store, &m.uid, index)?;
            open_path(&src)?;
            println!("opened {}", src.display());
            Ok(0)
        }
    }
}

fn pick_attachment(store: &Store, uid: &str, index: usize) -> Result<PathBuf, Error> {
    let files = store.list_attachments(uid)?;
    if index == 0 || index > files.len() {
        return Err(Error::Usage(format!(
            "attachment {index} out of range (1-{})",
            files.len()
        )));
    }
    Ok(files[index - 1].clone())
}

fn fs_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

pub fn open_path(path: &Path) -> Result<(), Error> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    } else if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", &path.to_string_lossy()]);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| Error::Usage(format!("open {}: {e}", path.display())))?;
    Ok(())
}

pub fn resolve_mail(store: &Store, spec: &str) -> Result<PathBuf, Error> {
    let p = PathBuf::from(spec);
    if p.is_file() {
        return Ok(p);
    }
    let unread = store.box_dir(MailBox::Unread).join(spec);
    if unread.is_file() {
        return Ok(unread);
    }
    for b in MailBox::all() {
        let cand = store.box_dir(b).join(spec);
        if cand.is_file() {
            return Ok(cand);
        }
    }
    if let Ok(n) = spec.parse::<usize>() {
        let mails = store.load_mails(MailBox::Unread)?;
        if n >= 1 && n <= mails.len() {
            if let Some(path) = &mails[n - 1].path {
                return Ok(path.clone());
            }
        }
    }
    Err(Error::Usage(format!("not found: {spec}")))
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
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect()
    }
}

fn extract_addr(from: &str) -> Option<String> {
    let start = from.find('<')?;
    let end = from.find('>')?;
    if end > start {
        Some(from[start + 1..end].trim().to_string())
    } else {
        None
    }
}

fn prompt(label: &str, default: Option<&str>) -> Result<String, Error> {
    if let Some(d) = default {
        eprint!("{label} [{d}]: ");
    } else {
        eprint!("{label}: ");
    }
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let t = line.trim();
    if t.is_empty() {
        if let Some(d) = default {
            return Ok(d.to_string());
        }
        return Err(Error::Usage(format!("{label} is required")));
    }
    Ok(t.to_string())
}

fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

fn read_password(prompt: &str) -> Result<String, Error> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    if !stdin_is_tty() {
        return Err(Error::Usage("password prompt needs a tty".into()));
    }
    let line = crate::termart::read_secret_line().map_err(|e| {
        if e.kind() == io::ErrorKind::Unsupported {
            Error::Usage("password prompt needs a tty".into())
        } else {
            e.into()
        }
    })?;
    eprintln!();
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::purelymail_preset;

    #[test]
    fn replace_account_rename_keeps_secret_when_password_none() {
        let root = tempfile::tempdir().unwrap();
        crate::paths::with_goblin_home(Some(root.path()), || {
            let acc = purelymail_preset("work", "Ada <ada@x>", "ada@x");
            save_account(acc.clone(), "hunter2", true).unwrap();
            let renamed = purelymail_preset("home", "Ada <ada@x>", "ada@x");
            replace_account("work", renamed.clone(), None).unwrap();
            let file = config::load_accounts(&paths::accounts_file()).unwrap();
            assert_eq!(file.default, "home");
            assert!(file.account("work").is_err());
            assert_eq!(
                secrets::load_password(&secret_id(&renamed)).unwrap(),
                "hunter2"
            );
        });
    }

    #[test]
    fn replace_account_email_change_moves_secret() {
        let root = tempfile::tempdir().unwrap();
        crate::paths::with_goblin_home(Some(root.path()), || {
            let acc = purelymail_preset("work", "Ada <ada@x>", "ada@x");
            save_account(acc.clone(), "hunter2", true).unwrap();
            let updated = purelymail_preset("work", "Ada <ada@y>", "ada@y");
            replace_account("work", updated.clone(), None).unwrap();
            assert!(secrets::load_password(&secret_id(&acc)).is_err());
            assert_eq!(
                secrets::load_password(&secret_id(&updated)).unwrap(),
                "hunter2"
            );
        });
    }

    #[test]
    fn replace_account_name_collision_errors() {
        let root = tempfile::tempdir().unwrap();
        crate::paths::with_goblin_home(Some(root.path()), || {
            let work = purelymail_preset("work", "Ada <ada@x>", "ada@x");
            let home = purelymail_preset("home", "Ada <ada@y>", "ada@y");
            save_account(work, "hunter2", true).unwrap();
            save_account(home, "hunter3", false).unwrap();
            let collide = purelymail_preset("home", "Ada <ada@x>", "ada@x");
            match replace_account("work", collide, None) {
                Err(Error::Hint { msg, .. }) => {
                    assert!(msg.contains("already watches"), "{msg}");
                }
                other => panic!("{other:?}"),
            }
            let file = config::load_accounts(&paths::accounts_file()).unwrap();
            assert!(file.account("work").is_ok());
            assert!(file.account("home").is_ok());
        });
    }

    #[test]
    fn replace_account_missing_goblin_errors() {
        let root = tempfile::tempdir().unwrap();
        crate::paths::with_goblin_home(Some(root.path()), || {
            let acc = purelymail_preset("work", "Ada <ada@x>", "ada@x");
            match replace_account("work", acc.clone(), None) {
                Err(Error::Hint { msg, .. }) => assert!(msg.contains("no goblins yet"), "{msg}"),
                other => panic!("{other:?}"),
            }
            save_account(acc, "hunter2", true).unwrap();
            let ghost = purelymail_preset("ghost", "Ada <ada@x>", "ada@x");
            match replace_account("ghost", ghost, None) {
                Err(Error::Hint { msg, .. }) => assert!(msg.contains("no goblin named"), "{msg}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn replace_account_new_password_overwrites_secret() {
        let root = tempfile::tempdir().unwrap();
        crate::paths::with_goblin_home(Some(root.path()), || {
            let acc = purelymail_preset("work", "Ada <ada@x>", "ada@x");
            save_account(acc.clone(), "hunter2", true).unwrap();
            replace_account("work", acc.clone(), Some("s3cret")).unwrap();
            assert_eq!(secrets::load_password(&secret_id(&acc)).unwrap(), "s3cret");
        });
    }

    #[test]
    fn bundle_and_plain_contain_no_password() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let acc = purelymail_preset("work", "Ada <ada@x>", "ada@x");
        let meta = MailMeta {
            uid: "1".into(),
            account: acc.name.clone(),
            from: "bob@x".into(),
            subject: "hi".into(),
            body: String::new(),
            ..MailMeta::default()
        };
        store
            .write_mail(
                MailBox::Unread,
                &meta,
                "secret body s3cret-not-a-password-field",
            )
            .unwrap();
        let json = serde_json::to_string(&acc).unwrap();
        assert!(!json.contains("password"));
        let mails = store.load_mails(MailBox::Unread).unwrap();
        let mut plain = String::new();
        for m in &mails {
            plain.push_str(&format!(
                "{}\tuid={}\tfrom={}\tsubject={}\n",
                m.name(),
                m.uid,
                m.from,
                m.subject
            ));
        }
        assert!(!plain.contains("password"));
        assert!(
            !plain.contains(&secrets::load_password("ada@x").unwrap_or_default())
                || secrets::load_password("ada@x").is_err()
        );
    }
}
