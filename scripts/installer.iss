#ifndef AppVersion
#define AppVersion "0.1.0"
#endif
#ifndef AppSource
#define AppSource "..\artifacts\ExEditor"
#endif
[Setup]
AppId={{AB30A2AE-13B6-4CDA-A937-3E61C122BA51}
AppName=ExEditor
AppVersion={#AppVersion}
AppPublisher=ExEditor contributors
AppPublisherURL=https://github.com/abelokoj/ExEditor
DefaultDirName={localappdata}\Programs\ExEditor
DefaultGroupName=ExEditor
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
DisableProgramGroupPage=yes
OutputDir=..\artifacts\installers
OutputBaseFilename=ExEditor-Setup-x64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\ExEditor.exe
CloseApplications=yes
RestartApplications=no
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "openwith"; Description: "Add ExEditor to the Open with menu (keep current defaults)"; Flags: unchecked
[Files]
Source: "{#AppSource}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{group}\ExEditor"; Filename: "{app}\ExEditor.exe"
Name: "{autodesktop}\ExEditor"; Filename: "{app}\ExEditor.exe"; Tasks: desktopicon
[Registry]
Root: HKCU; Subkey: "Software\Classes\ExEditor.Document"; ValueType: string; ValueData: "ExEditor document"; Flags: uninsdeletekey; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\ExEditor.Document\shell\open\command"; ValueType: string; ValueData: """{app}\ExEditor.exe"" ""%1"""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.txt\OpenWithProgids"; ValueType: none; ValueName: "ExEditor.Document"; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.rtf\OpenWithProgids"; ValueType: none; ValueName: "ExEditor.Document"; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.rtfd\OpenWithProgids"; ValueType: none; ValueName: "ExEditor.Document"; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.html\OpenWithProgids"; ValueType: none; ValueName: "ExEditor.Document"; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.htm\OpenWithProgids"; ValueType: none; ValueName: "ExEditor.Document"; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.docx\OpenWithProgids"; ValueType: none; ValueName: "ExEditor.Document"; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.odt\OpenWithProgids"; ValueType: none; ValueName: "ExEditor.Document"; Flags: uninsdeletevalue; Tasks: openwith
[Run]
Filename: "{app}\ExEditor.exe"; Description: "Launch ExEditor"; Flags: nowait postinstall skipifsilent
