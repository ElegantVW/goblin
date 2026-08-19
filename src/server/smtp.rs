//! SMTP inbound (MX) and submission. No Postfix.

use super::auth;
use super::config;
use super::maildir;
use super::paths;
use crate::error::Error;
use crate::fsutil;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

const DATA_LIMIT: usize = 25 * 1024 * 1024;
const LINE_LIMIT: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Inbound,
    Submission,
}

pub async fn accept_loop(
    listener: TcpListener,
    kind: Kind,
    tls: Option<Arc<rustls::ServerConfig>>,
    implicit: bool,
) {
    loop {
        match listener.accept().await {
            Ok((sock, peer)) => {
                let tls = tls.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle(sock, kind, tls, implicit).await {
                        eprintln!("goblind: smtp {peer}: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("goblind: smtp accept: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

async fn handle(
    sock: TcpStream,
    kind: Kind,
    tls: Option<Arc<rustls::ServerConfig>>,
    implicit: bool,
) -> Result<(), Error> {
    let _ = sock.set_nodelay(true);
    let starttls = if implicit { None } else { tls.clone() };
    let io = if implicit {
        let cfg = tls.ok_or_else(|| Error::TlsPolicy("implicit smtp tls requires certs".into()))?;
        let s = TlsAcceptor::from(cfg)
            .accept(sock)
            .await
            .map_err(|e| Error::TlsPolicy(format!("smtp tls: {e}")))?;
        SmtpIo::Tls(Box::new(BufReader::new(s)))
    } else {
        SmtpIo::Plain(BufReader::new(sock))
    };
    session(io, kind, starttls).await
}

enum SmtpIo {
    Plain(BufReader<TcpStream>),
    Tls(Box<BufReader<TlsStream<TcpStream>>>),
}

impl SmtpIo {
    async fn read_line(&mut self, line: &mut String) -> std::io::Result<usize> {
        match self {
            Self::Plain(r) => r.read_line(line).await,
            Self::Tls(r) => r.read_line(line).await,
        }
    }

    async fn read_until(&mut self, delim: u8, buf: &mut Vec<u8>) -> std::io::Result<usize> {
        match self {
            Self::Plain(r) => r.read_until(delim, buf).await,
            Self::Tls(r) => r.read_until(delim, buf).await,
        }
    }

    async fn write_all(&mut self, b: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Plain(r) => r.get_mut().write_all(b).await,
            Self::Tls(r) => r.get_mut().write_all(b).await,
        }
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(r) => r.get_mut().flush().await,
            Self::Tls(r) => r.get_mut().flush().await,
        }
    }

    fn is_tls(&self) -> bool {
        matches!(self, Self::Tls(_))
    }

    async fn into_tls(self, cfg: Arc<rustls::ServerConfig>) -> Result<Self, Error> {
        match self {
            Self::Plain(r) => {
                let tcp = r.into_inner();
                let s = TlsAcceptor::from(cfg)
                    .accept(tcp)
                    .await
                    .map_err(|e| Error::TlsPolicy(format!("starttls: {e}")))?;
                Ok(Self::Tls(Box::new(BufReader::new(s))))
            }
            Self::Tls(_) => Ok(self),
        }
    }
}

enum AuthPending {
    Plain,
    LoginUser,
    LoginPass { user: String },
}

struct State {
    kind: Kind,
    user: Option<String>,
    mail_from: Option<String>,
    rcpt: Vec<String>,
    pending: Option<AuthPending>,
}

impl State {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            user: None,
            mail_from: None,
            rcpt: Vec::new(),
            pending: None,
        }
    }

    fn reset_tx(&mut self) {
        self.mail_from = None;
        self.rcpt.clear();
    }
}

async fn session(
    mut io: SmtpIo,
    kind: Kind,
    mut starttls: Option<Arc<rustls::ServerConfig>>,
) -> Result<(), Error> {
    let host = smtp_host();
    reply(&mut io, &format!("220 {host} ESMTP goblind")).await?;
    let mut st = State::new(kind);
    loop {
        let mut line = String::new();
        let n = io
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Smtp(e.to_string()))?;
        if n == 0 {
            return Ok(());
        }
        if line.len() > LINE_LIMIT {
            reply(&mut io, "500 5.5.4 line too long").await?;
            return Ok(());
        }
        let raw = line.trim_end_matches(['\r', '\n']);
        if let Some(pending) = st.pending.take() {
            continue_auth(&mut io, &mut st, pending, raw).await?;
            continue;
        }
        let (cmd, arg) = split_cmd(raw);
        match cmd.as_str() {
            "EHLO" | "HELO" => {
                st.reset_tx();
                let tls_now = io.is_tls();
                let offer = starttls.is_some();
                ehlo_reply(&mut io, &host, kind, tls_now, offer, cmd == "EHLO").await?;
            }
            "STARTTLS" => {
                if io.is_tls() {
                    reply(&mut io, "503 5.5.1 already TLS").await?;
                    continue;
                }
                let Some(cfg) = starttls.take() else {
                    reply(&mut io, "502 5.5.1 STARTTLS not available").await?;
                    continue;
                };
                reply(&mut io, "220 2.0.0 Ready to start TLS").await?;
                io = io.into_tls(cfg).await?;
                st = State::new(kind);
            }
            "AUTH" => {
                if kind != Kind::Submission {
                    reply(&mut io, "502 5.5.1 AUTH not offered").await?;
                    continue;
                }
                if !io.is_tls() {
                    reply(&mut io, "530 5.7.0 Must issue a STARTTLS command first").await?;
                    continue;
                }
                if st.user.is_some() {
                    reply(&mut io, "503 5.5.1 already authenticated").await?;
                    continue;
                }
                auth_start(&mut io, &mut st, &arg).await?;
            }
            "MAIL" => match mail_cmd(&mut st, io.is_tls(), &arg) {
                Ok(()) => reply(&mut io, "250 2.1.0 OK").await?,
                Err(msg) => reply(&mut io, msg).await?,
            },
            "RCPT" => match rcpt_cmd(&mut st, &arg) {
                Ok(()) => reply(&mut io, "250 2.1.5 OK").await?,
                Err(msg) => reply(&mut io, msg).await?,
            },
            "DATA" => {
                if st.mail_from.is_none() || st.rcpt.is_empty() {
                    reply(&mut io, "503 5.5.1 need MAIL and RCPT").await?;
                    continue;
                }
                reply(&mut io, "354 End data with <CR><LF>.<CR><LF>").await?;
                let data = read_data(&mut io).await?;
                match finish_data(&st, &data) {
                    Ok(()) => reply(&mut io, "250 2.0.0 Message accepted").await?,
                    Err(e) => {
                        reply(&mut io, "451 4.3.0 local error").await?;
                        eprintln!("goblind: smtp data: {e}");
                    }
                }
                st.reset_tx();
            }
            "RSET" => {
                st.reset_tx();
                reply(&mut io, "250 2.0.0 OK").await?;
            }
            "NOOP" => reply(&mut io, "250 2.0.0 OK").await?,
            "VRFY" => reply(&mut io, "252 2.1.5 send some mail").await?,
            "HELP" => {
                reply(
                    &mut io,
                    "214 2.0.0 EHLO HELO STARTTLS AUTH MAIL RCPT DATA RSET NOOP QUIT",
                )
                .await?;
            }
            "QUIT" => {
                reply(&mut io, "221 2.0.0 Bye").await?;
                return Ok(());
            }
            _ => reply(&mut io, "500 5.5.2 unrecognized command").await?,
        }
    }
}

async fn auth_start(io: &mut SmtpIo, st: &mut State, arg: &str) -> Result<(), Error> {
    let (mech, rest) = match arg.split_once(' ') {
        Some((m, r)) => (m.to_ascii_uppercase(), r.trim().to_string()),
        None => (arg.to_ascii_uppercase(), String::new()),
    };
    match mech.as_str() {
        "PLAIN" => {
            if rest.is_empty() {
                st.pending = Some(AuthPending::Plain);
                reply(io, "334 ").await
            } else {
                finish_plain(io, st, &rest).await
            }
        }
        "LOGIN" => {
            if rest.is_empty() {
                st.pending = Some(AuthPending::LoginUser);
                reply(io, "334 VXNlcm5hbWU6").await
            } else if let Some(user) = decode_b64(&rest) {
                st.pending = Some(AuthPending::LoginPass { user });
                reply(io, "334 UGFzc3dvcmQ6").await
            } else {
                reply(io, "501 5.5.4 bad AUTH").await
            }
        }
        _ => reply(io, "504 5.5.4 unknown AUTH mechanism").await,
    }
}

async fn continue_auth(
    io: &mut SmtpIo,
    st: &mut State,
    pending: AuthPending,
    line: &str,
) -> Result<(), Error> {
    if line.eq_ignore_ascii_case("*") {
        return reply(io, "501 5.7.0 AUTH aborted").await;
    }
    match pending {
        AuthPending::Plain => finish_plain(io, st, line).await,
        AuthPending::LoginUser => {
            if let Some(user) = decode_b64(line) {
                st.pending = Some(AuthPending::LoginPass { user });
                reply(io, "334 UGFzc3dvcmQ6").await
            } else {
                reply(io, "501 5.5.4 bad AUTH").await
            }
        }
        AuthPending::LoginPass { user } => {
            if let Some(pass) = decode_b64(line) {
                finish_login(io, st, &user, &pass).await
            } else {
                reply(io, "501 5.5.4 bad AUTH").await
            }
        }
    }
}

async fn finish_plain(io: &mut SmtpIo, st: &mut State, b64: &str) -> Result<(), Error> {
    let Some(bytes) = decode_b64_bytes(b64) else {
        return reply(io, "501 5.5.4 bad AUTH").await;
    };
    let mut parts = bytes.split(|b| *b == 0);
    let _zid = parts.next();
    let user = parts.next().and_then(|s| std::str::from_utf8(s).ok());
    let pass = parts.next().and_then(|s| std::str::from_utf8(s).ok());
    match (user, pass) {
        (Some(u), Some(p)) => finish_login(io, st, u, p).await,
        _ => reply(io, "501 5.5.4 bad AUTH PLAIN").await,
    }
}

async fn finish_login(
    io: &mut SmtpIo,
    st: &mut State,
    user: &str,
    pass: &str,
) -> Result<(), Error> {
    match auth::verify_login(user, pass) {
        Ok(Some(addr)) => {
            st.user = Some(addr);
            reply(io, "235 2.7.0 Authentication successful").await
        }
        Ok(None) => reply(io, "535 5.7.8 Authentication failed").await,
        Err(e) => {
            eprintln!("goblind: smtp auth: {e}");
            reply(io, "454 4.7.0 Temporary authentication failure").await
        }
    }
}

fn mail_cmd(st: &mut State, tls: bool, arg: &str) -> Result<(), &'static str> {
    if st.kind == Kind::Submission && !tls {
        return Err("530 5.7.0 Must issue a STARTTLS command first");
    }
    if st.kind == Kind::Submission && st.user.is_none() {
        return Err("530 5.7.0 Authentication required");
    }
    let from = parse_path(arg, "FROM").ok_or("501 5.1.7 bad MAIL FROM")?;
    if st.kind == Kind::Submission {
        if from.is_empty() {
            return Err("501 5.1.7 empty MAIL FROM");
        }
        let Some(user) = st.user.as_deref() else {
            return Err("530 5.7.0 Authentication required");
        };
        if from != user {
            return Err("550 5.7.1 MAIL FROM must match authenticated user");
        }
    }
    st.mail_from = Some(from);
    st.rcpt.clear();
    Ok(())
}

fn rcpt_cmd(st: &mut State, arg: &str) -> Result<(), &'static str> {
    if st.mail_from.is_none() {
        return Err("503 5.5.1 need MAIL first");
    }
    let rcpt = parse_path(arg, "TO").ok_or("501 5.1.3 bad RCPT TO")?;
    if rcpt.is_empty() {
        return Err("501 5.1.3 empty RCPT");
    }
    if st.kind == Kind::Inbound {
        let file = config::load_or_empty().map_err(|_| "451 4.3.0 local error")?;
        if !file.accepts_recipient(&rcpt) {
            return Err("550 5.1.1 mailbox unavailable");
        }
    } else if !looks_like_addr(&rcpt) {
        return Err("501 5.1.3 bad RCPT TO");
    }
    st.rcpt.push(rcpt);
    Ok(())
}

fn finish_data(st: &State, data: &[u8]) -> Result<(), Error> {
    let from = st.mail_from.clone().unwrap_or_default();
    match st.kind {
        Kind::Inbound => {
            let file = config::load()?;
            for rcpt in &st.rcpt {
                let u = file.user(rcpt)?;
                maildir::deliver(&paths::user_maildir(&u.maildir), data)?;
            }
        }
        Kind::Submission => {
            enqueue(&from, &st.rcpt, st.user.as_deref(), data)?;
            if let Ok(file) = config::load_or_empty() {
                for rcpt in &st.rcpt {
                    if let Ok(u) = file.user(rcpt) {
                        maildir::deliver(&paths::user_maildir(&u.maildir), data)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn enqueue(from: &str, to: &[String], user: Option<&str>, raw: &[u8]) -> Result<(), Error> {
    paths::ensure_layout()?;
    let dir = paths::queue_dir();
    let id = unique_queue_id();
    let env = serde_json::json!({
        "from": from,
        "to": to,
        "user": user,
    });
    let json = serde_json::to_string_pretty(&env)?;
    fsutil::write_private(
        dir.join(format!("{id}.json")).as_path(),
        format!("{json}\n").as_bytes(),
    )?;
    fsutil::write_private(dir.join(format!("{id}.eml")).as_path(), raw)?;
    Ok(())
}

fn unique_queue_id() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{t}.{}", std::process::id())
}

async fn read_data(io: &mut SmtpIo) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = io
            .read_until(b'\n', &mut line)
            .await
            .map_err(|e| Error::Smtp(e.to_string()))?;
        if n == 0 {
            return Err(Error::Smtp("connection closed during DATA".into()));
        }
        if line == b".\r\n" || line == b".\n" {
            break;
        }
        let chunk = if line.first() == Some(&b'.') {
            &line[1..]
        } else {
            line.as_slice()
        };
        if out.len().saturating_add(chunk.len()) > DATA_LIMIT {
            loop {
                line.clear();
                let n = io
                    .read_until(b'\n', &mut line)
                    .await
                    .map_err(|e| Error::Smtp(e.to_string()))?;
                if n == 0 || line == b".\r\n" || line == b".\n" {
                    break;
                }
            }
            return Err(Error::Smtp("message too large".into()));
        }
        out.extend_from_slice(chunk);
    }
    Ok(out)
}

async fn ehlo_reply(
    io: &mut SmtpIo,
    host: &str,
    kind: Kind,
    tls: bool,
    offer_starttls: bool,
    extended: bool,
) -> Result<(), Error> {
    if !extended {
        return reply(io, &format!("250 {host}")).await;
    }
    let mut lines = vec![
        format!("250-{host} hello"),
        "250-PIPELINING".into(),
        "250-8BITMIME".into(),
        format!("250-SIZE {DATA_LIMIT}"),
    ];
    if offer_starttls && !tls {
        lines.push("250-STARTTLS".into());
    }
    if kind == Kind::Submission && tls {
        lines.push("250-AUTH PLAIN LOGIN".into());
        lines.push("250 AUTH=PLAIN".into());
    } else {
        lines.push("250 OK".into());
    }
    for l in lines {
        reply(io, &l).await?;
    }
    Ok(())
}

async fn reply(io: &mut SmtpIo, line: &str) -> Result<(), Error> {
    io.write_all(line.as_bytes())
        .await
        .map_err(|e| Error::Smtp(e.to_string()))?;
    io.write_all(b"\r\n")
        .await
        .map_err(|e| Error::Smtp(e.to_string()))?;
    io.flush().await.map_err(|e| Error::Smtp(e.to_string()))?;
    Ok(())
}

fn split_cmd(line: &str) -> (String, String) {
    match line.split_once(' ') {
        Some((c, rest)) => (c.to_ascii_uppercase(), rest.to_string()),
        None => (line.to_ascii_uppercase(), String::new()),
    }
}

fn parse_path(arg: &str, kind: &str) -> Option<String> {
    let s = arg.trim();
    let s = if let Some(rest) = s.strip_prefix(kind) {
        rest.trim().trim_start_matches(':').trim()
    } else if let Some((_, rest)) = s.split_once(':') {
        rest.trim()
    } else {
        s
    };
    let inner = if let Some(rest) = s.strip_prefix('<') {
        rest.split('>').next()?.trim()
    } else {
        s.split_whitespace().next().unwrap_or("").trim()
    };
    if inner.is_empty() {
        return Some(String::new());
    }
    let addr = if inner.starts_with('@') {
        inner.rsplit(':').next().unwrap_or(inner)
    } else {
        inner
    };
    let addr = addr
        .trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_ascii_lowercase();
    if addr.is_empty() {
        Some(String::new())
    } else if looks_like_addr(&addr) {
        Some(addr)
    } else {
        None
    }
}

fn looks_like_addr(s: &str) -> bool {
    let Some((l, d)) = s.split_once('@') else {
        return false;
    };
    !l.is_empty() && !d.is_empty() && !d.starts_with('.') && !d.ends_with('.')
}

fn decode_b64(s: &str) -> Option<String> {
    decode_b64_bytes(s).and_then(|b| String::from_utf8(b).ok())
}

fn decode_b64_bytes(s: &str) -> Option<Vec<u8>> {
    STANDARD.decode(s.trim().as_bytes()).ok()
}

fn smtp_host() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "localhost".into())
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != ' ' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mail_from_and_rcpt() {
        assert_eq!(
            parse_path("FROM:<design@vanguardaautomovel.com>", "FROM").unwrap(),
            "design@vanguardaautomovel.com"
        );
        assert_eq!(
            parse_path("TO:<Ada@Ex.com> SIZE=10", "TO").unwrap(),
            "ada@ex.com"
        );
        assert_eq!(parse_path("FROM:<>", "FROM").unwrap(), "");
        assert!(parse_path("FROM:<not-an-addr>", "FROM").is_none());
    }
}
