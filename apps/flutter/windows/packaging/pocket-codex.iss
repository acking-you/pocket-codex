; Inno Setup script for the Pocket-Codex desktop app (Windows installer).
;
; Invoked from .github/workflows/release.yml, e.g.:
;   ISCC.exe /DAppVersion=0.0.1 ^
;            /DBuildDir=<abs path to build\windows\x64\runner\Release> ^
;            /DOutDir=<abs path to dist> ^
;            windows\packaging\pocket-codex.iss
;
; Produces <OutDir>\pocket-codex-app-setup.exe, which the workflow renames to
; pocket-codex-app-<tag>-windows-x64-setup.exe. Relative paths below resolve
; against this script's directory (apps/flutter/windows/packaging).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef BuildDir
  #define BuildDir "..\..\build\windows\x64\runner\Release"
#endif
#ifndef OutDir
  #define OutDir "dist"
#endif

#define MyAppName "Pocket-Codex"
#define MyAppExe "pocket_codex.exe"
#define MyAppPublisher "Pocket-Codex Contributors"
#define MyAppURL "https://github.com/acking-you/pocket-codex"

[Setup]
; A stable, unique AppId so upgrades replace the previous install in place.
AppId={{8F2B7C1E-6E3A-4D5B-9C0F-1A2B3C4D5E6F}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\{#MyAppExe}
OutputDir={#OutDir}
OutputBaseFilename=pocket-codex-app-setup
SetupIconFile=..\runner\resources\app_icon.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional icons:"

[Files]
Source: "{#BuildDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
