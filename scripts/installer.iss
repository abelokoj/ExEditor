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
SetupIconFile=..\assets\ExEditor.ico
UninstallDisplayIcon={app}\ExEditor.exe,0
CloseApplications=yes
RestartApplications=no
ChangesAssociations=yes
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "openwith"; Description: "Add ExEditor to the Open with menu (keep current defaults)"; Flags: unchecked
Name: "contextmenu"; Description: "Add Edit with ExEditor to File Explorer"; Flags: checkedonce
[Files]
Source: "{#AppSource}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{group}\ExEditor"; Filename: "{app}\ExEditor.exe"; IconFilename: "{app}\ExEditor.exe"; IconIndex: 0
Name: "{autodesktop}\ExEditor"; Filename: "{app}\ExEditor.exe"; IconFilename: "{app}\ExEditor.exe"; IconIndex: 0; Tasks: desktopicon
[Registry]
Root: HKCU; Subkey: "Software\Classes\*\shell\ExEditor.Edit"; ValueType: string; ValueData: "Edit with ExEditor"; Flags: uninsdeletekey; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\*\shell\ExEditor.Edit"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\ExEditor.exe"",0"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\*\shell\ExEditor.Edit"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Document"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\*\shell\ExEditor.Edit\command"; ValueType: string; ValueData: """{app}\ExEditor.exe"" ""%1"""; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ExEditor.Edit"; ValueType: string; ValueData: "Edit with ExEditor"; Flags: uninsdeletekey; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ExEditor.Edit"; ValueType: string; ValueName: "AppliesTo"; ValueData: "System.FileExtension:="".rtfd"""; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ExEditor.Edit"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\ExEditor.exe"",0"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ExEditor.Edit"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Document"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ExEditor.Edit\command"; ValueType: string; ValueData: """{app}\ExEditor.exe"" ""%1"""; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\*\shell\ExEditor.Edit"; Flags: deletekey; Tasks: not contextmenu
Root: HKCU; Subkey: "Software\Classes\Directory\shell\ExEditor.Edit"; Flags: deletekey; Tasks: not contextmenu
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "ExEditor"; Flags: uninsdeletekey; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\shell\open\command"; ValueType: string; ValueData: """{app}\ExEditor.exe"" ""%1"""; Tasks: openwith
; Common-format Open With hints only; the wildcard Edit verb also handles other text/source extensions.
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".txt"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".rtf"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".rtfd"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".html"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".htm"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".docx"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".odt"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".md"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".markdown"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".mdown"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".mkd"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\Applications\ExEditor.exe\SupportedTypes"; ValueType: string; ValueName: ".mkdn"; ValueData: ""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\ExEditor.Document\DefaultIcon"; ValueType: string; ValueData: """{app}\ExEditor.exe"",0"; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\ExEditor.Document"; ValueType: string; ValueData: "ExEditor document"; Flags: uninsdeletekey; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\ExEditor.Document\shell\open\command"; ValueType: string; ValueData: """{app}\ExEditor.exe"" ""%1"""; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.txt\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.rtf\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.rtfd\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.html\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.htm\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.docx\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.odt\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.mdown\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.mkd\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
Root: HKCU; Subkey: "Software\Classes\.mkdn\OpenWithProgids"; ValueType: string; ValueName: "ExEditor.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: openwith
[Run]
Filename: "{app}\ExEditor.exe"; Description: "Launch ExEditor"; Flags: nowait postinstall skipifsilent
