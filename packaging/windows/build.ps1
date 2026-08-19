# Build Windows binaries + Inno Setup installer.
# Prerequisites: Rust toolchain, Inno Setup 6 (ISCC on PATH or default install path).

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Staging = Join-Path $PSScriptRoot "staging"
$Dist = Join-Path $Root "dist\windows"

Set-Location $Root
New-Item -ItemType Directory -Force -Path $Staging | Out-Null
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

Write-Host "Building release binaries..."
cargo build --release --bin goblin --bin goblin-gui
Copy-Item "target\release\goblin.exe" $Staging -Force
Copy-Item "target\release\goblin-gui.exe" $Staging -Force

@"
Goblin — mail client
====================

Desktop shortcut opens Goblin GUI (goblin-gui.exe).

Terminal / PowerShell (optional):
  goblin.exe who
  goblin.exe steal
  goblin.exe          # terminal TUI

Docs: https://github.com/ElegantVW/goblin
Purelymail migration checklist: docs/MIGRATE-PURELYMAIL.md
"@ | Set-Content -Path (Join-Path $Staging "README.txt") -Encoding UTF8

$Iscc = @(
  "${env:LocalAppData}\Programs\Inno Setup 6\ISCC.exe",
  "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
  "${env:ProgramFiles}\Inno Setup 6\ISCC.exe",
  "ISCC.exe"
) | Where-Object { $_ -eq "ISCC.exe" -or (Test-Path $_) } | Select-Object -First 1

if (-not $Iscc) {
  Write-Warning "ISCC not found — binaries are in $Staging (zip those for a manual install)."
  exit 0
}

Write-Host "Running Inno Setup: $Iscc"
& $Iscc (Join-Path $PSScriptRoot "goblin.iss")
Write-Host "Done. Installer under $Dist"
