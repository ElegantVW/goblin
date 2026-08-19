//! DNS recipes for Squarespace paste. No API — human applies rows.

pub fn print_dns(domain: &str, mail_host: Option<&str>) {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let mail = mail_host
        .map(|s| s.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("mail.{domain}"));

    println!("# goblind sky print-dns — paste into Squarespace → Domains → {domain} → DNS");
    println!("# Do NOT flip MX until goblind lab smoke is green. See docs/DOMAIN.md");
    println!();
    println!("# A/AAAA — point at the goblind host public IP(s)");
    println!("# Type  Host   Data");
    println!("# A     mail   <IPv4 of this machine / office box>");
    println!("# AAAA  mail   <IPv6 if any>");
    println!();
    println!("# MX");
    println!("# Type  Host  Priority  Data");
    println!("# MX    @     10        {mail}");
    println!();
    println!("# SPF (tune once outbound works)");
    println!("# Type  Host  Data");
    println!("# TXT   @     v=spf1 mx a:{mail} -all");
    println!();
    println!("# DKIM — after `goblind` generates keys, replace SELECTOR and PUBLIC_KEY");
    println!("# Type  Host                         Data");
    println!("# TXT   SELECTOR._domainkey          v=DKIM1; k=rsa; p=PUBLIC_KEY");
    println!();
    println!("# DMARC — start with none");
    println!("# Type  Host     Data");
    println!("# TXT   _dmarc   v=DMARC1; p=none; rua=mailto:postmaster@{domain}");
    println!();
    println!("# Remove Purelymail MX/SPF/DKIM only after steal+send via goblind succeed.");
}
