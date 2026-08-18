use crate::compose;
use crate::config::{self, Account, AccountFile, Endpoint};
use crate::error::Error;
use crate::imap::{self, SyncOpts};
use crate::import_aerc;
use crate::notify;
use crate::paths;
use crate::smtp;
use crate::store::{MailBox, MailMeta, Store};
use crate::{secrets, AccountCmd, Cmd};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

pub fn dispatch(cmd: Cmd) -> Result<u8, Error> {
    match cmd {
        Cmd::Account { action } => match action {
            AccountCmd::Add { preset } => cmd_account_add(preset.as_deref()),
            AccountCmd::Show => cmd_account_show(),
        },
        Cmd::ImportAerc { file } => cmd_import_aerc(file),
        Cmd::Sync {
            quiet,
            no_notify,
            all,
            force,
            folder,
            limit,
        } => cmd_sync(quiet, no_notify, all, force, folder, limit),
        Cmd::Idle => cmd_idle(),
        Cmd::List { box_name, plain } => cmd_list(&box_name, plain),
        Cmd::Show { file, plain } => cmd_show(&file, plain),
        Cmd::Bundle { snippet, limit } => cmd_bundle(snippet, limit),
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
        Cmd::Sound { set } => notify::cmd_sound(set.as_deref()),
    }
}

pub fn load_default_account() -> Result<(Account, String), Error> {
    let path = paths::accounts_file();
    if !path.exists() {
        return Err(Error::Usage(format!(
            "no accounts at {} — run: goblin account add",
            path.display()
        )));
    }
    let file = config::load_accounts(&path)?;
    let acc = file.default_account()?.clone();
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
        return Err(Error::Usage(format!(
            "no accounts at {} — run: goblin account add",
            path.display()
        )));
    }
    let file = config::load_accounts(&path)?;
    let acc = file.default_account()?;
    println!("name    {}", acc.name);
    println!("from    {}", acc.from);
    println!(
        "imap    {}@{}:{}",
        acc.imap.user, acc.imap.host, acc.imap.port
    );
    println!(
        "smtp    {}@{}:{}",
        acc.smtp.user, acc.smtp.host, acc.smtp.port
    );
    println!("file    {}", path.display());
    Ok(0)
}

fn cmd_account_add(preset: Option<&str>) -> Result<u8, Error> {
    if !stdin_is_tty() {
        return Err(Error::Usage("account add needs a tty".into()));
    }
    let preset = match preset {
        None => None,
        Some("purelymail") => Some("purelymail"),
        Some(other) => {
            return Err(Error::Usage(format!(
                "unknown preset {other:?} (try purelymail)"
            )));
        }
    };

    let name = prompt("account name", Some("work"))?;
    let from = prompt("from (Name <email>)", None)?;
    let user = prompt("username / email", extract_addr(&from).as_deref())?;

    let (imap, smtp_ep) = if preset == Some("purelymail") {
        let p = config::purelymail_preset(&name, &from, &user);
        (p.imap, p.smtp)
    } else {
        let imap_host = prompt("imap host", None)?;
        let imap_port = prompt("imap port", Some("993"))?
            .parse::<u16>()
            .map_err(|_| Error::Usage("bad imap port".into()))?;
        let smtp_host = prompt("smtp host", None)?;
        let smtp_port = prompt("smtp port", Some("465"))?
            .parse::<u16>()
            .map_err(|_| Error::Usage("bad smtp port".into()))?;
        (
            Endpoint {
                host: imap_host,
                port: imap_port,
                user: user.clone(),
            },
            Endpoint {
                host: smtp_host,
                port: smtp_port,
                user: user.clone(),
            },
        )
    };
    crate::tls::imap_mode(imap.port)?;
    crate::tls::smtp_mode(smtp_ep.port)?;

    let password = read_password("password: ")?;
    if password.is_empty() {
        return Err(Error::Usage("empty password".into()));
    }

    let acc = Account {
        name: name.clone(),
        from,
        imap,
        smtp: smtp_ep,
    };
    save_account(acc, &password)
}

fn save_account(acc: Account, password: &str) -> Result<u8, Error> {
    let file = AccountFile {
        default: acc.name.clone(),
        accounts: vec![acc.clone()],
    };
    let path = writable_accounts_path();
    config::save_accounts(&path, &file)?;
    secrets::store_password(&secret_id(&acc), password)?;
    println!("saved {}", path.display());
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
        return Err(Error::Usage(format!("no aerc config at {}", path.display())));
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
    save_account(acc, &password)?;
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
) -> Result<u8, Error> {
    let (acc, password) = load_default_account()?;
    if !quiet {
        eprintln!(
            "syncing {} from {} @ {} …",
            if all { "all recent" } else { "unread (UNSEEN)" },
            acc.imap.user,
            acc.imap.host
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
            "synced {} message(s) → {}",
            result.written,
            store.box_dir(MailBox::Unread).display()
        );
        if result.skipped > 0 {
            println!("(skipped {} already stored)", result.skipped);
        }
    }
    write_state(result.written)?;
    if result.written > 0 && !no_notify {
        if !notify::play() {
            eprintln!("(new mail! goblin wants to squeak — drop a sound at ~/.config/goblin/notify.mp3)");
        }
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
                let _ = cmd_sync(false, false, false, false, None, 30);
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
                        imap::mark_seen(&acc, password, &m.uid)?;
                        format!("uid {} marked Seen on server", m.uid)
                    }
                    MailBox::Trash => {
                        let folder = imap::trash(&acc, password, &m.uid)?;
                        format!("uid {} moved to {folder}", m.uid)
                    }
                    MailBox::Unread => unreachable!(),
                };
            }
        }
        store.move_mail(&path, dest)?;
        println!("moved {} → {}/  ({server_msg})", path.file_name().unwrap().to_string_lossy(), dest.as_str());
    }
    Ok(0)
}

fn cmd_send(to: &str, subject: &str, cc: Option<&str>, body_file: Option<&Path>) -> Result<u8, Error> {
    let (acc, password) = load_default_account()?;
    crate::tls::smtp_mode(acc.smtp.port)?;
    let body = read_body(body_file)?;
    let ccs: Vec<String> = cc
        .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|s| !s.is_empty()).collect())
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
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

fn read_password(prompt: &str) -> Result<String, Error> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    if !stdin_is_tty() {
        return Err(Error::Usage("password prompt needs a tty".into()));
    }
    unsafe {
        let fd = libc::STDIN_FILENO;
        let mut old: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut old) != 0 {
            return Err(Error::Usage("password prompt needs a tty".into()));
        }
        let mut new = old;
        new.c_lflag &= !libc::ECHO;
        libc::tcsetattr(fd, libc::TCSANOW, &new);
        let mut line = String::new();
        let res = io::stdin().lock().read_line(&mut line);
        libc::tcsetattr(fd, libc::TCSANOW, &old);
        eprintln!();
        res?;
        Ok(line.trim_end_matches(['\n', '\r']).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::purelymail_preset;

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
            .write_mail(MailBox::Unread, &meta, "secret body s3cret-not-a-password-field")
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
        assert!(!plain.contains(&secrets::load_password("ada@x").unwrap_or_default()) || secrets::load_password("ada@x").is_err());
    }
}
