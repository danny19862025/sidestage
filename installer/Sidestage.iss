; Inno Setup 6 script for Sidestage on Windows.
; Build: cargo build --release -p sidestage
;        ISCC /DAppVersion=x.y.z installer\Sidestage.iss
; Output: dist\Sidestage-Setup-<version>.exe
;
; Installs per user by default (no administrator prompt); an all-users
; install can be chosen in the first dialog. No drivers or services.
; Settings (%APPDATA%\Sidestage) are kept on uninstall.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "Sidestage"
#define AppExe "sidestage.exe"

[Setup]
AppId={{B3E5C1A7-6D2F-4C88-9E41-7A0D5F2C8B19}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Sidestage contributors
AppPublisherURL=https://github.com/danny19862025/sidestage
AppSupportURL=https://github.com/danny19862025/sidestage/issues
AppUpdatesURL=https://github.com/danny19862025/sidestage/releases
DefaultDirName={autopf}\Sidestage
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 1809 or later (Windows 11 recommended).
MinVersion=10.0.17763
LicenseFile=..\LICENSE
SetupIconFile=..\packaging\windows\sidestage.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir=..\dist
OutputBaseFilename=Sidestage-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "Start Sidestage when I sign in"; GroupDescription: "Startup:"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autoprograms}\{#AppName} Settings"; Filename: "{app}\{#AppExe}"; Parameters: "--settings"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Same value the app's own "Launch at login" setting writes, so the setting
; shows as on and can be turned off there. Removed on uninstall either way.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Sidestage"; ValueData: """{app}\{#AppExe}"" --autostart"; Tasks: autostart; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Sidestage"; Flags: uninsdeletevalue dontcreatekey

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Quit cleanly first so the reserved screen space is released.
Filename: "{app}\{#AppExe}"; Parameters: "--quit"; Flags: runhidden waituntilterminated; RunOnceId: "QuitSidestage"
Filename: "{sys}\taskkill.exe"; Parameters: "/IM {#AppExe} /F"; Flags: runhidden; RunOnceId: "StopSidestage"

[Code]
// Before upgrading, ask a running Sidestage to quit (releases its
// reserved screen space) instead of only relying on Restart Manager.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
  Exe: String;
begin
  Result := '';
  Exe := ExpandConstant('{app}\{#AppExe}');
  if FileExists(Exe) then
  begin
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Sleep(1500);
  end;
end;
