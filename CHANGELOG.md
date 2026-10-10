# Goblin changelog

## 2026-10-10 — letter-pouch (v0.1.1)

- GitHub Release: linux x86_64. Grove card has the pills. Windows and
  macOS are source (`docs/PORT.md`) — TUI is unix termios. TLS-only.
  Keyring for secrets. Friend uses their own mailbox.

## v1.1-steal (2026-09-26)

- `steal` is the primary fetch verb (`sync`/`fetch` stay as hidden aliases).
- Error shape: `goblin: <sentence>` + `next: <thing>`, `detail:` only with `FAE_DEBUG=1`.
- No `mail_move`/uid jargon in user output. TLS-only unchanged.
- Evidence: `cargo test` 44 pass.
