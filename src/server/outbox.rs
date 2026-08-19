//! Direct MX delivery on port 25. No Postfix. No smart-host.

use super::config;
use super::dkim;
use super::queue::{self, Job, MAX_ATTEMPTS};
use crate::error::Error;
use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

const POLL: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const IO_TIMEOUT: Duration = Duration::from_secs(60);
const LINE_LIMIT: usize = 8 * 1024;

pub async fn worker() {
    loop {
        if let Err(e) = tick().await {
            eprintln!("outbox: {e}");
        }
        tokio::time::sleep(POLL).await;
    }
}

pub async fn tick() -> Result<(), Error> {
    let jobs = queue::list_due_jobs()?;
    for job in jobs {
        process_job(job).await;
    }
    Ok(())
}

pub async fn deliver_rcpt(from: &str, to: &str, raw: &[u8]) -> Result<(), Error> {
    let domain = rcpt_domain(to)?;
    let hosts = mx_hosts(domain).await?;
    let port = delivery_port();
    let mut last = Error::Smtp(format!("no usable MX for {domain}"));
    for host in hosts {
        match deliver_to_host(&host, port, from, to, raw).await {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

async fn process_job(mut job: Job) {
    let mut raw = match queue::read_eml(&job.id) {
        Ok(b) => b,
        Err(e) => {
            job.last_error = Some(e.to_string());
            finish_attempt(&mut job);
            return;
        }
    };
    if job.attempts == 0 {
        match maybe_sign(&raw, &job.from) {
            Ok(signed) if signed != raw => {
                if let Err(e) = queue::write_eml(&job.id, &signed) {
                    eprintln!("outbox: id={} dkim write: {e}", job.id);
                } else {
                    raw = signed;
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("outbox: id={} dkim: {e}", job.id),
        }
    }

    let mut pending = Vec::new();
    let mut last_err = None;
    for rcpt in &job.to {
        match deliver_rcpt(&job.from, rcpt, &raw).await {
            Ok(()) => eprintln!("outbox: id={} to={rcpt} ok", job.id),
            Err(e) => {
                last_err = Some(e.to_string());
                pending.push(rcpt.clone());
            }
        }
    }

    if pending.is_empty() {
        if let Err(e) = queue::complete_job(&job.id) {
            eprintln!("outbox: id={} complete: {e}", job.id);
        }
        return;
    }

    job.to = pending;
    job.last_error = last_err;
    finish_attempt(&mut job);
}

fn finish_attempt(job: &mut Job) {
    job.attempts = job.attempts.saturating_add(1);
    let to = job.to.join(",");
    if job.attempts >= MAX_ATTEMPTS {
        eprintln!(
            "outbox: id={} to={to} fail {}",
            job.id,
            job.last_error.as_deref().unwrap_or("-")
        );
        if let Err(e) = queue::move_to_failed(job) {
            eprintln!("outbox: id={} failed-dir: {e}", job.id);
        }
        return;
    }
    let delay = queue::backoff_secs(job.attempts);
    job.next_attempt = queue::unix_now().saturating_add(delay);
    eprintln!(
        "outbox: id={} to={to} retry after={delay}s {}",
        job.id,
        job.last_error.as_deref().unwrap_or("-")
    );
    if let Err(e) = queue::save_job(job) {
        eprintln!("outbox: id={} save: {e}", job.id);
    }
}

fn maybe_sign(raw: &[u8], from: &str) -> Result<Vec<u8>, Error> {
    if has_dkim_signature(raw) {
        return Ok(raw.to_vec());
    }
    let file = config::load_or_empty()?;
    if file.domain.is_empty() {
        return Ok(raw.to_vec());
    }
    let Some(dom) = from.split_once('@').map(|(_, d)| d) else {
        return Ok(raw.to_vec());
    };
    if !dom.eq_ignore_ascii_case(&file.domain) {
        return Ok(raw.to_vec());
    }
    let selector = dkim::DEFAULT_SELECTOR;
    let pem_path = dkim::private_path(selector);
    if !pem_path.is_file() {
        return Ok(raw.to_vec());
    }
    let pem = std::fs::read_to_string(&pem_path)?;
    dkim::sign(&file.domain, selector, &pem, raw)
}

fn has_dkim_signature(raw: &[u8]) -> bool {
    let head = match raw.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(i) => &raw[..i],
        None => match raw.windows(2).position(|w| w == b"\n\n") {
            Some(i) => &raw[..i],
            None => raw,
        },
    };
    let text = String::from_utf8_lossy(head);
    text.lines()
        .any(|l| l.to_ascii_lowercase().starts_with("dkim-signature:"))
}

fn rcpt_domain(to: &str) -> Result<&str, Error> {
    to.split_once('@')
        .map(|(_, d)| d.trim())
        .filter(|d| !d.is_empty())
        .ok_or_else(|| Error::Smtp(format!("bad recipient {to}")))
}

fn delivery_port() -> u16 {
    std::env::var("GOBLIND_OUTBOX_SMTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|p| *p != 0)
        .unwrap_or(25)
}

async fn mx_hosts(domain: &str) -> Result<Vec<String>, Error> {
    if let Ok(stub) = std::env::var("GOBLIND_OUTBOX_MX") {
        let stub = stub.trim();
        if !stub.is_empty() {
            return Ok(vec![stub.to_string()]);
        }
    }
    lookup_mx(domain).await
}

async fn lookup_mx(domain: &str) -> Result<Vec<String>, Error> {
    let resolver = match TokioAsyncResolver::tokio_from_system_conf() {
        Ok(r) => r,
        Err(_) => TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default()),
    };
    let lookup = tokio::time::timeout(Duration::from_secs(15), resolver.mx_lookup(domain)).await;
    let mut ranked: Vec<(u16, String)> = Vec::new();
    if let Ok(Ok(mx)) = lookup {
        for rec in mx.iter() {
            let host = rec.exchange().to_ascii();
            let host = host.trim_end_matches('.').to_string();
            if host.is_empty() {
                // RFC 7505 null MX
                return Err(Error::Smtp(format!("{domain} null MX")));
            }
            ranked.push((rec.preference(), host));
        }
    }
    if ranked.is_empty() {
        return Ok(vec![domain.trim_end_matches('.').to_string()]);
    }
    ranked.sort_by_key(|(p, _)| *p);
    Ok(ranked.into_iter().map(|(_, h)| h).collect())
}

async fn deliver_to_host(
    host: &str,
    port: u16,
    from: &str,
    to: &str,
    raw: &[u8],
) -> Result<(), Error> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| Error::Smtp(format!("connect {host}:{port} timeout")))?
        .map_err(|e| Error::Smtp(format!("connect {host}:{port}: {e}")))?;
    let _ = tcp.set_nodelay(true);
    let mut io = Io::Plain(BufReader::new(tcp));
    let (code, greet) = read_reply(&mut io).await?;
    expect_code(code, 200..300, &greet)?;
    let ehlo_name = ehlo_name();
    let (code, ehlo) = smtp_cmd(&mut io, &format!("EHLO {ehlo_name}")).await?;
    expect_code(code, 200..300, &ehlo)?;
    if ehlo_has_starttls(&ehlo) {
        let (code, r) = smtp_cmd(&mut io, "STARTTLS").await?;
        expect_code(code, 200..300, &r)?;
        io = io.into_tls(host).await?;
        let (code, ehlo) = smtp_cmd(&mut io, &format!("EHLO {ehlo_name}")).await?;
        expect_code(code, 200..300, &ehlo)?;
    }
    let mail = if from.is_empty() {
        "MAIL FROM:<>".to_string()
    } else {
        format!("MAIL FROM:<{from}>")
    };
    let (code, r) = smtp_cmd(&mut io, &mail).await?;
    expect_code(code, 200..300, &r)?;
    let (code, r) = smtp_cmd(&mut io, &format!("RCPT TO:<{to}>")).await?;
    expect_code(code, 200..300, &r)?;
    let (code, r) = smtp_cmd(&mut io, "DATA").await?;
    expect_code(code, 300..400, &r)?;
    write_raw(&mut io, &smtp_data_payload(raw)).await?;
    let (code, r) = read_reply(&mut io).await?;
    expect_code(code, 200..300, &r)?;
    let _ = smtp_cmd(&mut io, "QUIT").await;
    Ok(())
}

enum Io {
    Plain(BufReader<TcpStream>),
    Tls(Box<BufReader<TlsStream<TcpStream>>>),
}

impl Io {
    async fn read_line(&mut self, line: &mut String) -> std::io::Result<usize> {
        match self {
            Self::Plain(r) => r.read_line(line).await,
            Self::Tls(r) => r.read_line(line).await,
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

    async fn into_tls(self, name: &str) -> Result<Self, Error> {
        match self {
            Self::Plain(r) => {
                let tcp = r.into_inner();
                let tls = crate::tls::wrap_tls(name, tcp).await?;
                Ok(Self::Tls(Box::new(BufReader::new(tls))))
            }
            Self::Tls(_) => Ok(self),
        }
    }
}

async fn smtp_cmd(io: &mut Io, line: &str) -> Result<(u16, String), Error> {
    write_raw(io, format!("{line}\r\n").as_bytes()).await?;
    read_reply(io).await
}

async fn write_raw(io: &mut Io, b: &[u8]) -> Result<(), Error> {
    tokio::time::timeout(IO_TIMEOUT, async {
        io.write_all(b)
            .await
            .map_err(|e| Error::Smtp(e.to_string()))?;
        io.flush().await.map_err(|e| Error::Smtp(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|_| Error::Smtp("smtp write timeout".into()))?
}

async fn read_reply(io: &mut Io) -> Result<(u16, String), Error> {
    tokio::time::timeout(IO_TIMEOUT, read_reply_inner(io))
        .await
        .map_err(|_| Error::Smtp("smtp read timeout".into()))?
}

async fn read_reply_inner(io: &mut Io) -> Result<(u16, String), Error> {
    let mut text = String::new();
    loop {
        let mut line = String::new();
        let n = io
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Smtp(e.to_string()))?;
        if n == 0 {
            return Err(Error::Smtp("connection closed".into()));
        }
        if line.len() > LINE_LIMIT {
            return Err(Error::Smtp("smtp reply too long".into()));
        }
        if line.len() < 4 {
            return Err(Error::Smtp(format!("short smtp reply {line:?}")));
        }
        let code: u16 = line[..3]
            .parse()
            .map_err(|_| Error::Smtp(format!("bad smtp code {line:?}")))?;
        let sep = line.as_bytes()[3];
        text.push_str(&line);
        if sep == b' ' {
            return Ok((code, text));
        }
        if sep != b'-' {
            return Err(Error::Smtp(format!("bad smtp reply {line:?}")));
        }
    }
}

fn expect_code(code: u16, range: std::ops::Range<u16>, text: &str) -> Result<(), Error> {
    if range.contains(&code) {
        Ok(())
    } else {
        let one = text.lines().next().unwrap_or(text).trim();
        Err(Error::Smtp(one.to_string()))
    }
}

fn ehlo_has_starttls(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim();
        l.len() >= 4 && l[4..].trim().eq_ignore_ascii_case("starttls")
    })
}

fn ehlo_name() -> String {
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

fn smtp_data_payload(raw: &[u8]) -> Vec<u8> {
    let n = normalize_crlf(raw);
    let mut out = Vec::with_capacity(n.len() + 8);
    let mut at_bol = true;
    for &b in &n {
        if at_bol && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        at_bol = b == b'\n';
    }
    if !n.ends_with(b"\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b".\r\n");
    out
}

fn normalize_crlf(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 8);
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'\r' => {
                out.extend_from_slice(b"\r\n");
                if i + 1 < raw.len() && raw[i + 1] == b'\n' {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            b'\n' => {
                out.extend_from_slice(b"\r\n");
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
pub fn with_outbox_stub<R>(mx: &str, port: u16, f: impl FnOnce() -> R) -> R {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let prev_mx = std::env::var_os("GOBLIND_OUTBOX_MX");
    let prev_port = std::env::var_os("GOBLIND_OUTBOX_SMTP_PORT");
    unsafe {
        std::env::set_var("GOBLIND_OUTBOX_MX", mx);
        std::env::set_var("GOBLIND_OUTBOX_SMTP_PORT", port.to_string());
    }
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    unsafe {
        match prev_mx {
            Some(v) => std::env::set_var("GOBLIND_OUTBOX_MX", v),
            None => std::env::remove_var("GOBLIND_OUTBOX_MX"),
        }
        match prev_port {
            Some(v) => std::env::set_var("GOBLIND_OUTBOX_SMTP_PORT", v),
            None => std::env::remove_var("GOBLIND_OUTBOX_SMTP_PORT"),
        }
    }
    drop(guard);
    match out {
        Ok(v) => v,
        Err(p) => std::panic::resume_unwind(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::paths::{self, with_goblind_home};
    use tokio::net::TcpListener;

    async fn mock_smtp(
        sessions: usize,
        reject_fail: bool,
    ) -> (u16, tokio::sync::oneshot::Receiver<Vec<u8>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let mut last = Vec::new();
            for _ in 0..sessions {
                let (s, _) = l.accept().await.unwrap();
                last = run_mock_session(s, reject_fail).await;
            }
            let _ = tx.send(last);
        });
        (port, rx)
    }

    async fn run_mock_session(s: TcpStream, reject_fail: bool) -> Vec<u8> {
        let mut r = BufReader::new(s);
        let mut got = Vec::new();
        write_line(&mut r, "220 mock ESMTP").await;
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                break;
            }
            let u = line.to_ascii_uppercase();
            if u.starts_with("EHLO") || u.starts_with("HELO") {
                write_line(&mut r, "250-mock hello").await;
                write_line(&mut r, "250 PIPELINING").await;
            } else if u.starts_with("MAIL") {
                write_line(&mut r, "250 2.1.0 OK").await;
            } else if u.starts_with("RCPT") {
                if reject_fail && u.contains("FAIL@") {
                    write_line(&mut r, "550 5.1.1 no").await;
                } else {
                    write_line(&mut r, "250 2.1.5 OK").await;
                }
            } else if u.starts_with("DATA") {
                write_line(&mut r, "354 go").await;
                loop {
                    let mut chunk = String::new();
                    r.read_line(&mut chunk).await.unwrap();
                    if chunk == ".\r\n" || chunk == ".\n" {
                        break;
                    }
                    let add = if let Some(rest) = chunk.strip_prefix('.') {
                        rest.as_bytes()
                    } else {
                        chunk.as_bytes()
                    };
                    got.extend_from_slice(add);
                }
                write_line(&mut r, "250 2.0.0 queued").await;
            } else if u.starts_with("QUIT") {
                write_line(&mut r, "221 bye").await;
                break;
            } else {
                write_line(&mut r, "500 no").await;
            }
        }
        got
    }

    async fn write_line(r: &mut BufReader<TcpStream>, line: &str) {
        r.get_mut().write_all(line.as_bytes()).await.unwrap();
        r.get_mut().write_all(b"\r\n").await.unwrap();
        r.get_mut().flush().await.unwrap();
    }

    #[test]
    fn delivers_to_local_mock_and_completes() {
        crate::tls::install_crypto();
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let (port, rx) = rt.block_on(mock_smtp(1, false));
            let raw = b"From: a@ex.com\r\nTo: b@ex.com\r\nSubject: hi\r\n\r\nhello\r\n";
            let id = queue::enqueue("a@ex.com", &["b@ex.com".into()], None, raw).unwrap();
            with_outbox_stub("127.0.0.1", port, || {
                rt.block_on(async {
                    tokio::time::timeout(Duration::from_secs(8), tick())
                        .await
                        .unwrap()
                        .unwrap();
                });
            });
            assert!(!queue::json_path(&id).exists());
            let got = rt.block_on(rx).unwrap();
            let text = String::from_utf8_lossy(&got);
            assert!(text.contains("hello"), "{text}");
            assert!(text.contains("Subject: hi"), "{text}");
        });
    }

    #[test]
    fn partial_rcpt_stays_queued() {
        crate::tls::install_crypto();
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let (port, _rx) = rt.block_on(mock_smtp(2, true));
            let raw = b"From: a@ex.com\r\nTo: ok@ex.com, fail@ex.com\r\n\r\nx\r\n";
            let id = queue::enqueue(
                "a@ex.com",
                &["ok@ex.com".into(), "fail@ex.com".into()],
                None,
                raw,
            )
            .unwrap();
            with_outbox_stub("127.0.0.1", port, || {
                rt.block_on(async {
                    tokio::time::timeout(Duration::from_secs(8), tick())
                        .await
                        .unwrap()
                        .unwrap();
                });
            });
            let jobs = queue::list_due_jobs().unwrap();
            assert!(jobs.is_empty(), "{jobs:?}");
            let text = std::fs::read_to_string(queue::json_path(&id)).unwrap();
            assert!(text.contains("fail@ex.com"), "{text}");
            assert!(!text.contains("ok@ex.com"), "{text}");
            assert!(text.contains("\"attempts\": 1"), "{text}");
        });
    }

    #[test]
    fn exhausted_attempts_go_to_failed() {
        crate::tls::install_crypto();
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let (port, _rx) = rt.block_on(mock_smtp(1, true));
            let id = queue::enqueue(
                "a@ex.com",
                &["fail@ex.com".into()],
                None,
                b"From: a@ex.com\r\n\r\nx\r\n",
            )
            .unwrap();
            let mut job = queue::list_due_jobs().unwrap().pop().unwrap();
            job.attempts = MAX_ATTEMPTS - 1;
            queue::save_job(&job).unwrap();
            with_outbox_stub("127.0.0.1", port, || {
                rt.block_on(async {
                    tokio::time::timeout(Duration::from_secs(8), tick())
                        .await
                        .unwrap()
                        .unwrap();
                });
            });
            assert!(!queue::json_path(&id).exists());
            assert!(paths::queue_failed_dir()
                .join(format!("{id}.json"))
                .is_file());
            let log = std::fs::read_to_string(paths::queue_dir().join("failed.log")).unwrap();
            assert!(log.contains(&id), "{log}");
        });
    }

    #[test]
    fn signs_when_key_and_domain_match() {
        crate::tls::install_crypto();
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            crate::server::config::save(&crate::server::config::AccountFile {
                domain: "vanguardaautomovel.com".into(),
                users: vec![],
            })
            .unwrap();
            dkim::init(dkim::DEFAULT_SELECTOR).unwrap();
            let rt = tokio::runtime::Runtime::new().unwrap();
            let (port, rx) = rt.block_on(mock_smtp(1, false));
            let raw = b"From: design@vanguardaautomovel.com\r\n\
To: out@example.com\r\n\
Subject: signed\r\n\r\nhello world\r\n";
            queue::enqueue(
                "design@vanguardaautomovel.com",
                &["out@example.com".into()],
                None,
                raw,
            )
            .unwrap();
            with_outbox_stub("127.0.0.1", port, || {
                rt.block_on(async {
                    tokio::time::timeout(Duration::from_secs(15), tick())
                        .await
                        .unwrap()
                        .unwrap();
                });
            });
            let got = rt.block_on(rx).unwrap();
            let text = String::from_utf8_lossy(&got);
            assert!(text.contains("DKIM-Signature:"), "{text}");
            assert!(text.contains("s=goblin"), "{text}");
            assert!(text.contains("hello world"), "{text}");
        });
    }
}
