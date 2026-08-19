//! Search across unread/read/trash.

use crate::error::Error;
use crate::store::{MailBox, MailMeta, Store};

pub fn matches(mail: &MailMeta, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let q = q.to_ascii_lowercase();
    haystack(mail).contains(&q)
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
            if matches(&m, query) {
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
        assert!(matches(&m, "ADA"));
        assert!(matches(&m, "report"));
        assert!(matches(&m, "ATTACHED"));
        assert!(!matches(&m, "zebra"));
    }

    #[test]
    fn empty_query_matches_all() {
        assert!(matches(&mail("a", "b", "c"), ""));
        assert!(matches(&mail("a", "b", "c"), "   "));
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
