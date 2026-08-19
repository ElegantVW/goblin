# Goblin v1 design

Rust mail **client**. Owns its accounts. Speaks IMAP + SMTP. No aerc at runtime.

## Language

Rust. Own repo, same install contract as Bulwark. Cross-OS. The same tree can grow an office `goblind` later; that is a different spec.

## Rules

- TLS required: IMAP 993 or 143+STARTTLS; SMTP 465 or 587+STARTTLS. Cert verification on. No insecure flag.
- Secrets: OS keyring (`service=goblin`), then `0600` `secrets` file, optional `accounts.json.gpg`. Never in JSON, URLs, argv, logs, or Pixie output.
- Unix: config dir `0700`, accounts and mail files `0600`. Fail closed if group/other-readable.
- Linux paths stay `~/.config/goblin/` and `~/.cache/goblin/mail/{unread,read,trash}/`.
- No other mail programs. `goblin import-aerc` is a one-shot and never writes the password into JSON.

## v1.1 (this phase)

- Search across unread/read/trash (`goblin search`, TUI `/`).
- Attachments extracted on sync to `~/.cache/goblin/attach/{uid}/`; `goblin attach list|save|open`; TUI `a` / `n`.
- Multiple accounts: `account add` upserts; `account show` lists; `account use NAME`; TUI `[` `]`; `sync --account NAME`.

## Still later

HTML compose, Bcc, drafts box, Sent IMAP APPEND, office mail server, replacing Purelymail, Windows/macOS service wrappers.

## Later

Office LAN engine in this repo. Purelymail stays the public MX/relay. Same client, new account pointing at the office host.
