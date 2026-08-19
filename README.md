# Goblin

Mail client. Owns its accounts. Speaks IMAP and SMTP. **No aerc.**

Rust engine for [faeOS](https://github.com/ElegantVW/faeOS) and for company use. One binary: CLI + TUI.

## Rules

- **TLS required.** IMAP is 993 (implicit TLS) or 143 + STARTTLS. SMTP is 465 (implicit TLS) or 587 + STARTTLS. Certificate verification is on. There is no insecure flag.
- **Secrets stay out of JSON.** OS keyring first (`service=goblin`). Fallback: a `0600` `secrets` file. Optional `accounts.json.gpg` via system `gpg`. Never in URLs, argv, logs, or Pixie output.
- **No other mail programs.** Not aerc, isync, msmtp, notmuch, or himalaya. `goblin import-aerc` is a one-shot migrator and never copies the password into `accounts.json`.

## Install

```bash
git clone git@github.com:ElegantVW/goblin.git ~/goblin
cd ~/goblin && ./build.sh install
```

`build.sh install` writes the ELF to `~/.local/lib/faeos/goblin` and a launcher to `~/bin/goblin`. Override the binary with `GOBLIN_BIN`.

## First run

```bash
goblin account add --preset purelymail
# or
goblin import-aerc
goblin sync
goblin
```

## Commands

```
goblin
goblin account add|show
goblin account add --preset purelymail
goblin import-aerc
goblin sync [--quiet] [--no-notify] [--all] [--force]
goblin idle
goblin list [unread|read|trash] [--plain]
goblin show <file|#> [--plain]
goblin bundle [--snippet N] [--limit N]
goblin move read|trash <files…> [--all] [--local-only]
goblin send --to ADDR --subject STR [--cc ADDR] [--body-file PATH|-]
goblin search QUERY
goblin attach list|save|open …
goblin account use NAME
goblin sound [--set FILE]
```

TUI: `/` search · `[]` account · `a` open attachment · `n` next attachment.

Linux paths: `~/.config/goblin/`, `~/.cache/goblin/mail/{unread,read,trash}/`.

## License

MIT — see [LICENSE](LICENSE).
