//! Our DKIM (selector `goblin`). No OpenDKIM.

use super::paths;
use crate::error::Error;
use crate::fsutil;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPublicKey};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::signature::{SignatureEncoding, Signer};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const DEFAULT_SELECTOR: &str = "goblin";

const SIGN_HEADERS: &[&str] = &[
    "from",
    "to",
    "subject",
    "date",
    "message-id",
    "mime-version",
    "content-type",
];

pub fn private_path(selector: &str) -> PathBuf {
    paths::dkim_dir().join(format!("{selector}.private.pem"))
}

pub fn public_path(selector: &str) -> PathBuf {
    paths::dkim_dir().join(format!("{selector}.txt"))
}

pub fn init(selector: &str) -> Result<(), Error> {
    let selector = validate_selector(selector)?;
    paths::ensure_layout()?;
    let priv_path = private_path(selector);
    if priv_path.is_file() {
        return Err(Error::say(
            format!("DKIM key already exists: {}", priv_path.display()),
            "remove that file to rotate",
        ));
    }
    let mut rng = rand::rngs::OsRng;
    let key = RsaPrivateKey::new(&mut rng, 2048)
        .map_err(|e| Error::Config(format!("dkim keygen: {e}")))?;
    let pem = key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| Error::Config(format!("dkim pem: {e}")))?;
    fsutil::write_private(&priv_path, pem.as_bytes())?;
    let p = public_p(&key)?;
    let rec = format!("v=DKIM1; k=rsa; p={p}\n");
    fsutil::write_private(&public_path(selector), rec.as_bytes())?;
    Ok(())
}

pub fn public_record(selector: &str) -> Option<String> {
    let path = public_path(selector);
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// List `(selector, TXT value)` for every `dkim/*.txt` that has a record.
pub fn public_records() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let dir = paths::dkim_dir();
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return out,
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.extension().and_then(|s| s.to_str()) != Some("txt") {
            continue;
        }
        let Some(sel) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(rec) = public_record(sel) {
            out.push((sel.to_string(), rec));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Insert a `DKIM-Signature` (relaxed/relaxed, rsa-sha256).
pub fn sign(domain: &str, selector: &str, pem: &str, message: &[u8]) -> Result<Vec<u8>, Error> {
    let selector = validate_selector(selector)?;
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return Err(Error::Config("dkim sign: empty domain".into()));
    }
    let key = load_private_pem(pem)?;
    let (head, body) = split_message(message);
    let fields = parse_headers(head);
    if find_header(&fields, "from").is_none() {
        return Err(Error::Config("dkim sign: missing From header".into()));
    }
    let mut signed: Vec<(String, Vec<u8>)> = Vec::new();
    for name in SIGN_HEADERS {
        if let Some((orig, value)) = find_header(&fields, name) {
            signed.push((orig, value.to_vec()));
        }
    }
    let h_tag = signed
        .iter()
        .map(|(n, _)| n.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(":");
    let bh = body_hash_b64(body);
    let t = unix_now();
    let empty_b = format!(
        "v=1; a=rsa-sha256; c=relaxed/relaxed; d={domain}; s={selector}; t={t}; bh={bh}; h={h_tag}; b="
    );
    let mut to_sign = Vec::new();
    for (name, value) in &signed {
        to_sign.extend(relaxed_header(name, value));
    }
    to_sign.extend(relaxed_header("dkim-signature", empty_b.as_bytes()));
    let signing_key = SigningKey::<Sha256>::new(key);
    let sig = signing_key
        .try_sign(&to_sign)
        .map_err(|e| Error::Config(format!("dkim sign: {e}")))?;
    let b = STANDARD.encode(sig.to_bytes());
    let mut hdr = format_dkim_header(&format!("{empty_b}{b}"));
    hdr.extend_from_slice(message);
    Ok(hdr)
}

pub fn body_hash_b64(body: &[u8]) -> String {
    let canon = relaxed_body(body);
    STANDARD.encode(Sha256::digest(canon))
}

fn public_p(key: &RsaPrivateKey) -> Result<String, Error> {
    let pubk = RsaPublicKey::from(key);
    let der = pubk
        .to_pkcs1_der()
        .map_err(|e| Error::Config(format!("dkim public: {e}")))?;
    Ok(STANDARD.encode(der.as_bytes()))
}

fn load_private_pem(pem: &str) -> Result<RsaPrivateKey, Error> {
    let pem = pem.trim();
    if let Ok(k) = RsaPrivateKey::from_pkcs8_pem(pem) {
        return Ok(k);
    }
    RsaPrivateKey::from_pkcs1_pem(pem).map_err(|e| Error::Config(format!("dkim key: {e}")))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn validate_selector(selector: &str) -> Result<&str, Error> {
    let s = selector.trim();
    if s.is_empty()
        || s.len() > 63
        || s.starts_with('-')
        || s.ends_with('-')
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(Error::Usage(format!("bad DKIM selector {selector:?}")));
    }
    Ok(s)
}

fn split_message(raw: &[u8]) -> (&[u8], &[u8]) {
    if let Some(i) = find_bytes(raw, b"\r\n\r\n") {
        return (&raw[..i], &raw[i + 4..]);
    }
    if let Some(i) = find_bytes(raw, b"\n\n") {
        return (&raw[..i], &raw[i + 2..]);
    }
    (raw, b"")
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn parse_headers(head: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut fields: Vec<(String, Vec<u8>)> = Vec::new();
    for line in split_logical_lines(head) {
        if line.is_empty() {
            continue;
        }
        let cont = line[0] == b' ' || line[0] == b'\t';
        if cont {
            if let Some((_, val)) = fields.last_mut() {
                val.extend_from_slice(&line);
            }
            continue;
        }
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        let name = String::from_utf8_lossy(&line[..colon]).into_owned();
        fields.push((name, line[colon + 1..].to_vec()));
    }
    fields
}

/// Last occurrence of a header (RFC 6376 §5.4.1).
fn find_header<'a>(fields: &'a [(String, Vec<u8>)], name: &str) -> Option<(String, &'a [u8])> {
    fields.iter().rev().find_map(|(n, v)| {
        if n.eq_ignore_ascii_case(name) {
            Some((n.clone(), v.as_slice()))
        } else {
            None
        }
    })
}

fn relaxed_header(name: &str, value: &[u8]) -> Vec<u8> {
    let mut val = collapse_wsp(value);
    while val.last() == Some(&b' ') {
        val.pop();
    }
    let start = val.iter().position(|&b| b != b' ').unwrap_or(val.len());
    val.drain(..start);
    let mut out = name.to_ascii_lowercase().into_bytes();
    out.push(b':');
    out.extend(val);
    out.extend_from_slice(b"\r\n");
    out
}

fn relaxed_body(body: &[u8]) -> Vec<u8> {
    let mut lines = split_logical_lines(body);
    for line in &mut lines {
        *line = collapse_wsp(line);
        while line.last() == Some(&b' ') {
            line.pop();
        }
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in lines {
        out.extend(line);
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn collapse_wsp(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut prev_wsp = false;
    for &b in input {
        if b == b' ' || b == b'\t' {
            if !prev_wsp {
                out.push(b' ');
                prev_wsp = true;
            }
        } else {
            out.push(b);
            prev_wsp = false;
        }
    }
    out
}

fn split_logical_lines(input: &[u8]) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        if input[i] == b'\n' {
            let mut end = i;
            if end > start && input[end - 1] == b'\r' {
                end -= 1;
            }
            lines.push(input[start..end].to_vec());
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    if start < input.len() {
        lines.push(input[start..].to_vec());
    }
    lines
}

fn format_dkim_header(value: &str) -> Vec<u8> {
    let mut s = String::from("DKIM-Signature: ");
    let mut col = s.len();
    for part in value.split_inclusive(' ') {
        if col + part.len() > 78 && col > 1 {
            s.push_str("\r\n ");
            col = 1;
        }
        s.push_str(part);
        col += part.len();
    }
    s.push_str("\r\n");
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::paths::with_goblind_home;
    use rsa::pkcs1v15::VerifyingKey;
    use rsa::signature::Verifier;

    fn sample_msg() -> &'static [u8] {
        b"From: Design <design@vanguardaautomovel.com>\r\n\
To: Outside <out@example.com>\r\n\
Subject: lab\r\n\
Date: Tue, 19 Aug 2025 12:00:00 +0000\r\n\
Message-ID: <lab@vanguardaautomovel.com>\r\n\
\r\n\
hello world\r\n"
    }

    #[test]
    fn fixed_body_hash_is_stable() {
        let expected = STANDARD.encode(Sha256::digest(b"hello world\r\n"));
        assert_eq!(body_hash_b64(b"hello world\r\n"), expected);
        assert_eq!(body_hash_b64(b"hello world\r\n\r\n\r\n"), expected);
        assert_eq!(body_hash_b64(b"hello   world   \r\n"), expected);
        assert_eq!(body_hash_b64(b"hello world"), expected);
        assert_eq!(body_hash_b64(split_message(sample_msg()).1), expected);
    }

    #[test]
    fn empty_body_hashes_empty() {
        assert_eq!(body_hash_b64(b""), STANDARD.encode(Sha256::digest(b"")));
        assert_eq!(
            body_hash_b64(b"\r\n\r\n"),
            STANDARD.encode(Sha256::digest(b""))
        );
    }

    #[test]
    fn init_and_sign_adds_verifiable_signature() {
        crate::tls::install_crypto();
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            init(DEFAULT_SELECTOR).unwrap();
            let priv_p = private_path(DEFAULT_SELECTOR);
            let pub_p = public_path(DEFAULT_SELECTOR);
            assert!(priv_p.is_file());
            assert!(pub_p.is_file());
            #[cfg(unix)]
            {
                assert_eq!(crate::fsutil::file_mode(&priv_p).unwrap(), Some(0o600));
            }
            let rec = std::fs::read_to_string(&pub_p).unwrap();
            assert!(rec.contains("v=DKIM1"));
            assert!(rec.contains("p="));
            assert!(!rec.contains("BEGIN"));
            let pem = std::fs::read_to_string(&priv_p).unwrap();
            let signed = sign(
                "vanguardaautomovel.com",
                DEFAULT_SELECTOR,
                &pem,
                sample_msg(),
            )
            .unwrap();
            let text = String::from_utf8_lossy(&signed);
            assert!(text.contains("DKIM-Signature:"), "{text}");
            assert!(text.contains("a=rsa-sha256"), "{text}");
            assert!(text.contains("c=relaxed/relaxed"), "{text}");
            assert!(text.contains("s=goblin"), "{text}");
            let bh = body_hash_b64(b"hello world\r\n");
            assert!(text.contains(&format!("bh={bh}")), "{text}");
            verify_signed("vanguardaautomovel.com", DEFAULT_SELECTOR, &pem, &signed);
            match init(DEFAULT_SELECTOR) {
                Err(Error::Hint { .. }) => {}
                other => panic!("{other:?}"),
            }
        });
    }

    fn verify_signed(domain: &str, selector: &str, pem: &str, signed: &[u8]) {
        let (head, body) = split_message(signed);
        let fields = parse_headers(head);
        let (_, dkim_raw) = find_header(&fields, "dkim-signature").expect("dkim header");
        let tags = parse_tags(dkim_raw);
        assert_eq!(tags.get("v").map(String::as_str), Some("1"));
        assert_eq!(tags.get("a").map(String::as_str), Some("rsa-sha256"));
        assert_eq!(tags.get("d").map(String::as_str), Some(domain));
        assert_eq!(tags.get("s").map(String::as_str), Some(selector));
        let bh = tags.get("bh").expect("bh");
        assert_eq!(bh, &body_hash_b64(body));
        let h = tags.get("h").expect("h");
        let b = tags.get("b").expect("b");
        let sig = STANDARD.decode(b.as_bytes()).unwrap();
        let empty_b = {
            let s = String::from_utf8_lossy(dkim_raw);
            let mut rebuilt = String::new();
            for part in s.split(';') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if part.starts_with("b=") || part.starts_with("b =") {
                    continue;
                }
                if !rebuilt.is_empty() {
                    rebuilt.push_str("; ");
                }
                rebuilt.push_str(part);
            }
            rebuilt.push_str("; b=");
            rebuilt
        };
        let mut to_sign = Vec::new();
        for name in h.split(':') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let (orig, value) = find_header(&fields, name).expect(name);
            to_sign.extend(relaxed_header(&orig, value));
        }
        to_sign.extend(relaxed_header("dkim-signature", empty_b.as_bytes()));
        let key = load_private_pem(pem).unwrap();
        let vk = VerifyingKey::<Sha256>::new(RsaPublicKey::from(&key));
        let sig = rsa::pkcs1v15::Signature::try_from(sig.as_slice()).unwrap();
        vk.verify(&to_sign, &sig).expect("rsa verify");
    }

    fn parse_tags(raw: &[u8]) -> std::collections::BTreeMap<String, String> {
        let s = String::from_utf8_lossy(raw);
        let mut map = std::collections::BTreeMap::new();
        for part in s.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (k, v) = match part.split_once('=') {
                Some(p) => p,
                None => continue,
            };
            let v: String = v.chars().filter(|c| !c.is_ascii_whitespace()).collect();
            map.insert(k.trim().to_ascii_lowercase(), v);
        }
        map
    }
}
