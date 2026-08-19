//! DNS recipes for Squarespace paste. No API — human applies rows.

use super::dkim;
use std::fmt::Write as _;

pub fn print_dns(domain: &str, mail_host: Option<&str>) {
    print!("{}", dns_recipe(domain, mail_host));
}

pub fn dns_recipe(domain: &str, mail_host: Option<&str>) -> String {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let mail = mail_host
        .map(|s| s.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("mail.{domain}"));

    let mut s = String::new();
    let _ = writeln!(
        s,
        "# goblind sky print-dns — paste into Squarespace → Domains → {domain} → DNS"
    );
    let _ = writeln!(
        s,
        "# Do NOT flip MX until goblind lab smoke is green. See docs/DOMAIN.md"
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "# A/AAAA — point at the goblind host public IP(s)");
    let _ = writeln!(s, "# Type  Host   Data");
    let _ = writeln!(s, "# A     mail   <IPv4 of this machine / office box>");
    let _ = writeln!(s, "# AAAA  mail   <IPv6 if any>");
    let _ = writeln!(s);
    let _ = writeln!(s, "# MX");
    let _ = writeln!(s, "# Type  Host  Priority  Data");
    let _ = writeln!(s, "# MX    @     10        {mail}");
    let _ = writeln!(s);
    let _ = writeln!(s, "# SPF (tune once outbound works)");
    let _ = writeln!(s, "# Type  Host  Data");
    let _ = writeln!(s, "# TXT   @     v=spf1 mx a:{mail} -all");
    let _ = writeln!(s);
    let records = dkim::public_records();
    if records.is_empty() {
        let _ = writeln!(s, "# DKIM — run `goblind dkim init` then re-run print-dns");
        let _ = writeln!(s, "# Type  Host                         Data");
        let _ = writeln!(
            s,
            "# TXT   goblin._domainkey            v=DKIM1; k=rsa; p=PUBLIC_KEY"
        );
    } else {
        let _ = writeln!(s, "# DKIM");
        let _ = writeln!(s, "# Type  Host                         Data");
        for (sel, rec) in records {
            let _ = writeln!(s, "# TXT   {sel}._domainkey            {rec}");
        }
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "# DMARC — start with none");
    let _ = writeln!(s, "# Type  Host     Data");
    let _ = writeln!(
        s,
        "# TXT   _dmarc   v=DMARC1; p=none; rua=mailto:postmaster@{domain}"
    );
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "# Remove Purelymail MX/SPF/DKIM only after steal+send via goblind succeed."
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil;
    use crate::server::paths::{self, with_goblind_home};

    #[test]
    fn placeholder_until_init() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            paths::ensure_layout().unwrap();
            let out = dns_recipe("vanguardaautomovel.com", None);
            assert!(out.contains("goblind dkim init"), "{out}");
            assert!(out.contains("goblin._domainkey"), "{out}");
            assert!(out.contains("p=PUBLIC_KEY"), "{out}");
            assert!(out.contains("mail.vanguardaautomovel.com"), "{out}");
        });
    }

    #[test]
    fn real_txt_when_key_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        with_goblind_home(Some(dir.path()), || {
            paths::ensure_layout().unwrap();
            fsutil::write_private(&dkim::public_path("goblin"), b"v=DKIM1; k=rsa; p=TESTKEY\n")
                .unwrap();
            let out = dns_recipe("vanguardaautomovel.com", None);
            assert!(out.contains("goblin._domainkey"), "{out}");
            assert!(out.contains("p=TESTKEY"), "{out}");
            assert!(!out.contains("goblind dkim init"), "{out}");
            assert!(!out.contains("p=PUBLIC_KEY"), "{out}");
        });
    }
}
