; Inno Setup 6 — Goblin Windows installer
; Desktop + Start Menu open goblin-gui.exe only.
; Terminal UI: run goblin.exe from PowerShell / Windows Terminal.

#define MyAppName "Goblin"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "ElegantVW / faeOS"
#define MyAppURL "https://github.com/ElegantVW/goblin"
#define MyAppExeName "goblin-gui.exe"

[Setup]
AppId={{A7C3E8D1-9B2F-4E6A-8C1D-0F5B7A2E9D43}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
DefaultDirName={autopf}\Goblin
DefaultGroupName=Goblin
DisableProgramGroupPage=yes
OutputDir=..\..\dist\windows
OutputBaseFilename=Goblin-Setup-{#MyAppVersion}
SetupIconFile=goblin.ico
Compression=lzma
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &Desktop shortcut"; GroupDescription: "Additional icons:"

[Files]
; Built by packaging/windows/build.ps1 into staging\
Source: "staging\goblin-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "staging\goblin.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "staging\README.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch Goblin"; Flags: nowait postinstall skipifsilent
