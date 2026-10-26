; The installer of ainitShot (decision 13): Inno Setup 6.7 in its dark look, for the current user,
; without administrator rights, into %LOCALAPPDATA%\Programs\ainitShot, with a shortcut in the
; Start menu and an entry in Settings > Apps. Its first page offers to start the program with
; Windows; its last one, to run it. It speaks the language of Windows, if the program does, and
; leaves the language it spoke for the program, which speaks it too (decision 11).
;
; The program updates itself by running it so, and every installer from 0.2.1 on takes this:
;   ainitShot-X.Y.Z-windows-x64-setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /UPDATE
; It closes the program, replaces it, leaves starting with Windows as it is and starts the new
; version with --updated.
;
; ci/build.sh builds it on Linux under Wine: tools/installer-texts.py writes languages.iss, then
;   ISCC /DVersion=X.Y.Z /DExe=<ainitshot.exe> /O<folder> /F<name> installer\ainitShot.iss

#ifndef Version
  #error ISCC needs the version and the program: /DVersion=X.Y.Z /DExe=<ainitshot.exe>
#endif
#ifndef Exe
  #error ISCC needs the version and the program: /DVersion=X.Y.Z /DExe=<ainitshot.exe>
#endif

[Setup]
AppId=ainitShot
AppName=ainitShot
AppVersion={#Version}
AppVerName=ainitShot {#Version}
AppPublisher=ainit
AppPublisherURL=https://shot.ainit.tech
AppSupportURL=https://shot.ainit.tech
AppUpdatesURL=https://shot.ainit.tech
AppCopyright=GPL-3.0-or-later
VersionInfoVersion={#Version}
VersionInfoProductName=ainitShot
VersionInfoProductVersion={#Version}
VersionInfoDescription=ainitShot {#Version}
VersionInfoCompany=ainit
VersionInfoCopyright=GPL-3.0-or-later
; For the current user: %LOCALAPPDATA%\Programs\ainitShot, without asking where.
PrivilegesRequired=lowest
DefaultDirName={autopf}\ainitShot
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableWelcomePage=no
DisableReadyPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
; Dark, as the program is, with its pictures for every scale from 100 % to 250 %
; (installer/make-images.py).
WizardStyle=modern dark
WizardImageFile=images\large-100.png,images\large-125.png,images\large-150.png,images\large-175.png,images\large-200.png,images\large-225.png,images\large-250.png
WizardSmallImageFile=images\small-100.png,images\small-125.png,images\small-150.png,images\small-175.png,images\small-200.png,images\small-225.png,images\small-250.png
SetupIconFile=ainitShot.ico
UninstallDisplayIcon={app}\ainitShot.ico
UninstallDisplayName=ainitShot
; The language of Windows, without asking.
ShowLanguageDialog=no
LanguageDetectionMethod=uilanguage
; Older translations of Inno Setup carry messages it no longer has.
NotRecognizedMessagesWarning=no
; The installer closes the program itself, as "Exit" does.
CloseApplications=no
RestartApplications=no
Compression=lzma2/max
SolidCompression=yes
OutputDir=..\dist
OutputBaseFilename=ainitShot-{#Version}-windows-x64-setup

#include "languages.iss"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "ainitShot.exe"; Flags: ignoreversion
Source: "ainitShot.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\ainitShot"; Filename: "{app}\ainitShot.exe"; IconFilename: "{app}\ainitShot.ico"

[Registry]
; The language of the installation, whose names are the program's codes of languages: the program
; takes it away at its next start and speaks it, if its settings haven't chosen a language of
; their own. An update leaves the language as it is.
Root: HKCU; Subkey: "Software\ainitShot"; ValueType: string; ValueName: "Language"; ValueData: "{language}"; Check: not Updating; Flags: uninsdeletevalue uninsdeletekeyifempty

[Run]
; An update starts the new version, which says so; an installation runs it if its box is ticked.
Filename: "{app}\ainitShot.exe"; Parameters: "--updated"; Check: Updating; Flags: nowait
Filename: "{app}\ainitShot.exe"; Description: "{cm:Run}"; Check: not Updating; Flags: postinstall nowait skipifsilent

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\ainitShot_is1';
  WM_CLOSE = $0010;

var
  AutostartBox: TNewCheckBox;

{ Whether the program itself runs the installer to update: /UPDATE. }
function Updating: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/UPDATE') = 0 then
      Result := True;
end;

{ Whether ainitShot starts with Windows: it is in Run, and the list of programs that start with
  Windows, in Task Manager and in Settings, hasn't turned it off, which an odd first byte of its
  record there says. The program reads it so too. }
function StartsWithWindows: Boolean;
var
  Approved: AnsiString;
begin
  Result := RegValueExists(HKCU, RunKey, 'ainitShot');
  if Result and RegQueryBinaryValue(HKCU, ApprovedKey, 'ainitShot', Approved) and (Length(Approved) > 0) then
    Result := (Ord(Approved[1]) and 1) = 0;
end;

{ Starts ainitShot with Windows, or stops that, as the switch in its settings does. }
procedure SetAutostart(On: Boolean);
begin
  RegDeleteValue(HKCU, ApprovedKey, 'ainitShot');
  if On then
    RegWriteStringValue(HKCU, RunKey, 'ainitShot', '"' + ExpandConstant('{app}\ainitShot.exe') + '"')
  else
    RegDeleteValue(HKCU, RunKey, 'ainitShot');
end;

{ Closes a running ainitShot as "Exit" does, which lets its browser window go properly, and waits
  up to half a minute for it to end and let its mutex go. }
procedure CloseRunning;
var
  Window: HWND;
  Waited: Integer;
begin
  Window := FindWindowByClassName('ainitShot.main');
  if Window <> 0 then
    PostMessage(Window, WM_CLOSE, 0, 0);
  Waited := 0;
  while CheckForMutexes('Local\ainitShot') and (Waited < 120) do
  begin
    Sleep(250);
    Waited := Waited + 1;
  end;
end;

{ The first page: its title, the box of starting with Windows under it, and "Install". }
procedure InitializeWizard;
begin
  WizardForm.WelcomeLabel2.Visible := False;
  WizardForm.AdjustLabelHeight(WizardForm.WelcomeLabel1);
  AutostartBox := TNewCheckBox.Create(WizardForm);
  AutostartBox.Parent := WizardForm.WelcomePage;
  AutostartBox.Left := WizardForm.WelcomeLabel1.Left;
  AutostartBox.Top := WizardForm.WelcomeLabel1.Top + WizardForm.WelcomeLabel1.Height + ScaleY(20);
  AutostartBox.Width := WizardForm.WelcomeLabel1.Width;
  AutostartBox.Height := ScaleY(24);
  AutostartBox.Caption := CustomMessage('Autostart');
  { A new installation starts the program with Windows; one over an installed program shows
    and keeps what is set now. }
  AutostartBox.Checked := not RegKeyExists(HKCU, UninstallKey) or StartsWithWindows;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = wpWelcome then
    WizardForm.NextButton.Caption := SetupMessage(msgButtonInstall);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseRunning;
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  { An update leaves starting with Windows as it is. }
  if (CurStep = ssPostInstall) and not Updating then
    SetAutostart(AutostartBox.Checked);
end;

function InitializeUninstall: Boolean;
begin
  CloseRunning;
  Result := True;
end;

{ The uninstaller takes starting with Windows away, and the models and updates it downloaded,
  which are not settings and take up room; the settings and the sign-in to Google go only if the
  user says so. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    SetAutostart(False);
    DelTree(ExpandConstant('{localappdata}\ainitShot\models'), True, True, True);
    DelTree(ExpandConstant('{localappdata}\ainitShot\updates'), True, True, True);
    if not UninstallSilent and
      (MsgBox(CustomMessage('RemoveData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    begin
      DelTree(ExpandConstant('{userappdata}\ainitShot'), True, True, True);
      DelTree(ExpandConstant('{localappdata}\ainitShot'), True, True, True);
    end;
  end;
end;
