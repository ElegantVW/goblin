//! Search across unread/read/trash.

use crate::error::Error;
use crate::store::{MailBox, MailMeta, Store};

/// Box-aware match. Query language (all case-insensitive):
/// bare words = AND-substring over from/to/subject/body/account/uid;
/// `from:X`, `subj:X`, `has:attach`, `is:unread|read|trash`.
pub fn matches_in(mail: &MailMeta, box_name: Option<MailBox>, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let hay = haystack(mail);
    for tok in q.split_whitespace() {
        let low = tok.to_ascii_lowercase();
        if let Some(v) = low.strip_prefix("from:") {
            if !mail.from.to_ascii_lowercase().contains(v) {
                return false;
            }
        } else if let Some(v) = low.strip_prefix("subj:").or_else(|| low.strip_prefix("subject:"))
        {
            if !mail.subject.to_ascii_lowercase().contains(v) {
                return false;
            }
        } else if low == "has:attach" {
            if mail.attachments.is_empty() {
                return false;
            }
        } else if let Some(v) = low.strip_prefix("is:") {
            let want = match v {
                "unread" => Some(MailBox::Unread),
                "read" => Some(MailBox::Read),
                "trash" => Some(MailBox::Trash),
                _ => None,
            };
            match (want, box_name) {
                (Some(w), Some(b)) if w == b => {}
                _ => return false,
            }
        } else if !hay.contains(&low) {
            return false;
        }
    }
    true
}

fn haystack(mail: &MailMeta) -> String {
    format!(
        "{} {} {} {} {} {} {}",
        mail.from, mail.to, mail.subject, mail.body, mail.account, mail.uid, mail.name()
    )
    .to_ascii_lowercase()
}

pub fn search_store(store: &Store, query: &str) -> Result<Vec<(MailBox, MailMeta)>, Error> {
    let mut out = Vec::new();
    for b in MailBox::all() {
        for m in store.load_mails(b)? {
            if matches_in(&m, Some(b), query) {
                out.push((b, m));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mail(from: &str, subject: &str, body: &str) -> MailMeta {
        MailMeta {
            uid: "1".into(),
            from: from.into(),
            subject: subject.into(),
            body: body.into(),
            ..MailMeta::default()
        }
    }

    #[test]
    fn matches_from_subject_body_case_insensitive() {
        let m = mail("Ada <ada@x>", "Quarterly Report", "please see attached");
        assert!(matches_in(&m, None, "ADA"));
        assert!(matches_in(&m, None, "report"));
        assert!(matches_in(&m, None, "ATTACHED"));
        assert!(!matches_in(&m, None, "zebra"));
    }

    #[test]
    fn query_operators() {
        let mut m = mail("Ada <ada@x>", "Quarterly Report", "please see attached");
        assert!(matches_in(&m, None, "from:ada report"));
        assert!(!matches_in(&m, None, "from:bob"));
        assert!(matches_in(&m, None, "subj:quarterly"));
        assert!(!matches_in(&m, None, "subj:invoice"));
        assert!(!matches_in(&m, None, "has:attach"));
        m.attachments.push("bill.pdf".into());
        assert!(matches_in(&m, None, "has:attach"));
        assert!(matches_in(&m, Some(MailBox::Unread), "is:unread"));
        assert!(!matches_in(&m, Some(MailBox::Read), "is:unread"));
        assert!(!matches_in(&m, None, "is:unread"));
        assert!(!matches_in(&m, Some(MailBox::Unread), "is:zzz"));
    }

    #[test]
    fn empty_query_matches_all() {
        assert!(matches_in(&mail("a", "b", "c"), None, ""));
        assert!(matches_in(&mail("a", "b", "c"), None, "   "));
    }

    #[test]
    fn search_spans_all_boxes() {
        let dir = tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let mut a = mail("ann@x", "hello", "one");
        a.uid = "1".into();
        let mut b = mail("bob@x", "invoice", "two");
        b.uid = "2".into();
        store.write_mail(MailBox::Unread, &a, "one").unwrap();
        store.write_mail(MailBox::Trash, &b, "two").unwrap();
        let hits = search_store(&store, "invoice").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, MailBox::Trash);
        assert_eq!(hits[0].1.subject, "invoice");
        let all = search_store(&store, "").unwrap();
        assert_eq!(all.len(), 2);
    }
}
