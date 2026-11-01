; 安装、升级和卸载共用此定义；AppId 必须保持稳定，用户数据位于安装目录之外。
#define AppFileVersion GetVersionNumbersString(PublishDir + "\Sayso.exe")

[Setup]
AppId=Sayso
AppName=Sayso
AppVersion={#AppVersion}
AppPublisher=Sayso
AppPublisherURL=https://github.com/moayuisuda/Sayso
AppSupportURL=https://github.com/moayuisuda/Sayso/issues
AppUpdatesURL=https://github.com/moayuisuda/Sayso/releases/latest
VersionInfoVersion={#AppFileVersion}
DefaultDirName={localappdata}\Programs\Sayso
DefaultGroupName=Sayso
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
; 与 SingleInstance 创建的命名互斥量一致，安装和卸载均要求先退出托盘程序。
AppMutex=Local\Sayso.Windows.{username}
SetupIconFile={#PublishDir}\Assets\Sayso.ico
UninstallDisplayIcon={app}\Sayso.exe
ChangesAssociations=yes
WizardStyle=modern
Compression=lzma2/ultra64
SolidCompression=yes
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Files]
Source: "{#PublishDir}\*"; DestDir: "{app}"; Excludes: "*.pdb,smoke-results.json"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Sayso"; Filename: "{app}\Sayso.exe"; WorkingDir: "{app}"

[Registry]
; 注册打开方式，不覆盖用户已选择的默认应用。
Root: HKCU; Subkey: "Software\Classes\.sayso"; Flags: uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.sayso\OpenWithProgids"; ValueType: string; ValueName: "Sayso.Extension"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\Sayso.Extension"; ValueType: string; ValueData: "Sayso Extension"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Sayso.Extension\DefaultIcon"; ValueType: string; ValueData: "{app}\Sayso.exe,0"
Root: HKCU; Subkey: "Software\Classes\Sayso.Extension\shell\open\command"; ValueType: string; ValueData: """{app}\Sayso.exe"" ""%1"""

[Run]
Filename: "{app}\Sayso.exe"; Description: "{cm:LaunchProgram,Sayso}"; Flags: nowait postinstall skipifsilent
