# Porting Goblin — other machines

**Looking for testers.** Linux (Debian, Q4OS, others), Windows 10/11,
macOS. Open a [GitHub issue](https://github.com/ElegantVW/goblin/issues)
or email evenweaker@disroot.org with subject `[goblin test]`.

## What ships

1. **Vessel (this is the product)** — TUI + CLI, TLS-only IMAP/SMTP,
   secrets in the OS keyring. Linux prebuilt on Grove.
2. **Windows / macOS this pass** — build from source. The horde TUI is
   unix termios; a native Windows `.exe` would not open it. WSL + the
   linux binary is the Windows friend path.

Hard rules: **TLS required**. Secrets never in JSON, URLs, argv, or logs.
Use **your** mailbox. Do not probe anyone else's host.

## Install

Prebuilt linux is on the Grove card:
[elegantvw.github.io/grove/#app-goblin](https://elegantvw.github.io/grove/#app-goblin)
and on [GitHub Releases](https://github.com/ElegantVW/goblin/releases).
Windows and macOS are source this pass.

Linux prebuilt: `chmod +x goblin-linux-x86_64` and put it on PATH.
Built on Arch; older glibc boxes should build from source. A Secret
Service keyring (GNOME Keyring, KWallet, …) is the usual store; Goblin
falls back to a `0600` `secrets` file.

Preferred if you want to prove the port:

```bash
sudo apt install rustc cargo   # Debian / Q4OS; keyring via the desktop
brew install rust              # macOS
git clone https://github.com/ElegantVW/goblin.git
cd goblin && ./build.sh install   # ~/.local/lib/faeos/goblin + ~/bin/goblin
```

Windows: WSL, then the linux binary (or build inside WSL). Native
Windows Terminal TUI waits on a termios port.

Config: `~/.config/goblin/` (accounts.json with **no** passwords).
Mail cache: `~/.cache/goblin/mail/{unread,read,trash}/`.

## Smoke list

Fill this in when you report:

- OS and version
- Terminal (kitty, Konsole, Windows Terminal / WSL, …)
- How you installed (Grove linux / `build.sh`)
- Which public preset you summoned (purelymail / google / disroot / outlook / yahoo)
- `goblin steal` then `goblin peek` — what you saw
- What broke

Steps:

1. `goblin --help`
2. `goblin summon` — pick a sky, **your** address, password (app password if the house asks).
3. `goblin steal` then `goblin peek`
4. `goblin` opens the horde. `q` quits. `h` is help if the TUI offers it.
5. `goblin who` names the nest.

Do not paste passwords, mail bodies, or account JSON into the report.

## How we test on the development box

Isolated `HOME`: `--help` and `who` (empty nest prints `no goblins yet`).
`cargo test --offline`. We do not `steal` against house mail in the
publish smoke.

## What we do not need in a first report

Office LAN, hire desk, notify sound, aerc import. Those live on the
development box. No passwords, keys, or client names.
