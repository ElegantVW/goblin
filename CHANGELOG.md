# Goblin changelog

## v1.1-steal (2026-09-26)

- `steal` is the primary fetch verb (`sync`/`fetch` stay as hidden aliases).
- Error shape: `goblin: <sentence>` + `next: <thing>`, `detail:` only with `FAE_DEBUG=1`.
- No `mail_move`/uid jargon in user output. TLS-only unchanged.
- Evidence: `cargo test` 44 pass.
