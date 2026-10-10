![Goblin hero](assets/hero/goblin.svg)

# Goblin — mail spirit. Ask him. **No aerc.** ✉️

Rust engine for [faeOS](https://github.com/ElegantVW/faeOS) and for company use. One binary: CLI + TUI. TLS-only, secrets in keyring — never in JSON, URLs, or logs.

Status: **0.1.1** — looking for testers. TLS-only. Secrets in keyring.
Downloads (linux): [Grove](https://elegantvw.github.io/grove/#app-goblin) /
[Releases](https://github.com/ElegantVW/goblin/releases). Windows and
macOS are source this pass (`docs/PORT.md`). Voice: `goblin: <one sentence>`
+ `next: <one thing>`.

For: terminal IMAP/SMTP mail with secrets in keyring. Not for: Gmail OAuth, Exchange, bulk mail.

## Looking for testers

We want reports from **Linux** (Debian, Q4OS, others), **Windows** (WSL
+ linux binary), and **macOS** (build from source). Open a
[GitHub issue](https://github.com/ElegantVW/goblin/issues) with the
smoke list in [`docs/PORT.md`](docs/PORT.md), or email
evenweaker@disroot.org with subject `[goblin test]`. OS, terminal,
preset, what `steal`/`peek` showed, what broke. No passwords or mail
bodies.

## Look

![Goblin first run — pick a sky](assets/screenshots/goblin-summon.png)
![Goblin steal with no accounts yet](assets/screenshots/goblin-steal.png)

```
/\   /\
 |   | 
 |o o| 
 |   | 
  \_/  
```

## Rules

- **TLS required.** IMAP is 993 (implicit TLS) or 143 + STARTTLS. SMTP is 465 (implicit TLS) or 587 + STARTTLS. Certificate verification is on. There is no insecure flag.
- **Secrets stay out of JSON.** OS keyring first (`service=goblin`). Fallback: a `0600` `secrets` file. Optional `accounts.json.gpg` via system `gpg`. Never in URLs, argv, logs, or Pixie output.
- **No other mail programs.** Not aerc, isync, msmtp, notmuch, or himalaya. `goblin import-aerc` is a one-shot migrator and never copies the password into `accounts.json`.

## Install

Grove linux vessel, or build:

```bash
git clone https://github.com/ElegantVW/goblin.git ~/goblin
cd ~/goblin && ./build.sh install
```

`build.sh install` writes the ELF to `~/.local/lib/faeos/goblin` and a launcher to `~/bin/goblin`. Override the binary with `GOBLIN_BIN`. Port notes: [`docs/PORT.md`](docs/PORT.md).

## First run

```bash
goblin summon            # pick purelymail / google / disroot / outlook / yahoo
goblin steal
goblin
```

## Commands

```
goblin                         open the horde
goblin summon [--preset google]
goblin who · wake NAME · mend [NAME] · dismiss [NAME]
goblin steal                   new letters
goblin peek [unread|read|trash]
goblin read 1
goblin send --to ADDR --subject STR
goblin hunt invoice
goblin watch
goblin keep | trash
goblin parcel list|save|open
goblin squeak [--set FILE]
```

Old names (`sync`, `list`, `show`, `search`, `account`…) still work as aliases.

TUI: `/` hunt (`from:` `subj:` `has:attach` `is:unread`) · `O` sort · `H` hire desk · `[]` wake · `N` summon · `X` dismiss · `a` parcel. Mouse: click selects, double-click opens, wheel scrolls, hover underlines (`GOBLIN_MOUSE=0` / `GOBLIN_HOVER=0` disable).

Linux paths: `~/.config/goblin/`, `~/.cache/goblin/mail/{unread,read,trash}/`.

## License

MIT — see [LICENSE](LICENSE).
