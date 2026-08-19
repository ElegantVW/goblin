# Domain + mail for vanguardaautomovel.com

Squarespace holds the **domain and DNS UI**. Goblin does **not** replace Squarespace (there is no public Squarespace DNS API). Goblin will own **mail** for the domain once `goblind` ships.

## Today (Purelymail still MX)

| Piece | Where |
|-------|--------|
| Registrar / DNS | Squarespace → Domains → `vanguardaautomovel.com` → DNS |
| Public MX | Purelymail (current) |
| Company mailbox | `design@vanguardaautomovel.com` |
| Client | `goblin` goblin **Vanguarda** (preset/hosts already in accounts) |

### Vanguarda daily driver

```bash
goblin who
goblin wake Vanguarda
goblin steal
goblin peek
goblin send --to someone@example.com --subject "…" --body-file msg.txt
goblin          # TUI horde
```

Keep Purelymail MX until `goblind` is proven on this machine (then the office box).

## Target shape (eliminate Purelymail)

```text
Squarespace DNS  --MX/SPF/DKIM/DMARC-->  mail.vanguardaautomovel.com
                                              │
                                           goblind
                                              │
                                    goblin clients (shop PCs)
```

1. **Host:** this machine first; later a full-time office PC. Same `goblind` binary; change the A/AAAA record when you move.
2. **Name:** prefer `mail.vanguardaautomovel.com` (A/AAAA → server public IP). MX points at that name.
3. **TLS:** Let’s Encrypt (or equivalent) for IMAP 993 / SMTP 465 (and 587 STARTTLS).
4. **Risk:** residential/office ISP may block outbound port 25; uptime and IP reputation matter. If deliverability fails, park `goblind` on a small VPS and keep Squarespace DNS.

## Squarespace DNS checklist (cutover — do not run until goblind is ready)

Lower TTL on MX/A a day ahead. Then Custom records approximately:

| Type | Host | Data (examples — Goblin will print exact values) |
|------|------|---------------------------------------------------|
| A / AAAA | `mail` | public IPv4 / IPv6 of the goblind host |
| MX | `@` | `mail.vanguardaautomovel.com` (priority 10) |
| TXT | `@` | `v=spf1 mx a:mail.vanguardaautomovel.com -all` (tune) |
| TXT | `goblin._domainkey` | DKIM public key from `goblind dkim init` / `sky print-dns` |
| TXT | `_dmarc` | `v=DMARC1; p=none; rua=mailto:design@…` (start with `p=none`) |

Remove Purelymail MX/SPF/DKIM only after:

- `goblin steal` against goblind INBOX works
- outbound mail is accepted by major providers
- you have waited for TTL

Optional dual-MX during trial: keep Purelymail at a worse priority, or run a short parallel receive test before the flip.

## goblind (our server — in progress)

`goblind` is **our** Rust binary in this repo (not Postfix/Dovecot/Purelymail). Data default: `GOBLIND_HOME` or the platform local-data dir for `goblind`.

```bash
# Mailbox (password never in accounts.json)
export GOBLIND_HOME=/path/to/goblind-data   # optional isolation
goblind user add design@vanguardaautomovel.com
goblind user list

# DKIM (selector goblin) then paste the TXT — do not flip MX yet
goblind dkim init
goblind sky print-dns vanguardaautomovel.com

# Run listeners + outbound worker (submission 465 or 587; IMAP 993)
goblind run --bind 0.0.0.0 --smtp-in 25 --smtp-sub 465 --imap 993
```

Outbound is **ours**: a queue worker talks to the recipient MX on port 25 and signs with our DKIM. No Postfix, no OpenDKIM, no smart-host.

- Local recipients stay in Maildir; only **external** addresses are queued.
- Failures retry (60s × 2^n, cap 1h, 8 attempts) then `queue/failed/` + `queue/failed.log`. No DSN bounce is generated.
- If port 25 outbound is blocked (common on residential/office ISPs), move `goblind` to a VPS or other unblocked IP. There is no relay code to add.

Still coming: systemd unit, MX cutover smoke. **Do not flip production MX** until lab steal+send is green.

Lab CA is `$GOBLIND_HOME/tls/ca.pem`. Point the client at it so verification stays on (no insecure flag):

```bash
export GOBLIN_EXTRA_CA=$GOBLIND_HOME/tls/ca.pem
# goblin nest: imap 127.0.0.1:993  smtp 127.0.0.1:465  (or localhost / mail.vanguardaautomovel.com)
goblin steal
goblin send --to design@vanguardaautomovel.com --subject "lab" --body-file msg.txt
```

Binding 25/465/993 needs root, `CAP_NET_BIND_SERVICE`, or a lowered `net.ipv4.ip_unprivileged_port_start`.

Registrar renewals, nameservers, and transfers stay in Squarespace. If you need API-driven DNS, move nameservers to Cloudflare (or similar) — that is optional and separate.
