//! RFC5322 builder. No Bcc.

use crate::store::MailMeta;
use chrono::Utc;

pub fn compose(from: &str, to: &str, cc: &[String], subject: &str, body: &str) -> Vec<u8> {
    build(from, to, cc, subject, body, None, None)
}

pub fn reply(from: &str, original: &MailMeta, body: &str) -> Vec<u8> {
    let subject = re_subject(&original.subject);
    let to = addr_of(&original.from);
    let mid = original.message_id.trim();
    let in_reply_to = if mid.is_empty() { None } else { Some(mid) };
    let references = in_reply_to;
    build(from, &to, &[], &subject, body, in_reply_to, references)
}

pub fn quote_body(original: &MailMeta) -> String {
    let mut out = String::new();
    if !original.from.is_empty() {
        out.push_str(&format!("On {}, {} wrote:\n", original.date, original.from));
    }
    for line in original.body.lines() {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn re_subject(subject: &str) -> String {
    let t = subject.trim();
    if t.len() >= 3 && t[..3].eq_ignore_ascii_case("re:") {
        t.to_string()
    } else {
        format!("Re: {t}")
    }
}

fn addr_of(from: &str) -> String {
    if let (Some(s), Some(e)) = (from.find('<'), from.find('>')) {
        if e > s {
            return from[s + 1..e].trim().to_string();
        }
    }
    from.trim().to_string()
}

fn build(
    from: &str,
    to: &str,
    cc: &[String],
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    references: Option<&str>,
) -> Vec<u8> {
    let date = Utc::now().format("%a, %d %b %Y %H:%M:%S +0000").to_string();
    let mid = message_id();
    let mut headers = String::new();
    headers.push_str(&format!("From: {from}\r\n"));
    headers.push_str(&format!("To: {to}\r\n"));
    if !cc.is_empty() {
        headers.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
    }
    headers.push_str(&format!("Subject: {subject}\r\n"));
    headers.push_str(&format!("Date: {date}\r\n"));
    headers.push_str(&format!("Message-ID: {mid}\r\n"));
    if let Some(irt) = in_reply_to {
        headers.push_str(&format!("In-Reply-To: {irt}\r\n"));
    }
    if let Some(refs) = references {
        headers.push_str(&format!("References: {refs}\r\n"));
    }
    headers.push_str("MIME-Version: 1.0\r\n");
    headers.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    headers.push_str("Content-Transfer-Encoding: 8bit\r\n");
    headers.push_str("\r\n");
    let mut out = headers.into_bytes();
    let body = body.replace('\n', "\r\n");
    out.extend_from_slice(body.as_bytes());
    if !body.ends_with("\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn message_id() -> String {
    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".into());
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("<{n}@{host}>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_has_required_headers_and_no_bcc() {
        let raw = compose(
            "Ada <ada@example.com>",
            "bob@example.com",
            &["cc@example.com".into()],
            "Hello",
            "body line",
        );
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains("From: Ada <ada@example.com>\r\n"));
        assert!(text.contains("To: bob@example.com\r\n"));
        assert!(text.contains("Cc: cc@example.com\r\n"));
        assert!(text.contains("Subject: Hello\r\n"));
        assert!(text.contains("Date: "));
        assert!(text.contains("Message-ID: <"));
        assert!(text.contains("MIME-Version: 1.0\r\n"));
        assert!(text.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(!text.to_ascii_lowercase().contains("bcc:"));
        assert!(text.contains("\r\n\r\nbody line\r\n"));
    }

    #[test]
    fn reply_sets_re_once_and_in_reply_to() {
        let original = MailMeta {
            from: "Bob <bob@example.com>".into(),
            subject: "Re: already".into(),
            message_id: "<mid@x>".into(),
            date: "Mon, 1 Jan 2026 00:00:00 +0000".into(),
            body: "old".into(),
            ..MailMeta::default()
        };
        let raw = reply("Ada <ada@example.com>", &original, "thanks");
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains("Subject: Re: already\r\n"));
        assert!(!text.contains("Subject: Re: Re:"));
        assert!(text.contains("In-Reply-To: <mid@x>\r\n"));
        assert!(text.contains("References: <mid@x>\r\n"));
        assert!(text.contains("To: bob@example.com\r\n"));
    }

    #[test]
    fn reply_adds_re_when_missing() {
        let original = MailMeta {
            from: "bob@example.com".into(),
            subject: "Hi".into(),
            message_id: "<z@z>".into(),
            ..MailMeta::default()
        };
        let text = String::from_utf8(reply("me@x", &original, "k")).unwrap();
        assert!(text.contains("Subject: Re: Hi\r\n"));
    }
}
