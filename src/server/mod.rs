//! goblind — our mail server. No Postfix. No Dovecot. No Purelymail.

pub mod args;
pub mod auth;
pub mod config;
pub mod maildir;
pub mod paths;
pub mod sky;

use crate::error::Error;
use args::{Args, Cmd, UserCmd};

pub fn run(args: Args) -> Result<u8, Error> {
    match args.cmd {
        Cmd::Run {
            smtp_in,
            smtp_sub,
            imap,
            bind,
        } => cmd_run(bind, smtp_in, smtp_sub, imap),
        Cmd::User { action } => match action {
            UserCmd::Add { address, password } => cmd_user_add(&address, password.as_deref()),
            UserCmd::Passwd { address, password } => cmd_user_passwd(&address, password.as_deref()),
            UserCmd::List => cmd_user_list(),
        },
        Cmd::Sky { action } => match action {
            args::SkyCmd::PrintDns { domain, mail_host } => {
                sky::print_dns(&domain, mail_host.as_deref());
                Ok(0)
            }
        },
    }
}

fn cmd_run(bind: String, smtp_in: u16, smtp_sub: u16, imap: u16) -> Result<u8, Error> {
    paths::ensure_layout()?;
    let file = config::load_or_empty()?;
    eprintln!(
        "goblind: home={} users={} (listeners not fully wired yet — next tasks)",
        paths::home().display(),
        file.users.len()
    );
    eprintln!("goblind: would bind {bind} smtp-in={smtp_in} smtp-sub={smtp_sub} imap={imap}");
    eprintln!("goblind: use `goblind user add design@vanguardaautomovel.com` then continue build");
    Err(Error::Usage(
        "goblind run: SMTP/IMAP listeners land in the next tasks — layout + users are ready"
            .into(),
    ))
}

fn cmd_user_add(address: &str, password: Option<&str>) -> Result<u8, Error> {
    let address = normalize_addr(address)?;
    let password = match password {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => read_secret(&format!("password for {address}: "))?,
    };
    if password.is_empty() {
        return Err(Error::Usage("empty password".into()));
    }
    paths::ensure_layout()?;
    let mut file = config::load_or_empty()?;
    if file.users.iter().any(|u| u.address == address) {
        return Err(Error::say(
            format!("user {address} already exists"),
            "goblind user passwd …",
        ));
    }
    let local = localpart(&address)?;
    let md = paths::maildir_root().join(local);
    maildir::ensure(&md)?;
    file.users.push(config::User {
        address: address.clone(),
        maildir: format!("mail/{local}"),
    });
    if file.domain.is_empty() {
        if let Some((_, domain)) = address.split_once('@') {
            file.domain = domain.to_string();
        }
    }
    config::save(&file)?;
    auth::store_password(&address, &password)?;
    println!("added {address}");
    Ok(0)
}

fn cmd_user_passwd(address: &str, password: Option<&str>) -> Result<u8, Error> {
    let address = normalize_addr(address)?;
    let file = config::load()?;
    file.user(&address)?;
    let password = match password {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => read_secret(&format!("new password for {address}: "))?,
    };
    if password.is_empty() {
        return Err(Error::Usage("empty password".into()));
    }
    auth::store_password(&address, &password)?;
    println!("updated password for {address}");
    Ok(0)
}

fn cmd_user_list() -> Result<u8, Error> {
    let file = config::load_or_empty()?;
    if file.users.is_empty() {
        println!("(no users — goblind user add ADDR)");
        return Ok(0);
    }
    if !file.domain.is_empty() {
        println!("domain: {}", file.domain);
    }
    for u in &file.users {
        println!("{}\t{}", u.address, u.maildir);
    }
    Ok(0)
}

fn normalize_addr(address: &str) -> Result<String, Error> {
    let a = address.trim().to_ascii_lowercase();
    if !a.contains('@') || a.starts_with('@') || a.ends_with('@') {
        return Err(Error::Usage(format!("bad address {address:?}")));
    }
    Ok(a)
}

fn localpart(address: &str) -> Result<&str, Error> {
    address
        .split_once('@')
        .map(|(l, _)| l)
        .filter(|l| !l.is_empty())
        .ok_or_else(|| Error::Usage(format!("bad address {address:?}")))
}

fn read_secret(prompt: &str) -> Result<String, Error> {
    use std::io::{self, IsTerminal, Write};
    eprint!("{prompt}");
    io::stderr().flush().ok();
    if !io::stdin().is_terminal() {
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
