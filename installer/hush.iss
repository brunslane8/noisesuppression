; Hush installer. Build with: ISCC.exe installer\hush.iss  (after cargo build --release)

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{2117B894-788E-4555-AFB9-20D731FCED90}
AppName=Hush
AppVersion={#AppVersion}
AppPublisher=Hush
DefaultDirName={autopf}\Hush
DisableProgramGroupPage=yes
DisableDirPage=yes
OutputDir=..\dist
OutputBaseFilename=Hush-Setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\hush.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=force

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"

[Files]
Source: "..\target\release\hush.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Hush"; Filename: "{app}\hush.exe"
Name: "{autodesktop}\Hush"; Filename: "{app}\hush.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Hush"; ValueData: """{app}\hush.exe"" --hidden"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\hush.exe"; Description: "Start Hush now"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{app}\hush.exe"; Parameters: "--restore"; Flags: runhidden waituntilterminated; RunOnceId: "RestoreMic"
Filename: "{sys}\taskkill.exe"; Parameters: "/IM hush.exe /F"; Flags: runhidden waituntilterminated; RunOnceId: "StopHush"

[Code]
const
  RenderKey = 'SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render';
  DescProp = '{a45c254e-df1c-4efd-8020-67d146a850e0},2';

var
  DownloadPage: TDownloadWizardPage;
  CableDownloaded: Boolean;
  CableInstalled: Boolean;

// Is VB-CABLE already installed? Look for its "CABLE Input" speaker.
function CablePresent: Boolean;
var
  Keys: TArrayOfString;
  I: Integer;
  Desc: String;
begin
  Result := False;
  if RegGetSubkeyNames(HKLM64, RenderKey, Keys) then
    for I := 0 to GetArrayLength(Keys) - 1 do
      if RegQueryStringValue(HKLM64, RenderKey + '\' + Keys[I] + '\Properties', DescProp, Desc) then
        if Pos('CABLE Input', Desc) > 0 then
        begin
          Result := True;
          Exit;
        end;
end;

procedure InitializeWizard;
begin
  DownloadPage := CreateDownloadPage('Getting the audio driver',
    'Hush uses the free VB-CABLE driver to work in every app. Downloading it from vb-audio.com...', nil);
end;

function TryDownload(const Url: String): Boolean;
begin
  Result := False;
  DownloadPage.Clear;
  DownloadPage.Add(Url, 'vbcable.zip', '');
  try
    DownloadPage.Download;
    Result := True;
  except
    Log('Download failed: ' + Url + ' ' + GetExceptionMessage);
  end;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = wpReady) and not CablePresent then
  begin
    DownloadPage.Show;
    try
      CableDownloaded := TryDownload('https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip')
        or TryDownload('https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack43.zip');
    finally
      DownloadPage.Hide;
    end;
    if not CableDownloaded then
      SuppressibleMsgBox('Hush could not download its audio driver (VB-CABLE). Check your internet connection and run the installer again,' +
        ' or install VB-CABLE yourself from https://vb-audio.com/Cable/', mbError, MB_OK, IDOK);
  end;
end;

procedure InstallCable;
var
  Dir: String;
  Code: Integer;
begin
  Dir := ExpandConstant('{tmp}\vbcable');
  WizardForm.StatusLabel.Caption := 'Installing the audio driver. If Windows asks, click Install.';
  Exec('powershell.exe', '-NoProfile -ExecutionPolicy Bypass -Command "Expand-Archive -Force -LiteralPath ''' +
    ExpandConstant('{tmp}\vbcable.zip') + ''' -DestinationPath ''' + Dir + '''"', '', SW_HIDE, ewWaitUntilTerminated, Code);
  if not FileExists(Dir + '\VBCABLE_Setup_x64.exe') then
  begin
    SuppressibleMsgBox('The audio driver download looks damaged. Please install VB-CABLE from https://vb-audio.com/Cable/', mbError, MB_OK, IDOK);
    Exit;
  end;
  if Exec(Dir + '\VBCABLE_Setup_x64.exe', '-i -h', Dir, SW_SHOWNORMAL, ewWaitUntilTerminated, Code) then
    CableInstalled := CablePresent or (Code = 0);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and CableDownloaded then
    InstallCable;
end;

// A fresh driver works best after a restart; Hush starts by itself afterwards.
function NeedRestart: Boolean;
begin
  Result := CableInstalled;
end;
