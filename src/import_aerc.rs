//! One-shot aerc accounts.conf import. Password is returned, never written to JSON.

use crate::config::{Account, Endpoint};
use crate::error::Error;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Imported {
    pub account: Account,
    pub password: String,
}

pub fn import_aerc(path: &Path) -> Result<Imported, Error> {
    let text = std::fs::read_to_string(path)?;
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut cur: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            cur = Some(line[1..line.len() - 1].to_string());
            sections
                .entry(cur.clone().unwrap())
                .or_default();
            continue;
        }
        let Some(name) = &cur else { continue };
        if let Some((k, v)) = line.split_once('=') {
            sections
                .entry(name.clone())
                .or_default()
                .insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let (name, acc) = sections
        .into_iter()
        .next()
        .ok_or_else(|| Error::Config("no accounts in aerc config".into()))?;
    let source = acc
        .get("source")
        .ok_or_else(|| Error::Config("aerc account has no source".into()))?;
    let parsed = parse_imap_url(source)?;
    let from = acc.get("from").cloned().unwrap_or_default();
    let smtp_host = parsed.host_guess_smtp();
    let smtp_user = parsed.user.clone();
    let account = Account {
        name,
        from,
        imap: Endpoint {
            host: parsed.host,
            port: parsed.port,
            user: parsed.user,
        },
        smtp: Endpoint {
            host: smtp_host,
            port: 465,
            user: smtp_user,
        },
    };
    Ok(Imported {
        account,
        password: parsed.password,
    })
}

struct ParsedUrl {
    host: String,
    port: u16,
    user: String,
    password: String,
}

impl ParsedUrl {
    fn host_guess_smtp(&self) -> String {
        if self.host.starts_with("imap.") {
            format!("smtp.{}", self.host.trim_start_matches("imap."))
        } else {
            self.host.clone()
        }
    }
}

fn parse_imap_url(src: &str) -> Result<ParsedUrl, Error> {
    let src = src.trim();
    let (ssl, rest) = if let Some(r) = src.strip_prefix("imaps://") {
        (true, r)
    } else if let Some(r) = src.strip_prefix("imap://") {
        (false, r)
    } else {
        return Err(Error::Config(format!("unsupported source: {src}")));
    };
    // user:pass@host:port
    let (creds, hostport) = rest
        .rsplit_once('@')
        .ok_or_else(|| Error::Config("aerc source missing @host".into()))?;
    let (user, password) = creds
        .split_once(':')
        .ok_or_else(|| Error::Config("aerc source missing password".into()))?;
    let (host, port) = if let Some((h, p)) = hostport.rsplit_once(':') {
        let p: u16 = p
            .parse()
            .map_err(|_| Error::Config("bad aerc port".into()))?;
        (h, p)
    } else {
        (hostport, if ssl { 993 } else { 143 })
    };
    Ok(ParsedUrl {
        host: percent_decode(host),
        port,
        user: percent_decode(user),
        password: percent_decode(password),
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parses_fixture_and_does_not_put_password_in_account() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("accounts.conf");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "[Personal]\nsource = imaps://user:s3cret@imap.example:993\nfrom = User <user@example.com>\n"
        )
        .unwrap();
        let imported = import_aerc(&path).unwrap();
        assert_eq!(imported.account.imap.host, "imap.example");
        assert_eq!(imported.account.imap.user, "user");
        assert_eq!(imported.account.imap.port, 993);
        assert_eq!(imported.password, "s3cret");
        let json = serde_json::to_string(&imported.account).unwrap();
        assert!(
            !json.contains("s3cret"),
            "password leaked into account json: {json}"
        );
        assert!(!json.contains("password"));
    }
}
