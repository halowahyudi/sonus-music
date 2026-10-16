; Inno Setup script for the Sonus Windows installer.
;
; Build:
;   1. cargo build --release --target x86_64-pc-windows-msvc
;   2. Open this file in Inno Setup (or run ISCC.exe sonus.iss from CI)
;
; The installer copies the single sonus.exe. yt-dlp and ffmpeg are NOT bundled:
; the app downloads them into the user's data dir on first run. That keeps the
; installer tiny and means the tools are always updatable without reinstalling.

#ifndef MyAppVersion
  #define MyAppVersion "0.3.0"
#endif
#ifndef MyAppDir
  #define MyAppDir "..\..\target\release"
#endif

#define MyAppName "Sonus"
#define MyAppPublisher "Sonus Contributors"
#define MyAppExeName "sonus.exe"

[Setup]
AppId={{7E3B4C1A-9D0E-4F3B-8A56-2D9C6E5A7B1F}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\Sonus
DefaultGroupName=Sonus
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=sonus-{#MyAppVersion}-setup
SetupIconFile=..\assets\sonus.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#MyAppDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
; MIT license text ships alongside the binary.
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Sonus"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\Sonus"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,Sonus}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{userappdata}\sonus"
