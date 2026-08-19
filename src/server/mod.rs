//! goblind — our mail server. No Postfix. No Dovecot. No Purelymail.

pub mod args;
pub mod auth;
pub mod config;
pub mod imap;
pub mod maildir;
pub mod paths;
pub mod sky;
pub mod smtp;
pub mod tlsutil;

use crate::error::Error;
use args::{Args, Cmd, UserCmd};
use tokio::net::TcpListener;

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
    crate::tls::install_crypto();
    tlsutil::ensure_certs()?;
    let file = config::load_or_empty()?;
    eprintln!(
        "goblind: home={} users={}",
        paths::home().display(),
        file.users.len()
    );
    eprintln!(
        "goblind: lab CA {}  (client: export GOBLIN_EXTRA_CA=that file)",
        tlsutil::ca_file().display()
    );
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Usage(format!("tokio: {e}")))?;
    rt.block_on(serve(bind, smtp_in, smtp_sub, imap))
}

async fn serve(bind: String, smtp_in: u16, smtp_sub: u16, imap: u16) -> Result<u8, Error> {
    let tls = tlsutil::server_config()?;
    let in_l = TcpListener::bind((bind.as_str(), smtp_in))
        .await
        .map_err(|e| Error::Smtp(format!("bind smtp-in {bind}:{smtp_in}: {e}")))?;
    let sub_l = TcpListener::bind((bind.as_str(), smtp_sub))
        .await
        .map_err(|e| Error::Smtp(format!("bind smtp-sub {bind}:{smtp_sub}: {e}")))?;
    let imap_l = TcpListener::bind((bind.as_str(), imap))
        .await
        .map_err(|e| Error::Imap(format!("bind imap {bind}:{imap}: {e}")))?;
    eprintln!(
        "goblind: listening smtp-in {}",
        in_l.local_addr().map_err(Error::Io)?
    );
    eprintln!(
        "goblind: listening smtp-sub {}",
        sub_l.local_addr().map_err(Error::Io)?
    );
    eprintln!(
        "goblind: listening imap {}",
        imap_l.local_addr().map_err(Error::Io)?
    );
    // 587 = STARTTLS; 465 and lab high ports = implicit TLS.
    let implicit_sub = smtp_sub != 587;
    tokio::spawn(smtp::accept_loop(
        in_l,
        smtp::Kind::Inbound,
        Some(tls.clone()),
        false,
    ));
    tokio::spawn(smtp::accept_loop(
        sub_l,
        smtp::Kind::Submission,
        Some(tls.clone()),
        implicit_sub,
    ));
    tokio::spawn(imap::accept_loop(imap_l, tls));
    tokio::signal::ctrl_c().await.map_err(Error::Io)?;
    eprintln!("goblind: stopping");
    Ok(0)
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

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::tls;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    const USER: &str = "design@vanguardaautomovel.com";
    const PASS: &str = "s3cret";

    fn setup_user() {
        paths::ensure_layout().unwrap();
        config::save(&config::AccountFile {
            domain: "vanguardaautomovel.com".into(),
            users: vec![config::User {
                address: USER.into(),
                maildir: "mail/design".into(),
            }],
        })
        .unwrap();
        auth::store_password(USER, PASS).unwrap();
        maildir::ensure(&paths::user_maildir("mail/design")).unwrap();
        tlsutil::ensure_certs().unwrap();
    }

    async fn smtp_line(r: &mut BufReader<TcpStream>) -> String {
        let mut s = String::new();
        r.read_line(&mut s).await.unwrap();
        s
    }

    async fn smtp_cmd(r: &mut BufReader<TcpStream>, cmd: &str) -> String {
        r.get_mut().write_all(cmd.as_bytes()).await.unwrap();
        r.get_mut().write_all(b"\r\n").await.unwrap();
        r.get_mut().flush().await.unwrap();
        smtp_line(r).await
    }

    #[test]
    fn inbound_smtp_then_imap_fetch() {
        tls::install_crypto();
        let dir = tempfile::tempdir().unwrap();
        paths::with_goblind_home(Some(dir.path()), || {
            setup_user();
            let ca = tlsutil::ca_file();
            tls::with_extra_ca_env(Some(&ca), || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let in_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let in_port = in_l.local_addr().unwrap().port();
                    tokio::spawn(smtp::accept_loop(in_l, smtp::Kind::Inbound, None, false));

                    let tcp = TcpStream::connect(("127.0.0.1", in_port)).await.unwrap();
                    let mut r = BufReader::new(tcp);
                    assert!(smtp_line(&mut r).await.starts_with("220"));
                    let ehlo = smtp_cmd(&mut r, "EHLO lab").await;
                    assert!(ehlo.starts_with("250"), "{ehlo}");
                    // drain EHLO continuation
                    loop {
                        if ehlo_done(&ehlo) {
                            break;
                        }
                        let l = smtp_line(&mut r).await;
                        if l.starts_with("250 ") {
                            break;
                        }
                    }
                    let from = smtp_cmd(&mut r, "MAIL FROM:<outside@example.com>").await;
                    assert!(from.starts_with("250"), "{from}");
                    let rcpt = smtp_cmd(&mut r, &format!("RCPT TO:<{USER}>")).await;
                    assert!(rcpt.starts_with("250"), "{rcpt}");
                    let data = smtp_cmd(&mut r, "DATA").await;
                    assert!(data.starts_with("354"), "{data}");
                    r.get_mut()
                        .write_all(b"Subject: goblin lab\r\n\r\nhello from mx\r\n.\r\n")
                        .await
                        .unwrap();
                    r.get_mut().flush().await.unwrap();
                    let accepted = smtp_line(&mut r).await;
                    assert!(accepted.starts_with("250"), "{accepted}");
                    let _ = smtp_cmd(&mut r, "QUIT").await;

                    let listed = maildir::indexed(&paths::user_maildir("mail/design")).unwrap();
                    assert_eq!(listed.len(), 1);
                    assert_eq!(listed[0].uid, 1);

                    let imap_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let imap_port = imap_l.local_addr().unwrap().port();
                    let tls_cfg = tlsutil::server_config().unwrap();
                    tokio::spawn(imap::accept_loop(imap_l, tls_cfg));
                    let tcp = TcpStream::connect(("127.0.0.1", imap_port)).await.unwrap();
                    let tls_s = tls::wrap_tls("localhost", tcp).await.unwrap();
                    let mut ir = BufReader::new(tls_s);
                    let greet = imap_line(&mut ir).await;
                    assert!(greet.starts_with("* OK"), "{greet}");
                    let login = imap_cmd(
                        &mut ir,
                        r#"A001 LOGIN "design@vanguardaautomovel.com" "s3cret""#,
                    )
                    .await;
                    assert!(login.iter().any(|l| l.starts_with("A001 OK")), "{login:?}");
                    let sel = imap_cmd(&mut ir, r#"A002 SELECT "INBOX""#).await;
                    assert!(sel.iter().any(|l| l.starts_with("A002 OK")), "{sel:?}");
                    let search = imap_cmd(&mut ir, "A003 UID SEARCH ALL").await;
                    assert!(
                        search
                            .iter()
                            .any(|l| l.starts_with("* SEARCH") && l.contains('1')),
                        "{search:?}"
                    );
                    let fetch = imap_cmd(&mut ir, "A004 UID FETCH 1 (UID BODY.PEEK[])").await;
                    assert!(fetch.iter().any(|l| l.starts_with("A004 OK")), "{fetch:?}");
                    let joined = fetch.join("\n");
                    assert!(
                        joined.contains("goblin lab") || joined.contains("hello from mx"),
                        "{joined}"
                    );
                    let store =
                        imap_cmd(&mut ir, r#"A005 UID STORE 1 +FLAGS.SILENT (\Seen)"#).await;
                    assert!(store.iter().any(|l| l.starts_with("A005 OK")), "{store:?}");
                    let _ = imap_cmd(&mut ir, "A006 LOGOUT").await;
                });
            });
        });
    }

    fn ehlo_done(line: &str) -> bool {
        line.starts_with("250 ")
    }

    async fn imap_line<S: tokio::io::AsyncBufRead + Unpin>(r: &mut S) -> String {
        let mut s = String::new();
        r.read_line(&mut s).await.unwrap();
        s.trim_end_matches(['\r', '\n']).to_string()
    }

    async fn imap_cmd<S>(r: &mut BufReader<S>, cmd: &str) -> Vec<String>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        r.get_mut().write_all(cmd.as_bytes()).await.unwrap();
        r.get_mut().write_all(b"\r\n").await.unwrap();
        r.get_mut().flush().await.unwrap();
        let tag = cmd.split_whitespace().next().unwrap().to_string();
        let prefix = format!("{tag} ");
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            r.read_line(&mut line).await.unwrap();
            if let Some(size) = literal_size(&line) {
                let mut buf = vec![0u8; size];
                tokio::io::AsyncReadExt::read_exact(r, &mut buf)
                    .await
                    .unwrap();
                lines.push(format!(
                    "{}{}",
                    line.trim_end(),
                    String::from_utf8_lossy(&buf)
                ));
                let mut rest = String::new();
                let _ = r.read_line(&mut rest).await;
                continue;
            }
            let t = line.trim_end_matches(['\r', '\n']).to_string();
            let done = t.starts_with(&prefix);
            lines.push(t);
            if done {
                break;
            }
        }
        lines
    }

    fn literal_size(line: &str) -> Option<usize> {
        let start = line.rfind('{')?;
        let end = line[start..].find('}')?;
        line[start + 1..start + end].parse().ok()
    }
}
