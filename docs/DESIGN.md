# Goblin design

Rust mail **product**. One repo, three pillars. No aerc at runtime.

## Language

Rust. Own repo (Bulwark-style install on faeOS). Cross-OS **client**. Server (`goblind`) is Linux-first.

## Three pillars

| Pillar | Binary / surface | Job |
|--------|------------------|-----|
| 1. Simple mail | `goblin` client | Personal + company daily mail (IMAP/SMTP). Horde CLI + TUI. |
| 2. Private company mail | `goblind` | Company IMAP/SMTP for domains we own (e.g. `@vanguardaautomovel.com`). Replaces Purelymail. |
| 3. Domain / sky ops | CLI (+ later TUI) | Mail DNS recipes + health checks; mailbox admin. Not the registrar. |

See [DOMAIN.md](DOMAIN.md) for `vanguardaautomovel.com` + Squarespace.

## Rules

- TLS required: IMAP 993 or 143+STARTTLS; SMTP 465 or 587+STARTTLS. Cert verification on. No insecure flag.
- Secrets: default `0600` secrets file; optional `--features keyring` (OS keyring first). Optional `accounts.json.gpg`. Never in JSON, URLs, argv, logs, or Pixie output.
- Unix: config dir `0700`, accounts and mail files `0600`. Fail closed if group/other-readable.
- Linux paths stay `~/.config/goblin/` and `~/.cache/goblin/mail/{unread,read,trash}/` when `GOBLIN_HOME` unset. Windows uses the platform config/cache dirs from the `directories` crate.
- No other mail programs. `goblin import-aerc` is a one-shot and never writes the password into JSON.

## Client status

Shipped: search, attachments, multi-account (summon/who/wake/mend/dismiss), Purelymail/google/disroot/outlook/yahoo presets, quality gate (portable crate, optional keyring, CI).

In progress: **company client parity** — real Windows TUI (crossterm), password echo-off on Windows, DOMAIN/DESIGN docs, CI green on Windows.

## Purelymail retirement

Purelymail is the **current** public MX for Vanguarda. It will be **removed** once `goblind` answers MX for the domain (first on the build machine, then the office server). Until cutover, the client keeps using Purelymail hosts for the Vanguarda goblin.

## goblind (server)

In-tree: SMTP inbound + submission, IMAP, Maildir, lab TLS, **outbound MX worker** (port 25, queue retries, no DSN), **DKIM** (`goblind dkim init`, selector `goblin`). No Postfix / OpenDKIM / smart-host.

## Later (not this phase)

- Squarespace MX cutover (DOMAIN.md checklist) — do not flip production MX until lab smoke is green
- HTML compose, Bcc, drafts, Sent IMAP APPEND
- `sky prepare` / `sky check`
- UID namespaces per folder/account (avoid cross-folder UID collisions in the local cache)
