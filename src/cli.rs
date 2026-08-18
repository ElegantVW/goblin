use crate::config::{self, Account, AccountFile, Endpoint};
use crate::error::Error;
use crate::paths;
use crate::{secrets, AccountCmd, Cmd};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

pub fn dispatch(cmd: Cmd) -> Result<u8, Error> {
    match cmd {
        Cmd::Account { action } => match action {
            AccountCmd::Add { preset } => cmd_account_add(preset.as_deref()),
            AccountCmd::Show => cmd_account_show(),
        },
        Cmd::ImportAerc { .. } => Err(Error::NotImplemented("import-aerc")),
        Cmd::Sync { .. } => Err(Error::NotImplemented("sync")),
        Cmd::Idle => Err(Error::NotImplemented("idle")),
        Cmd::List { .. } => Err(Error::NotImplemented("list")),
        Cmd::Show { .. } => Err(Error::NotImplemented("show")),
        Cmd::Bundle { .. } => Err(Error::NotImplemented("bundle")),
        Cmd::Move { .. } => Err(Error::NotImplemented("move")),
        Cmd::Send { .. } => Err(Error::NotImplemented("send")),
        Cmd::Sound { .. } => Err(Error::NotImplemented("sound")),
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
    let secret_id = secret_id(&acc);
    let password = secrets::load_password(&secret_id)?;
    Ok((acc, password))
}

pub fn secret_id(acc: &Account) -> String {
    acc.imap.user.clone()
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
    println!("imap    {}@{}:{}", acc.imap.user, acc.imap.host, acc.imap.port);
    println!("smtp    {}@{}:{}", acc.smtp.user, acc.smtp.host, acc.smtp.port);
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

    let (imap, smtp) = if preset == Some("purelymail") {
        (
            Endpoint::purelymail_imap(&user),
            Endpoint::purelymail_smtp(&user),
        )
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

    let password = read_password("password: ")?;
    if password.is_empty() {
        return Err(Error::Usage("empty password".into()));
    }

    let acc = Account {
        name: name.clone(),
        from,
        imap,
        smtp,
    };
    let file = AccountFile {
        default: name,
        accounts: vec![acc.clone()],
    };
    let path = paths::accounts_file();
    // never write .gpg from this path
    let path = if path.extension().and_then(|s| s.to_str()) == Some("gpg") {
        paths::config_dir().join("accounts.json")
    } else {
        path
    };
    config::save_accounts(&path, &file)?;
    secrets::store_password(&secret_id(&acc), &password)?;
    println!("saved {}", path.display());
    Ok(0)
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

#[allow(dead_code)]
pub fn accounts_path() -> PathBuf {
    paths::accounts_file()
}
