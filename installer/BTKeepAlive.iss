; Inno Setup script for BT KeepAlive.
; Build the exe first (on Windows):
;   cargo build --release -p btkeepalive-app
; then:
;   iscc installer\BTKeepAlive.iss
; Override the exe folder when cross-building:
;   iscc /DExeDir="..\target\x86_64-pc-windows-msvc\release" installer\BTKeepAlive.iss
#define MyAppName "BT KeepAlive"
#ifndef MyAppVersion
#define MyAppVersion "2.0.1"
#endif
#ifndef ExeDir
#define ExeDir "..\target\release"
#endif
#define MyAppPublisher "BT KeepAlive"
#define MyAppExeName "BTKeepAlive.exe"

[Setup]
AppId={{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL=https://github.com/kadato/bt-keepalive
AppSupportURL=https://github.com/kadato/bt-keepalive/issues
AppUpdatesURL=https://github.com/kadato/bt-keepalive/releases
VersionInfoVersion={#MyAppVersion}
VersionInfoProductName={#MyAppName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
CloseApplications=yes
DefaultDirName={localappdata}\Programs\BTKeepAlive
DefaultGroupName={#MyAppName}
OutputDir=..\dist
OutputBaseFilename=BTKeepAlive-setup
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=lowest
SetupIconFile=..\btkeepalive-app\icons\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}

[Files]
Source: "{#ExeDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ExeDir}\WebView2Loader.dll"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{userstartup}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: startup

[Tasks]
Name: startup; Description: "Launch BT KeepAlive at Windows startup"; GroupDescription: "Additional tasks:"; Flags: unchecked

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
