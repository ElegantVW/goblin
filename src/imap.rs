//! IMAP client over rustls. No OpenSSL. No other mail programs.

use crate::config::Account;
use crate::error::Error;
use crate::store::{MailBox, MailMeta, Store};
use crate::tls::{self, TlsMode};
use mail_parser::MessageParser;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

const BODY_LIMIT: usize = 4000;
const IDLE_SECS: u64 = 15 * 60;

#[derive(Debug, Default, Clone)]
pub struct SyncOpts {
    pub all: bool,
    pub force: bool,
    pub limit: usize,
    pub folder: Option<String>,
}

#[derive(Debug, Default)]
pub struct SyncResult {
    pub written: usize,
    pub skipped: usize,
}

pub fn validate(port: u16) -> Result<(), Error> {
    tls::imap_mode(port).map(|_| ())
}

pub fn sync(account: &Account, password: &str, store: &Store, opts: SyncOpts) -> Result<SyncResult, Error> {
    tls::install_crypto();
    validate(account.imap.port)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::Imap(e.to_string()))?;
    rt.block_on(sync_async(account, password, store, opts))
}

pub fn mark_seen(account: &Account, password: &str, uid: &str) -> Result<(), Error> {
    tls::install_crypto();
    validate(account.imap.port)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::Imap(e.to_string()))?;
    rt.block_on(async {
        let folder = "INBOX";
        let mut c = Client::connect(account, password).await?;
        c.select(folder).await?;
        c.uid_store(uid, "+FLAGS.SILENT (\\Seen)").await?;
        c.logout().await.ok();
        Ok(())
    })
}

pub fn trash(account: &Account, password: &str, uid: &str) -> Result<String, Error> {
    tls::install_crypto();
    validate(account.imap.port)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::Imap(e.to_string()))?;
    rt.block_on(async {
        let mut c = Client::connect(account, password).await?;
        c.select("INBOX").await?;
        c.uid_store(uid, "+FLAGS.SILENT (\\Seen)").await?;
        let mut dest = String::from("deleted");
        for folder in ["Trash", "INBOX.Trash", "Deleted", "INBOX.Deleted Messages"] {
            if c.uid_copy(uid, folder).await.is_ok() {
                dest = folder.to_string();
                break;
            }
        }
        c.uid_store(uid, "+FLAGS.SILENT (\\Deleted)").await?;
        c.cmd("EXPUNGE").await?;
        c.logout().await.ok();
        Ok(dest)
    })
}

pub fn idle_once(account: &Account, password: &str) -> Result<(), Error> {
    tls::install_crypto();
    validate(account.imap.port)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::Imap(e.to_string()))?;
    rt.block_on(async {
        let mut c = Client::connect(account, password).await?;
        c.select("INBOX").await?;
        c.idle(Duration::from_secs(IDLE_SECS)).await?;
        c.logout().await.ok();
        Ok(())
    })
}

async fn sync_async(
    account: &Account,
    password: &str,
    store: &Store,
    opts: SyncOpts,
) -> Result<SyncResult, Error> {
    let folder = opts
        .folder
        .clone()
        .unwrap_or_else(|| "INBOX".into());
    let mut c = Client::connect(account, password).await?;
    c.select(&folder).await?;
    let spec = if opts.all { "ALL" } else { "UNSEEN" };
    let mut uids = c.uid_search(spec).await?;
    if opts.all && opts.limit > 0 && uids.len() > opts.limit {
        uids = uids[uids.len() - opts.limit..].to_vec();
    }
    let known = if opts.force {
        Default::default()
    } else {
        store.known_uids()?
    };
    let mut result = SyncResult::default();
    for uid in uids {
        let key = uid.to_string();
        if known.contains(&key) && !opts.force {
            result.skipped += 1;
            continue;
        }
        let raw = c.uid_fetch_rfc822(&key).await?;
        let (meta, body) = parse_message(&key, account, &folder, &raw);
        store.write_mail(MailBox::Unread, &meta, &body)?;
        result.written += 1;
    }
    c.logout().await.ok();
    Ok(result)
}

fn parse_message(uid: &str, account: &Account, folder: &str, raw: &[u8]) -> (MailMeta, String) {
    let parsed = MessageParser::default().parse(raw);
    let (from, to, date, subject, message_id, body) = if let Some(msg) = parsed {
        let from = msg
            .from()
            .and_then(|a| a.first())
            .map(|a| format_addr(a.name(), a.address()))
            .unwrap_or_default();
        let to = msg
            .to()
            .and_then(|a| a.first())
            .map(|a| format_addr(a.name(), a.address()))
            .unwrap_or_default();
        let date = msg.date().map(|d| d.to_rfc3339()).unwrap_or_default();
        let subject = msg.subject().unwrap_or("(no subject)").to_string();
        let message_id = msg.message_id().unwrap_or("").to_string();
        let body = extract_body(&msg);
        (from, to, date, subject, message_id, body)
    } else {
        (
            String::new(),
            String::new(),
            String::new(),
            "(no subject)".into(),
            String::new(),
            String::from_utf8_lossy(raw).into_owned(),
        )
    };
    let mut body = collapse_ws(&body);
    if body.len() > BODY_LIMIT {
        body.truncate(BODY_LIMIT);
        body.push('…');
    }
    let meta = MailMeta {
        uid: uid.into(),
        account: account.name.clone(),
        folder: folder.into(),
        from,
        to,
        date,
        subject,
        message_id,
        path: None,
        body: String::new(),
    };
    (meta, body)
}

fn format_addr(name: Option<&str>, addr: Option<&str>) -> String {
    match (name, addr) {
        (Some(n), Some(a)) if !n.is_empty() => format!("{n} <{a}>"),
        (_, Some(a)) => a.to_string(),
        (Some(n), _) => n.to_string(),
        _ => String::new(),
    }
}

fn extract_body(msg: &mail_parser::Message<'_>) -> String {
    if let Some(t) = msg.body_text(0) {
        return t.to_string();
    }
    if let Some(h) = msg.body_html(0) {
        return html_to_text(&h);
    }
    String::new()
}

fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for (pat, rep) in [
        ("<br>", "\n"),
        ("<br/>", "\n"),
        ("<br />", "\n"),
        ("</p>", "\n"),
        ("</div>", "\n"),
        ("</tr>", "\n"),
        ("</li>", "\n"),
    ] {
        s = s.replace(pat, rep);
        s = s.replace(&pat.to_ascii_uppercase(), rep);
    }
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            prev_space = false;
            out.push(c);
        }
    }
    out.trim().to_string()
}

struct Client {
    r: BufReader<TlsStream<TcpStream>>,
    tag: u32,
}

impl Client {
    async fn connect(account: &Account, password: &str) -> Result<Self, Error> {
        let host = &account.imap.host;
        let port = account.imap.port;
        let mode = tls::imap_mode(port)?;
        let tcp = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|e| Error::Imap(format!("connect {host}:{port}: {e}")))?;
        let tls = match mode {
            TlsMode::Implicit => {
                let tls = tls::wrap_tls(host, tcp).await?;
                let mut c = Client {
                    r: BufReader::new(tls),
                    tag: 0,
                };
                c.read_line().await?; // greeting
                c
            }
            TlsMode::StartTls => {
                let mut plain = BufReader::new(tcp);
                let mut greet = String::new();
                plain
                    .read_line(&mut greet)
                    .await
                    .map_err(|e| Error::Imap(e.to_string()))?;
                plain
                    .get_mut()
                    .write_all(b"A0000 STARTTLS\r\n")
                    .await
                    .map_err(|e| Error::Imap(e.to_string()))?;
                plain
                    .get_mut()
                    .flush()
                    .await
                    .map_err(|e| Error::Imap(e.to_string()))?;
                loop {
                    let mut line = String::new();
                    plain
                        .read_line(&mut line)
                        .await
                        .map_err(|e| Error::Imap(e.to_string()))?;
                    if line.starts_with("A0000 OK") {
                        break;
                    }
                    if line.starts_with("A0000 NO") || line.starts_with("A0000 BAD") {
                        return Err(Error::Imap(format!(
                            "STARTTLS refused: {}",
                            line.trim()
                        )));
                    }
                }
                let tcp = plain.into_inner();
                let tls = tls::wrap_tls(host, tcp).await?;
                Client {
                    r: BufReader::new(tls),
                    tag: 0,
                }
            }
        };
        let mut c = tls;
        let user = imap_quote(&account.imap.user);
        let pass = imap_quote(password);
        c.cmd(&format!("LOGIN {user} {pass}")).await?;
        Ok(c)
    }

    async fn select(&mut self, folder: &str) -> Result<(), Error> {
        self.cmd(&format!("SELECT {}", imap_quote(folder))).await?;
        Ok(())
    }

    async fn uid_search(&mut self, spec: &str) -> Result<Vec<u32>, Error> {
        let lines = self.cmd(&format!("UID SEARCH {spec}")).await?;
        let mut uids = Vec::new();
        for line in lines {
            if let Some(rest) = line.strip_prefix("* SEARCH") {
                for p in rest.split_whitespace() {
                    if let Ok(n) = p.parse() {
                        uids.push(n);
                    }
                }
            }
        }
        Ok(uids)
    }

    async fn uid_fetch_rfc822(&mut self, uid: &str) -> Result<Vec<u8>, Error> {
        let tag = self.next_tag();
        let cmd = format!("{tag} UID FETCH {uid} (UID BODY.PEEK[])\r\n");
        self.r
            .get_mut()
            .write_all(cmd.as_bytes())
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        self.r
            .get_mut()
            .flush()
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        read_fetch_body(&mut self.r, &tag).await
    }

    async fn uid_store(&mut self, uid: &str, item: &str) -> Result<(), Error> {
        self.cmd(&format!("UID STORE {uid} {item}")).await?;
        Ok(())
    }

    async fn uid_copy(&mut self, uid: &str, folder: &str) -> Result<(), Error> {
        self.cmd(&format!("UID COPY {uid} {}", imap_quote(folder)))
            .await?;
        Ok(())
    }

    async fn idle(&mut self, dur: Duration) -> Result<(), Error> {
        let tag = self.next_tag();
        self.r
            .get_mut()
            .write_all(format!("{tag} IDLE\r\n").as_bytes())
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        self.r
            .get_mut()
            .flush()
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        // wait for + 
        let _ = tokio::time::timeout(Duration::from_secs(30), self.read_line()).await;
        let _ = tokio::time::timeout(dur, self.read_line()).await;
        self.r
            .get_mut()
            .write_all(b"DONE\r\n")
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        self.r
            .get_mut()
            .flush()
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        // drain until tagged
        let prefix = format!("{tag} ");
        loop {
            let line = self.read_line().await?;
            if line.starts_with(&prefix) {
                if line[prefix.len()..].starts_with("OK") || line[prefix.len()..].starts_with("NO") || line[prefix.len()..].starts_with("BAD") {
                    if line[prefix.len()..].starts_with("OK") {
                        return Ok(());
                    }
                    return Err(Error::Imap(line));
                }
            }
        }
    }

    async fn logout(&mut self) -> Result<(), Error> {
        let _ = self.cmd("LOGOUT").await;
        Ok(())
    }

    async fn cmd(&mut self, command: &str) -> Result<Vec<String>, Error> {
        let tag = self.next_tag();
        let line = format!("{tag} {command}\r\n");
        self.r
            .get_mut()
            .write_all(line.as_bytes())
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        self.r
            .get_mut()
            .flush()
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        let prefix = format!("{tag} ");
        let mut lines = Vec::new();
        loop {
            let got = self.read_line().await?;
            if got.starts_with(&prefix) {
                let rest = &got[prefix.len()..];
                if rest.starts_with("OK") {
                    lines.push(got);
                    return Ok(lines);
                }
                return Err(Error::Imap(got));
            }
            lines.push(got);
        }
    }

    async fn read_line(&mut self) -> Result<String, Error> {
        let mut s = String::new();
        let n = self
            .r
            .read_line(&mut s)
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        if n == 0 {
            return Err(Error::Imap("connection closed".into()));
        }
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        Ok(s)
    }

    fn next_tag(&mut self) -> String {
        self.tag += 1;
        format!("A{:04}", self.tag)
    }
}

async fn read_fetch_body<S>(r: &mut BufReader<S>, tag: &str) -> Result<Vec<u8>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let prefix = format!("{tag} ");
    let mut body = Vec::new();
    loop {
        let mut line = String::new();
        let n = r
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        if n == 0 {
            return Err(Error::Imap("connection closed during fetch".into()));
        }
        if line.starts_with(&prefix) {
            if line[prefix.len()..].starts_with("OK") {
                return Ok(body);
            }
            return Err(Error::Imap(line.trim().into()));
        }
        if let Some(size) = literal_size(&line) {
            let mut buf = vec![0u8; size];
            r.read_exact(&mut buf)
                .await
                .map_err(|e| Error::Imap(e.to_string()))?;
            body = buf;
            // trailing line after literal
            let mut rest = String::new();
            let _ = r.read_line(&mut rest).await;
        }
    }
}

fn literal_size(line: &str) -> Option<usize> {
    let start = line.rfind('{')?;
    let end = line[start..].find('}')?;
    line[start + 1..start + end].parse().ok()
}

fn imap_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' | '"' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_refuses_cleartext_ports() {
        assert!(validate(993).is_ok());
        assert!(validate(143).is_ok());
        match validate(80) {
            Err(Error::TlsPolicy(_)) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn quote_escapes() {
        assert_eq!(imap_quote(r#"a"b\"#), r#""a\"b\\""#);
    }
}
