; Per-user installer for the Windows tray app; no administrator rights. build.ps1 passes the
; version, the built program folder and the icon, and compiles it with Inno Setup 6.5 or later.
#ifndef AppVersion
  #error Pass /DAppVersion=x.y.z /DAppDir=<built folder> /DIconFile=<icon>
#endif

[Setup]
; Never change the AppId: upgrades and the uninstall entry are found by it.
AppId={{66818B0F-B59E-45CC-81AA-99315BFB1931}
AppName=Starmeter
AppVersion={#AppVersion}
AppVerName=Starmeter {#AppVersion}
AppPublisher=minicoke
AppPublisherURL=https://github.com/timecatbit/starmeter
AppSupportURL=https://github.com/timecatbit/starmeter/issues
AppUpdatesURL=https://github.com/timecatbit/starmeter/releases
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
; With PrivilegesRequired=lowest this is %LOCALAPPDATA%\Programs\Starmeter. An upgrade keeps the folder it
; found, so copies installed as Token Monitor stay in %LOCALAPPDATA%\Programs\Token Monitor.
DefaultDirName={autopf}\Starmeter
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
; The running tray holds this mutex. Setup and the uninstaller ask to quit it from the tray menu,
; which also waits for a CLI update in progress, rather than replacing files in use.
AppMutex=TokenMonitorTray
CloseApplications=no
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\Starmeter.exe
UninstallDisplayName=Starmeter
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ShowLanguageDialog=no

[Languages]
Name: "zh"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "登录 Windows 时启动"; Flags: unchecked

[InstallDelete]
; A newer build may drop libraries; the previous runtime must not stay mixed in with this one.
Type: filesandordirs; Name: "{app}\_internal"
Type: filesandordirs; Name: "{app}\licenses"
; Copies installed as Token Monitor gave the program and its shortcuts that name.
Type: files; Name: "{app}\Token Monitor.exe"
Type: files; Name: "{autoprograms}\Token Monitor.lnk"
Type: files; Name: "{autodesktop}\Token Monitor.lnk"

[Files]
Source: "{#AppDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
; The same AppUserModelID as the app, so its notifications carry its name and icon.
Name: "{autoprograms}\Starmeter"; Filename: "{app}\Starmeter.exe"; AppUserModelID: "timecatbit.TokenMonitor"
Name: "{autodesktop}\Starmeter"; Filename: "{app}\Starmeter.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Starmeter.exe"; Description: "{cm:LaunchProgram,Starmeter}"; Flags: nowait postinstall skipifsilent

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  RunValue = 'TokenMonitor';

function InstalledExe: String;
begin
  Result := ExpandConstant('{app}\Starmeter.exe');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  { The entry the tray menu writes. One that already exists now starts this copy, so a choice made
    with the ZIP build carries over; only the task adds a new one. }
  if (CurStep = ssPostInstall) and (WizardIsTaskSelected('autostart') or RegValueExists(HKCU, RunKey, RunValue)) then
    RegWriteStringValue(HKCU, RunKey, RunValue, '"' + InstalledExe + '" --background');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command: String;
begin
  { Remove the sign-in entry only when it starts this copy. }
  if (CurUninstallStep = usUninstall) and RegQueryStringValue(HKCU, RunKey, RunValue, Command) and
     (Pos(Lowercase(InstalledExe), Lowercase(Command)) > 0) then
    RegDeleteValue(HKCU, RunKey, RunValue);
end;
