; Installer for the AAEmu sandbox launcher (Inno Setup 6).
;
; Build:   cargo build --release
;          iscc /DAppVersion=0.1.0 installer\aac-sandbox-launcher.iss
; Result:  installer\out\AAEmuSandboxSetup-<version>.exe
;
; Rules this script keeps:
; - per user, no administrator rights, nothing outside the user's own folders;
; - it installs the launcher only: no game files (the launcher gets those itself, from the player's AA Classic
;   folder or by download), nothing into an AA Classic folder, nothing into Documents\AAClassic;
; - uninstall removes the launcher and its start menu entry. The launcher's data folder
;   (%LOCALAPPDATA%\AAEmuSandbox: the client copy, kept builds, settings) is removed only when the player says yes.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#define AppName "AAEmu Sandbox Launcher"
#define AppExe "aac-sandbox-launcher.exe"
#define DataDir "{localappdata}\AAEmuSandbox"

[Setup]
AppId={{6E0B3C52-8D0C-4E1B-9A55-5B7C1F0A35A1}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=AAEmu
DefaultDirName={localappdata}\Programs\AAEmuSandboxLauncher
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
OutputDir=out
OutputBaseFilename=AAEmuSandboxSetup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{userdesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; what the launcher's own update leaves next to the exe
Type: files; Name: "{app}\*.old"
Type: files; Name: "{app}\*.new"

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    Data := ExpandConstant('{#DataDir}');
    if DirExists(Data) and not UninstallSilent then
    begin
      if MsgBox('Also remove the game copy, kept client builds and settings in' + #13#10 + Data + '?' + #13#10 + #13#10 +
                'This can be over 45 GB. Your AA Classic install is not touched either way.',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(Data, True, True, True);
    end;
  end;
end;
