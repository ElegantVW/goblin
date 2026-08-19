//! IMAP4rev1 over implicit TLS. Enough for the goblin client. No Dovecot.

use super::auth;
use super::config;
use super::maildir::{self, Entry, FlagMode};
use super::paths;
use crate::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

const LINE_LIMIT: usize = 32 * 1024;
const CAP_PRE: &str = "IMAP4rev1 AUTH=PLAIN AUTH=LOGIN";
const CAP_POST: &str = "IMAP4rev1 UIDPLUS IDLE";

pub async fn accept_loop(listener: TcpListener, tls: Arc<rustls::ServerConfig>) {
    let acceptor = TlsAcceptor::from(tls);
    loop {
        match listener.accept().await {
            Ok((sock, peer)) => {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle(sock, acceptor).await {
                        eprintln!("goblind: imap {peer}: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("goblind: imap accept: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

async fn handle(sock: TcpStream, acceptor: TlsAcceptor) -> Result<(), Error> {
    let _ = sock.set_nodelay(true);
    let tls = acceptor
        .accept(sock)
        .await
        .map_err(|e| Error::TlsPolicy(format!("imap tls: {e}")))?;
    session(BufReader::new(tls)).await
}

struct Selected {
    root: PathBuf,
}

async fn session<S>(mut io: BufReader<S>) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_line(
        &mut io,
        &format!("* OK [CAPABILITY {CAP_PRE}] goblind ready"),
    )
    .await?;
    let mut user: Option<String> = None;
    let mut selected: Option<Selected> = None;
    loop {
        let mut line = String::new();
        let n = io
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        if n == 0 {
            return Ok(());
        }
        if line.len() > LINE_LIMIT {
            write_line(&mut io, "* BYE line too long").await?;
            return Ok(());
        }
        let raw = line.trim_end_matches(['\r', '\n']);
        if raw.is_empty() {
            continue;
        }
        let tokens = match tokenize(raw) {
            Ok(t) if !t.is_empty() => t,
            _ => {
                write_line(&mut io, "* BAD protocol error").await?;
                continue;
            }
        };
        let tag = tokens[0].clone();
        if tokens.len() < 2 {
            tagged(&mut io, &tag, "BAD missing command").await?;
            continue;
        }
        let cmd = tokens[1].to_ascii_uppercase();
        match cmd.as_str() {
            "CAPABILITY" => {
                let cap = if user.is_some() { CAP_POST } else { CAP_PRE };
                write_line(&mut io, &format!("* CAPABILITY {cap}")).await?;
                tagged(&mut io, &tag, "OK CAPABILITY completed").await?;
            }
            "NOOP" => tagged(&mut io, &tag, "OK NOOP completed").await?,
            "LOGOUT" => {
                write_line(&mut io, "* BYE goblind").await?;
                tagged(&mut io, &tag, "OK LOGOUT completed").await?;
                return Ok(());
            }
            "LOGIN" => {
                if user.is_some() {
                    tagged(&mut io, &tag, "NO already logged in").await?;
                    continue;
                }
                if tokens.len() < 4 {
                    tagged(&mut io, &tag, "BAD LOGIN needs user and password").await?;
                    continue;
                }
                match auth::verify_login(&tokens[2], &tokens[3]) {
                    Ok(Some(addr)) => {
                        user = Some(addr);
                        tagged(&mut io, &tag, "OK LOGIN completed").await?;
                    }
                    Ok(None) => {
                        tagged(&mut io, &tag, "NO [AUTHENTICATIONFAILED] LOGIN failed").await?
                    }
                    Err(e) => {
                        eprintln!("goblind: imap login: {e}");
                        tagged(&mut io, &tag, "NO LOGIN failed").await?;
                    }
                }
            }
            "LIST" | "LSUB" => {
                if user.is_none() {
                    tagged(&mut io, &tag, "NO not authenticated").await?;
                    continue;
                }
                write_line(&mut io, r#"* LIST (\HasNoChildren) "/" INBOX"#).await?;
                tagged(&mut io, &tag, "OK LIST completed").await?;
            }
            "SELECT" | "EXAMINE" => {
                let Some(addr) = user.as_deref() else {
                    tagged(&mut io, &tag, "NO not authenticated").await?;
                    continue;
                };
                if tokens.len() < 3 || !is_inbox(&tokens[2]) {
                    tagged(&mut io, &tag, "NO unknown mailbox").await?;
                    continue;
                }
                match mailbox_root(addr) {
                    Ok(root) => {
                        let entries = maildir::indexed(&root)?;
                        let uv = maildir::uidvalidity(&root)?;
                        let un = maildir::uidnext(&root)?;
                        let exists = entries.len();
                        let recent = entries.iter().filter(|e| e.recent).count();
                        let unseen = entries.iter().position(|e| !e.seen()).map(|i| i + 1);
                        write_line(
                            &mut io,
                            r#"* FLAGS (\Answered \Flagged \Deleted \Seen \Draft)"#,
                        )
                        .await?;
                        write_line(
                            &mut io,
                            r#"* OK [PERMANENTFLAGS (\Answered \Flagged \Deleted \Seen \Draft \*)] Flags permitted"#,
                        )
                        .await?;
                        write_line(&mut io, &format!("* {exists} EXISTS")).await?;
                        write_line(&mut io, &format!("* {recent} RECENT")).await?;
                        if let Some(u) = unseen {
                            write_line(&mut io, &format!("* OK [UNSEEN {u}] first unseen")).await?;
                        }
                        write_line(&mut io, &format!("* OK [UIDVALIDITY {uv}] UIDs valid")).await?;
                        write_line(&mut io, &format!("* OK [UIDNEXT {un}] predicted next UID"))
                            .await?;
                        selected = Some(Selected { root });
                        let mode = if cmd == "EXAMINE" {
                            "READ-ONLY"
                        } else {
                            "READ-WRITE"
                        };
                        tagged(&mut io, &tag, &format!("OK [{mode}] SELECT completed")).await?;
                    }
                    Err(e) => {
                        eprintln!("goblind: imap select: {e}");
                        tagged(&mut io, &tag, "NO SELECT failed").await?;
                    }
                }
            }
            "UID" => {
                let Some(sel) = selected.as_ref() else {
                    tagged(&mut io, &tag, "NO no mailbox selected").await?;
                    continue;
                };
                if tokens.len() < 3 {
                    tagged(&mut io, &tag, "BAD UID needs a subcommand").await?;
                    continue;
                }
                let sub = tokens[2].to_ascii_uppercase();
                match sub.as_str() {
                    "SEARCH" => uid_search(&mut io, &tag, &sel.root, &tokens[3..]).await?,
                    "FETCH" => uid_fetch(&mut io, &tag, &sel.root, &tokens[3..]).await?,
                    "STORE" => uid_store(&mut io, &tag, &sel.root, &tokens[3..]).await?,
                    "COPY" => uid_copy(&mut io, &tag, &tokens[3..]).await?,
                    _ => tagged(&mut io, &tag, "BAD unknown UID command").await?,
                }
            }
            "SEARCH" => {
                let Some(sel) = selected.as_ref() else {
                    tagged(&mut io, &tag, "NO no mailbox selected").await?;
                    continue;
                };
                seq_search(&mut io, &tag, &sel.root, &tokens[2..]).await?;
            }
            "FETCH" => {
                let Some(sel) = selected.as_ref() else {
                    tagged(&mut io, &tag, "NO no mailbox selected").await?;
                    continue;
                };
                seq_fetch(&mut io, &tag, &sel.root, &tokens[2..]).await?;
            }
            "STORE" => {
                let Some(sel) = selected.as_ref() else {
                    tagged(&mut io, &tag, "NO no mailbox selected").await?;
                    continue;
                };
                seq_store(&mut io, &tag, &sel.root, &tokens[2..]).await?;
            }
            "EXPUNGE" => {
                let Some(sel) = selected.as_ref() else {
                    tagged(&mut io, &tag, "NO no mailbox selected").await?;
                    continue;
                };
                let entries = maildir::indexed(&sel.root)?;
                let mut seqs: Vec<usize> = entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.deleted())
                    .map(|(i, _)| i + 1)
                    .collect();
                seqs.reverse();
                maildir::expunge_deleted(&sel.root)?;
                for s in seqs {
                    write_line(&mut io, &format!("* {s} EXPUNGE")).await?;
                }
                tagged(&mut io, &tag, "OK EXPUNGE completed").await?;
            }
            "CLOSE" => {
                if let Some(sel) = selected.take() {
                    let _ = maildir::expunge_deleted(&sel.root);
                }
                tagged(&mut io, &tag, "OK CLOSE completed").await?;
            }
            "IDLE" => {
                if user.is_none() {
                    tagged(&mut io, &tag, "NO not authenticated").await?;
                    continue;
                }
                write_line(&mut io, "+ idling").await?;
                loop {
                    let mut idle_line = String::new();
                    let n = io
                        .read_line(&mut idle_line)
                        .await
                        .map_err(|e| Error::Imap(e.to_string()))?;
                    if n == 0 {
                        return Ok(());
                    }
                    if idle_line.trim().eq_ignore_ascii_case("DONE") {
                        break;
                    }
                }
                tagged(&mut io, &tag, "OK IDLE completed").await?;
            }
            "AUTHENTICATE" => {
                tagged(&mut io, &tag, "NO AUTHENTICATE not offered; use LOGIN").await?
            }
            _ => tagged(&mut io, &tag, "BAD unknown command").await?,
        }
    }
}

async fn uid_search<S>(
    io: &mut BufReader<S>,
    tag: &str,
    root: &std::path::Path,
    args: &[String],
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let entries = maildir::indexed(root)?;
    let unseen = args.iter().any(|a| a.eq_ignore_ascii_case("UNSEEN"));
    let uids: Vec<String> = entries
        .iter()
        .filter(|e| !unseen || !e.seen())
        .map(|e| e.uid.to_string())
        .collect();
    if uids.is_empty() {
        write_line(io, "* SEARCH").await?;
    } else {
        write_line(io, &format!("* SEARCH {}", uids.join(" "))).await?;
    }
    tagged(io, tag, "OK UID SEARCH completed").await
}

async fn seq_search<S>(
    io: &mut BufReader<S>,
    tag: &str,
    root: &std::path::Path,
    args: &[String],
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let entries = maildir::indexed(root)?;
    let unseen = args.iter().any(|a| a.eq_ignore_ascii_case("UNSEEN"));
    let seqs: Vec<String> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !unseen || !e.seen())
        .map(|(i, _)| (i + 1).to_string())
        .collect();
    if seqs.is_empty() {
        write_line(io, "* SEARCH").await?;
    } else {
        write_line(io, &format!("* SEARCH {}", seqs.join(" "))).await?;
    }
    tagged(io, tag, "OK SEARCH completed").await
}

async fn uid_fetch<S>(
    io: &mut BufReader<S>,
    tag: &str,
    root: &std::path::Path,
    args: &[String],
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if args.is_empty() {
        return tagged(io, tag, "BAD FETCH needs a UID set").await;
    }
    let entries = maildir::indexed(root)?;
    let un = maildir::uidnext(root)?;
    let uids = parse_uidset(&args[0], &entries, un);
    let items = fetch_items(&args[1..]);
    for uid in uids {
        if let Some((seq, e)) = find_uid(&entries, uid) {
            write_fetch(io, seq, e, &items).await?;
        }
    }
    tagged(io, tag, "OK UID FETCH completed").await
}

async fn seq_fetch<S>(
    io: &mut BufReader<S>,
    tag: &str,
    root: &std::path::Path,
    args: &[String],
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if args.is_empty() {
        return tagged(io, tag, "BAD FETCH needs a sequence set").await;
    }
    let entries = maildir::indexed(root)?;
    let seqs = parse_seqset(&args[0], entries.len());
    let items = fetch_items(&args[1..]);
    for seq in seqs {
        if let Some(e) = entries.get(seq.saturating_sub(1)) {
            write_fetch(io, seq, e, &items).await?;
        }
    }
    tagged(io, tag, "OK FETCH completed").await
}

async fn write_fetch<S>(
    io: &mut BufReader<S>,
    seq: usize,
    entry: &Entry,
    items: &[FetchItem],
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut entry = entry.clone();
    let want_body = items.iter().any(|i| matches!(i, FetchItem::Body { .. }));
    let set_seen = items
        .iter()
        .any(|i| matches!(i, FetchItem::Body { peek: false }));
    if want_body && entry.recent {
        entry = maildir::to_cur(&entry)?;
    }
    if set_seen && !entry.seen() {
        entry = maildir::store_flags(&entry, FlagMode::Add, &["\\Seen"])?;
    }
    let mut attrs = Vec::new();
    let mut body: Option<(bool, Vec<u8>)> = None;
    for item in items {
        match item {
            FetchItem::Uid => attrs.push(format!("UID {}", entry.uid)),
            FetchItem::Flags => attrs.push(format!("FLAGS ({})", entry.imap_flags())),
            FetchItem::Rfc822Size => {
                let n = std::fs::metadata(&entry.path).map(|m| m.len()).unwrap_or(0);
                attrs.push(format!("RFC822.SIZE {n}"));
            }
            FetchItem::Body { peek: _ } => {
                if body.is_none() {
                    body = Some((false, maildir::read_bytes(&entry)?));
                }
            }
            FetchItem::Rfc822 => {
                body = Some((true, maildir::read_bytes(&entry)?));
            }
        }
    }
    if let Some((rfc822, raw)) = body {
        let name = if rfc822 { "RFC822" } else { "BODY[]" };
        let prefix = if attrs.is_empty() {
            String::new()
        } else {
            format!("{} ", attrs.join(" "))
        };
        // Literal: `{n}\r\n` immediately precedes the bytes; `)` follows the payload.
        let head = format!("* {seq} FETCH ({prefix}{name} {{{}}}", raw.len());
        write_line(io, &head).await?;
        io.get_mut()
            .write_all(&raw)
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        io.get_mut()
            .write_all(b")\r\n")
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
        io.get_mut()
            .flush()
            .await
            .map_err(|e| Error::Imap(e.to_string()))?;
    } else {
        write_line(io, &format!("* {seq} FETCH ({})", attrs.join(" "))).await?;
    }
    Ok(())
}

async fn uid_store<S>(
    io: &mut BufReader<S>,
    tag: &str,
    root: &std::path::Path,
    args: &[String],
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    store_inner(io, tag, root, args, true).await
}

async fn seq_store<S>(
    io: &mut BufReader<S>,
    tag: &str,
    root: &std::path::Path,
    args: &[String],
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    store_inner(io, tag, root, args, false).await
}

async fn store_inner<S>(
    io: &mut BufReader<S>,
    tag: &str,
    root: &std::path::Path,
    args: &[String],
    by_uid: bool,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if args.len() < 2 {
        return tagged(io, tag, "BAD STORE needs a set and flags").await;
    }
    let entries = maildir::indexed(root)?;
    let targets: Vec<(usize, Entry)> = if by_uid {
        let un = maildir::uidnext(root)?;
        parse_uidset(&args[0], &entries, un)
            .into_iter()
            .filter_map(|u| find_uid(&entries, u).map(|(s, e)| (s, e.clone())))
            .collect()
    } else {
        parse_seqset(&args[0], entries.len())
            .into_iter()
            .filter_map(|s| entries.get(s.saturating_sub(1)).map(|e| (s, e.clone())))
            .collect()
    };
    let item = args[1].to_ascii_uppercase();
    let silent = item.contains("SILENT");
    let mode = if item.starts_with('+') {
        FlagMode::Add
    } else if item.starts_with('-') {
        FlagMode::Remove
    } else {
        FlagMode::Replace
    };
    let flags: Vec<&str> = args[2..]
        .iter()
        .map(String::as_str)
        .filter(|s| {
            s.starts_with('\\')
                || s.eq_ignore_ascii_case("seen")
                || s.eq_ignore_ascii_case("deleted")
        })
        .collect();
    for (seq, e) in targets {
        let updated = maildir::store_flags(&e, mode, &flags)?;
        if !silent {
            write_line(
                io,
                &format!(
                    "* {seq} FETCH (UID {} FLAGS ({}))",
                    updated.uid,
                    updated.imap_flags()
                ),
            )
            .await?;
        }
    }
    tagged(io, tag, "OK STORE completed").await
}

async fn uid_copy<S>(io: &mut BufReader<S>, tag: &str, args: &[String]) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if args.len() < 2 {
        return tagged(io, tag, "BAD COPY needs a UID set and mailbox").await;
    }
    if is_inbox(&args[1]) {
        tagged(io, tag, "OK UID COPY completed").await
    } else {
        tagged(io, tag, "NO [TRYCREATE] unknown mailbox").await
    }
}

#[derive(Debug, Clone)]
enum FetchItem {
    Uid,
    Flags,
    Rfc822Size,
    Rfc822,
    Body { peek: bool },
}

fn fetch_items(args: &[String]) -> Vec<FetchItem> {
    let mut out = Vec::new();
    if args.is_empty() {
        out.push(FetchItem::Flags);
        out.push(FetchItem::Uid);
        out.push(FetchItem::Body { peek: true });
        return out;
    }
    for a in args {
        let u = a.to_ascii_uppercase();
        if u == "UID" {
            out.push(FetchItem::Uid);
        } else if u == "FLAGS" {
            out.push(FetchItem::Flags);
        } else if u == "RFC822.SIZE" {
            out.push(FetchItem::Rfc822Size);
        } else if u == "RFC822" {
            out.push(FetchItem::Rfc822);
        } else if u.starts_with("BODY.PEEK") || u == "RFC822.PEEK" {
            out.push(FetchItem::Body { peek: true });
        } else if u.starts_with("BODY") {
            out.push(FetchItem::Body { peek: false });
        }
    }
    if out.is_empty() {
        out.push(FetchItem::Uid);
        out.push(FetchItem::Body { peek: true });
    }
    out
}

fn find_uid(entries: &[Entry], uid: u32) -> Option<(usize, &Entry)> {
    entries
        .iter()
        .enumerate()
        .find(|(_, e)| e.uid == uid)
        .map(|(i, e)| (i + 1, e))
}

fn parse_uidset(spec: &str, entries: &[Entry], uidnext: u32) -> Vec<u32> {
    let max = uidnext.saturating_sub(1);
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once(':') {
            let start = parse_star(a, max);
            let end = parse_star(b, max);
            let (lo, hi) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            for e in entries {
                if e.uid >= lo && e.uid <= hi {
                    out.push(e.uid);
                }
            }
        } else {
            let n = parse_star(part, max);
            if entries.iter().any(|e| e.uid == n) {
                out.push(n);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn parse_seqset(spec: &str, exists: usize) -> Vec<usize> {
    let max = exists;
    let mut out = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once(':') {
            let start = parse_star(a, max as u32) as usize;
            let end = parse_star(b, max as u32) as usize;
            let (lo, hi) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            for n in lo.max(1)..=hi.min(max) {
                out.push(n);
            }
        } else {
            let n = parse_star(part, max as u32) as usize;
            if n >= 1 && n <= max {
                out.push(n);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn parse_star(s: &str, max: u32) -> u32 {
    if s == "*" {
        max
    } else {
        s.parse().unwrap_or(0)
    }
}

fn mailbox_root(addr: &str) -> Result<PathBuf, Error> {
    let file = config::load()?;
    let u = file.user(addr)?;
    Ok(paths::user_maildir(&u.maildir))
}

fn is_inbox(name: &str) -> bool {
    name.eq_ignore_ascii_case("inbox") || name.eq_ignore_ascii_case("\"inbox\"")
}

async fn tagged<S>(io: &mut BufReader<S>, tag: &str, rest: &str) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_line(io, &format!("{tag} {rest}")).await
}

async fn write_line<S>(io: &mut BufReader<S>, line: &str) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    io.get_mut()
        .write_all(line.as_bytes())
        .await
        .map_err(|e| Error::Imap(e.to_string()))?;
    io.get_mut()
        .write_all(b"\r\n")
        .await
        .map_err(|e| Error::Imap(e.to_string()))?;
    io.get_mut()
        .flush()
        .await
        .map_err(|e| Error::Imap(e.to_string()))?;
    Ok(())
}

fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '"' {
            chars.next();
            let mut tok = String::new();
            loop {
                match chars.next() {
                    None => return Err("unterminated string".into()),
                    Some('\\') => {
                        if let Some(n) = chars.next() {
                            tok.push(n);
                        }
                    }
                    Some('"') => break,
                    Some(x) => tok.push(x),
                }
            }
            out.push(tok);
        } else if c == '(' {
            chars.next();
            let mut depth = 1;
            let mut inner = String::new();
            while depth > 0 {
                match chars.next() {
                    None => return Err("unterminated list".into()),
                    Some('(') => {
                        depth += 1;
                        inner.push('(');
                    }
                    Some(')') => {
                        depth -= 1;
                        if depth > 0 {
                            inner.push(')');
                        }
                    }
                    Some(x) => inner.push(x),
                }
            }
            out.extend(tokenize(&inner)?);
        } else {
            let mut tok = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '(' || c == ')' {
                    break;
                }
                tok.push(c);
                chars.next();
            }
            out.push(tok);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_login_and_fetch() {
        let t = tokenize(r#"A0001 LOGIN "design@x.com" "s3cret""#).unwrap();
        assert_eq!(t, ["A0001", "LOGIN", "design@x.com", "s3cret"]);
        let t = tokenize("A0002 UID FETCH 1 (UID BODY.PEEK[])").unwrap();
        assert_eq!(t, ["A0002", "UID", "FETCH", "1", "UID", "BODY.PEEK[]"]);
        let t = tokenize(r#"A0003 UID STORE 1 +FLAGS.SILENT (\Seen)"#).unwrap();
        assert_eq!(t, ["A0003", "UID", "STORE", "1", "+FLAGS.SILENT", "\\Seen"]);
    }
}
