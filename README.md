# Goblin

Mail spirit. Ask him. **No aerc.**

Rust engine for [faeOS](https://github.com/ElegantVW/faeOS) and for company use. One binary: CLI + TUI.

## Rules

- **TLS required.** IMAP is 993 (implicit TLS) or 143 + STARTTLS. SMTP is 465 (implicit TLS) or 587 + STARTTLS. Certificate verification is on. There is no insecure flag.
- **Secrets stay out of JSON.** Default: a `0600` `secrets` file under the config dir. Optional `cargo build --features keyring` uses the OS keyring first (`service=goblin`), then the file fallback. Optional `accounts.json.gpg` via system `gpg`. Never in URLs, argv, logs, or Pixie output.
- **No other mail programs.** Not aerc, isync, msmtp, notmuch, or himalaya. `goblin import-aerc` is a one-shot migrator and never copies the password into `accounts.json`.

## Install (any machine with Rust)

```bash
git clone https://github.com/ElegantVW/goblin.git
cd goblin
cargo test
cargo build --release
# binary: target/release/goblin
```

Optional OS keyring (needs a secret-service / keyutils / Windows Credential Manager):

```bash
cargo build --release --features keyring
```

Override all paths (tests, portable USB, CI):

```bash
export GOBLIN_HOME=/path/to/goblin-data
```

Unset:

- Linux: `~/.config/goblin` and `~/.cache/goblin`
- Windows: `%APPDATA%\goblin` (config) and `%LOCALAPPDATA%\goblin` (cache) via the `directories` crate
- macOS: `~/Library/Application Support/faeos.goblin` (and related cache)

faeOS house install remains `./build.sh install` (writes `~/.local/lib/faeos/goblin` + `~/bin/goblin`).

## First run

```bash
goblin summon            # pick purelymail / google / disroot / outlook / yahoo
goblin steal
goblin
```

### Vanguarda Automovel (company goblin)

Domain DNS stays on Squarespace; mail is still Purelymail until `goblind` cutover — see [docs/DOMAIN.md](docs/DOMAIN.md).

```bash
goblin wake Vanguarda    # design@vanguardaautomovel.com
goblin steal
goblin peek
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

TUI: `/` hunt · `[]` wake · `N` summon · `X` dismiss · `a` parcel.

Linux paths: `~/.config/goblin/`, `~/.cache/goblin/mail/{unread,read,trash}/`.

## Windows

Desktop users get **Goblin GUI** (`goblin-gui.exe`). The installer creates a Desktop / Start Menu shortcut to the GUI only.

PowerShell / Windows Terminal (optional):

```powershell
goblin who
goblin steal
goblin          # terminal TUI
```

Build installer (on Windows, with [Inno Setup 6](https://jrsoftware.org/isinfo.php)):

```powershell
.\packaging\windows\build.ps1
# → dist\windows\Goblin-Setup-0.1.0.exe
```

Printable Purelymail → Goblin cutover: [docs/MIGRATE-PURELYMAIL.md](docs/MIGRATE-PURELYMAIL.md).

## License

MIT — see [LICENSE](LICENSE).