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
| TXT | `._domainkey` / selector | DKIM public key from goblind |
| TXT | `_dmarc` | `v=DMARC1; p=none; rua=mailto:design@…` (start with `p=none`) |

Remove Purelymail MX/SPF/DKIM only after:

- `goblin steal` against goblind INBOX works
- outbound mail is accepted by major providers
- you have waited for TTL

Optional dual-MX during trial: keep Purelymail at a worse priority, or run a short parallel receive test before the flip.

## What Goblin will add later

- `goblind` — company IMAP/SMTP for `@vanguardaautomovel.com`
- `goblin sky prepare DOMAIN` — print the Squarespace rows above
- `goblin sky check DOMAIN` — verify live DNS matches the recipe (read-only)
- Mailbox provisioning for shop users

Registrar renewals, nameservers, and transfers stay in Squarespace. If you need API-driven DNS, move nameservers to Cloudflare (or similar) — that is optional and separate.
