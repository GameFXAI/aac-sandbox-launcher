; Installer for the AA Replay launcher (Inno Setup 6).
;
; Build:   cargo build --release
;          iscc /DAppVersion=0.2.1 installer\aac-sandbox-launcher.iss
; Result:  installer\out\AAReplaySetup.exe
;
; One installer for every tester. The account API hands it out as AAReplaySetup-<invite code>.exe (the same bytes
; under a name with the tester's own code); this installer reads the code from its own file name and leaves it in
; the launcher's folder (invite.txt), so the first PLAY makes the account without anyone typing a code.
; For a test: AAReplaySetup.exe /INVITE=<code> does the same.
;
; Rules this script keeps:
; - per user, no administrator rights, nothing outside the user's own folders;
; - it installs the launcher only: no game files (the launcher makes its own copy from the player's AA Classic
;   folder), nothing into an AA Classic folder, nothing into Documents\AAClassic;
; - the AppId is the one of the first installer (AAEmu Sandbox Launcher), so an earlier install is upgraded in place;
; - uninstall removes the launcher and its shortcuts. The launcher's data folder (%LOCALAPPDATA%\AAEmuSandbox: the
;   client copy, kept builds, settings, the remembered sign-in) and a copy on AA Classic's drive (X:\AAReplay\client)
;   are removed only when the player says yes.

#ifndef AppVersion
  #define AppVersion "0.2.1"
#endif
#define AppName "AA Replay"
#define AppExe "aac-sandbox-launcher.exe"
#define DataDir "{localappdata}\AAEmuSandbox"
#define InviteAlphabet "ABCDEFGHJKMNPQRSTUVWXYZ23456789"

[Setup]
; the same id as every earlier installer: never change it
AppId={{6E0B3C52-8D0C-4E1B-9A55-5B7C1F0A35A1}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=AA Replay
; an earlier install keeps its folder (UsePreviousAppDir); a new one goes here
DefaultDirName={localappdata}\Programs\AAReplay
DefaultGroupName={#AppName}
; the Start menu folder is "AA Replay" even after an install that had the first installer's folder name
UsePreviousGroup=no
DisableWelcomePage=yes
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=out
OutputBaseFilename=AAReplaySetup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
CloseApplications=yes
RestartApplications=no
; Authenticode signing of the setup and its uninstaller, only when release.ps1 compiles with /DSign and passes the
; "aareplay" sign tool (installer\sign.ps1 -Require). Without it the installer is unsigned, as before.
#ifdef Sign
SignTool=aareplay
SignedUninstaller=yes
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
ReadyLabel1=AA Replay will be installed for you only (no administrator rights).
ReadyLabel2a=Your AA Classic install is not changed. Click Install, and the launcher opens when it is done.

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{userdesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"
; the first installer's names, from an install before this one
[InstallDelete]
Type: files; Name: "{group}\AAEmu Sandbox Launcher.lnk"
Type: files; Name: "{userdesktop}\AAEmu Sandbox Launcher.lnk"
Type: files; Name: "{userprograms}\AAEmu Sandbox Launcher\AAEmu Sandbox Launcher.lnk"
Type: files; Name: "{userprograms}\AAEmu Sandbox Launcher\AA Replay.lnk"
Type: dirifempty; Name: "{userprograms}\AAEmu Sandbox Launcher"

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

; recording AA Classic (recorder.rs): stopped and taken out of Windows start before the files go
[UninstallRun]
Filename: "{app}\{#AppExe}"; Parameters: "recorder off"; Flags: runhidden waituntilterminated; RunOnceId: "RecorderOff"

[Registry]
; the recorder's sign-in entry, in case the launcher could not remove it
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "AA Replay Recorder"; Flags: uninsdeletevalue dontcreatekey

[UninstallDelete]
; what the launcher's own update leaves next to the exe
Type: files; Name: "{app}\*.old"
Type: files; Name: "{app}\*.new"

[Code]
{ The invite in "AAReplaySetup-<code>.exe" (a browser may add " (1)" after it), or in /INVITE=<code>. }
function ValidInvite(Code: String): Boolean;
var
  I: Integer;
begin
  Result := Length(Code) = 20;
  if Result then
    for I := 1 to 20 do
      if Pos(Code[I], '{#InviteAlphabet}') = 0 then
        Result := False;
end;

function InviteCode(): String;
var
  Name: String;
  I: Integer;
begin
  Result := Uppercase(Trim(ExpandConstant('{param:INVITE|}')));
  if ValidInvite(Result) then
    Exit;
  Result := '';
  Name := Uppercase(ExtractFileName(ExpandConstant('{srcexe}')));
  I := Pos('AAREPLAYSETUP-', Name);
  if I > 0 then
  begin
    Name := Copy(Name, I + Length('AAREPLAYSETUP-'), 20);
    if ValidInvite(Name) then
      Result := Name;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Code, Dir: String;
begin
  if CurStep = ssPostInstall then
  begin
    Code := InviteCode();
    if Code <> '' then
    begin
      Dir := ExpandConstant('{#DataDir}');
      ForceDirectories(Dir);
      SaveStringToFile(Dir + '\invite.txt', Code, False);
      Log('invite left for the launcher');
    end;
  end;
end;

{ The copy on AA Classic's drive, when the launcher made one there (its folder is always X:\...\AAReplay\client). }
function OtherDriveCopy(): String;
var
  Lines: AnsiString;
  Dir: String;
begin
  Result := '';
  if LoadStringFromFile(ExpandConstant('{#DataDir}') + '\client-dir.txt', Lines) then
  begin
    Dir := Trim(String(Lines));
    if (Length(Dir) > 16) and (CompareText(Copy(Dir, Length(Dir) - 15, 16), '\AAReplay\client') = 0) and DirExists(Dir) then
      Result := Dir;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data, Other, Also: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    Data := ExpandConstant('{#DataDir}');
    Other := OtherDriveCopy();
    Also := '';
    if Other <> '' then
      Also := #13#10 + 'and ' + Other;
    if (DirExists(Data) or (Other <> '')) and not UninstallSilent then
    begin
      if MsgBox('Also remove the game copy, kept client builds, settings and your saved sign-in in' + #13#10 + Data +
                Also + '?' + #13#10 + #13#10 +
                'This can be over 45 GB. Your AA Classic install is not touched either way.',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      begin
        if Other <> '' then
        begin
          DelTree(Other, True, True, True);
          RemoveDir(ExtractFileDir(Other));
        end;
        DelTree(Data, True, True, True);
      end;
    end;
  end;
end;
