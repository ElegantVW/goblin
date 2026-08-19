# Purelymail → Goblin — take to work (checklist)

Print this. Cross off lines with a pen. Squarespace stays for the **domain/DNS only**. Mail moves to **your** `goblind` + **Goblin** client (GUI on Windows; `goblin` in PowerShell/terminal if you want).

**Domain:** vanguardaautomovel.com  
**Company mailbox:** design@vanguardaautomovel.com  
**Mail hostname:** mail.vanguardaautomovel.com  

---

## A. Write down (before you leave)

| Item | Value |
|------|--------|
| Public IPv4 of goblind host | `46.50.107.65` (re-check: _______________) |
| LAN IP of goblind PC | `192.168.255._______` |
| GOBLIND_HOME on server | `/home/evenweaker/goblind-data` |
| goblind password file | `…/goblind-data/design.password` |
| DNS cheat-sheet file | `…/goblind-data/squarespace-dns.txt` |
| Windows installer later | `Goblin-Setup.exe` → Desktop shortcut opens **GUI only** |

---

## B. Router (office / home gateway)

Forward to the goblind LAN IP:

- [ ] TCP **25** → PC:25  
- [ ] TCP **465** → PC:465  
- [ ] TCP **993** → PC:993  
- [ ] (optional) TCP **587** → PC:587  

From a phone hotspot, test:

- [ ] `nc` / port check to public IP **:25**  
- [ ] same for **:993**  

---

## C. Squarespace DNS — do **not** flip MX yet

Open: Domains → **vanguardaautomovel.com** → **DNS** → custom records.

### C1. Publish first (order matters)

- [ ] **A** — Host `mail` → public IPv4 `________________`  
- [ ] **TXT** — Host `goblin._domainkey` → full `v=DKIM1; k=rsa; p=…` from cheat-sheet  
- [ ] **TXT** — Host `@` SPF (edit existing SPF if one exists; don’t create two):  
  `v=spf1 mx a:mail.vanguardaautomovel.com ~all`  
- [ ] **TXT** — Host `_dmarc` →  
  `v=DMARC1; p=none; rua=mailto:postmaster@vanguardaautomovel.com`  

Wait for DNS (often minutes–hours):

- [ ] `dig +short mail.vanguardaautomovel.com A` shows your IP  
- [ ] `dig +short goblin._domainkey.vanguardaautomovel.com TXT` shows `p=`  

### C2. Point clients at goblind (still Purelymail MX)

- [ ] Windows: install Goblin GUI when ready — **Desktop shortcut opens GUI**  
- [ ] Or PowerShell: run `goblin` for terminal UI / CLI  
- [ ] Account hosts: IMAP/SMTP = `mail.vanguardaautomovel.com` (or public IP)  
- [ ] Ports: IMAP **993**, SMTP **465**  
- [ ] User: `design@vanguardaautomovel.com`  
- [ ] Password: from `design.password` on the server (not the Purelymail password)  
- [ ] If using lab CA: set `GOBLIN_EXTRA_CA` to server `tls/ca.pem` (or install a real Let’s Encrypt cert later)  

Smoke:

- [ ] Steal / sync works  
- [ ] Send to yourself (disroot/gmail)  
- [ ] On server: `journalctl -u goblind -f` shows `outbox: … ok`  
- [ ] Recipient “show original” → DKIM **pass** (after DKIM TXT is live)  

### C3. Flip MX (only after C2 is green)

- [ ] Lower MX TTL if UI allows  
- [ ] **MX** Host `@` Priority **10** → `mail.vanguardaautomovel.com`  
- [ ] Delete Purelymail MX rows  
- [ ] From Gmail/phone: send to `design@vanguardaautomovel.com`  
- [ ] Steal in Goblin — mail arrives  

---

## D. Decommission Purelymail (save money)

- [ ] Export/archive anything still only in Purelymail webmail  
- [ ] Remove leftover Purelymail SPF/DKIM TXT if any  
- [ ] Tighten SPF to `-all` once only goblind sends:  
  `v=spf1 mx a:mail.vanguardaautomovel.com -all`  
- [ ] Cancel Purelymail subscription  
- [ ] Confirm invoice/renewal is gone  

---

## E. Later — office PC

- [ ] Stop `goblind` on old host  
- [ ] Copy whole `GOBLIND_HOME` to office machine  
- [ ] Start `goblind` + systemd there  
- [ ] Update **A** record if public IP changed (MX name stays `mail.…`)  

---

## F. If something breaks

| Symptom | Likely fix |
|---------|------------|
| Can’t connect :25 from outside | Router forward / ISP inbound block |
| `outbox: … fail` / Helo errors | EHLO must be FQDN (`mail.vanguardaautomovel.com`); check `GOBLIND_EHLO` |
| Outbound always fails | Outbound :25 blocked → move goblind to VPS; update A only |
| SPF fails while still on Purelymail | Don’t use hard `-all` until Purelymail is cancelled |
| Windows SmartScreen | Unsigned build: More info → Run anyway (code signing later) |

---

## Sign-off

| Step | Date | Initials |
|------|------|----------|
| Router done | | |
| DNS A+DKIM+SPF+DMARC | | |
| Client smoke OK | | |
| MX flipped | | |
| Purelymail cancelled | | |

*Goblin = your software. No Postfix/Dovecot/Purelymail in the stack after D.*
