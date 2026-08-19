# Windows installer

## Why there is no `.exe` on the pen / Linux box yet

Building `Goblin-Setup-0.1.0.exe` needs a **Windows** environment:

| Tool | Purpose |
|------|---------|
| **Rust** (`rustup`) | Compiles `goblin.exe` + `goblin-gui.exe` |
| **Inno Setup 6** (`ISCC.exe`) | Wraps those binaries into the Setup installer + Desktop shortcut |

Linux cannot usefully produce that installer without Wine/MinGW cross-tooling we do not ship. The scripts under this folder are the recipe; CI builds the artifact.

## Easiest: download from GitHub Actions

1. Push/open the `goblind` (or `main`) branch.
2. Open **Actions → windows-installer →** latest green run.
3. Download artifact **`goblin-windows`** (contains `Goblin-Setup-0.1.0.exe`).
4. On the Windows PC: run the Setup → Desktop shortcut opens **GUI only**.

Or: **Actions → Run workflow** (`workflow_dispatch`) on that workflow.

## Build on a Windows PC yourself

```powershell
# 1) Install Rust: https://rustup.rs
# 2) Install Inno Setup 6: https://jrsoftware.org/isinfo.php
# 3) From a clone of this repo:
cd path\to\goblin
.\packaging\windows\build.ps1
# → dist\windows\Goblin-Setup-0.1.0.exe
```

If Inno Setup is missing, `build.ps1` still leaves `packaging\windows\staging\goblin-gui.exe` (and `goblin.exe`) you can copy manually and pin to the Desktop.

## Shortcut policy

- Desktop / Start Menu → **`goblin-gui.exe`**
- Terminal UI → run **`goblin.exe`** from PowerShell / Windows Terminal (no separate TUI shortcut)
