[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [string]$LogDirectory = 'logs'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $IsWindows -or -not $env:RUNNER_TEMP) {
    throw 'Installer registration checks run only on a Windows GitHub Actions runner.'
}
if (-not [Environment]::Is64BitProcess) { throw 'Use 64-bit PowerShell for the x64 installer checks.' }

$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$installerFile = (Resolve-Path -LiteralPath $InstallerPath).ProviderPath
$runnerTemp = (Resolve-Path -LiteralPath $env:RUNNER_TEMP).ProviderPath.TrimEnd('\')
$testRoot = [IO.Path]::GetFullPath((Join-Path $runnerTemp ('ExEditor shell check 漢字 ' + [Guid]::NewGuid().ToString('N'))))
$installDirectory = Join-Path $testRoot 'Application with spaces Ω'
$logRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot $LogDirectory))
if (-not $testRoot.StartsWith($runnerTemp + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The installer test directory must stay inside RUNNER_TEMP.'
}
if (-not $logRoot.StartsWith($projectRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Installer test logs must stay inside the project.'
}
if (Test-Path -LiteralPath $testRoot) { throw 'The unique installer test directory already exists.' }
New-Item -ItemType Directory -Path $testRoot, $logRoot -Force | Out-Null

$registry = @{
    User = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry64)
    Machine = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, [Microsoft.Win32.RegistryView]::Registry64)
    Merged = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::ClassesRoot, [Microsoft.Win32.RegistryView]::Registry64)
}
$extensions = @('.txt', '.rtf', '.rtfd', '.html', '.htm', '.docx', '.odt', '.md', '.markdown', '.mdown', '.mkd', '.mkdn')
$defaultExtensions = @('.txt', '.rtf', '.md', '.docx')
$menuKeys = @('Software\Classes\*\shell\ExEditor.Edit', 'Software\Classes\Directory\shell\ExEditor.Edit')
$applicationKey = 'Software\Classes\Applications\ExEditor.exe'
$documentKey = 'Software\Classes\ExEditor.Document'
$uninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{AB30A2AE-13B6-4CDA-A937-3E61C122BA51}_is1'
$appFile = Join-Path $installDirectory 'ExEditor.exe'
$expectedCommand = '"{0}" "%1"' -f $appFile
$expectedIcon = '"{0}",0' -f $appFile
$originalDefaults = @{}
$fixtureName = 'ExEditorShellCheck.Existing.' + [Guid]::NewGuid().ToString('N')
$fixtureKey = 'Software\Classes\.md\OpenWithProgids'
$fixtureCreated = $false
$finishedUninstall = $false
$results = [Collections.Generic.List[string]]::new()

function Read-RegistryValue([string]$Hive, [string]$Subkey, [string]$Name = '') {
    $key = $registry[$Hive].OpenSubKey($Subkey)
    try {
        if ($null -eq $key -or $Name -notin $key.GetValueNames()) {
            return [pscustomobject]@{ Exists = $false; Kind = $null; Data = $null }
        }
        return [pscustomobject]@{
            Exists = $true
            Kind = [string]$key.GetValueKind($Name)
            Data = $key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        }
    } finally {
        if ($null -ne $key) { $key.Dispose() }
    }
}

function Assert-KeyAbsent([string]$Subkey) {
    $key = $registry.User.OpenSubKey($Subkey)
    if ($null -ne $key) {
        $key.Dispose()
        throw "Unexpected ExEditor registry key: $Subkey"
    }
}

function Assert-Value([string]$Subkey, [string]$Name, [string]$Expected) {
    $actual = Read-RegistryValue 'User' $Subkey $Name
    if (-not $actual.Exists -or $actual.Kind -ne 'String' -or [string]$actual.Data -cne $Expected) {
        throw "Incorrect registry value: $Subkey [$Name]"
    }
}

function Association-State {
    $state = [ordered]@{}
    foreach ($extension in $defaultExtensions) {
        $state["$extension.user"] = Read-RegistryValue 'User' "Software\Classes\$extension"
        $state["$extension.machine"] = Read-RegistryValue 'Machine' "Software\Classes\$extension"
        $state["$extension.effective"] = Read-RegistryValue 'Merged' $extension
        $choice = "Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$extension\UserChoice"
        $state["$extension.userChoice"] = Read-RegistryValue 'User' $choice 'ProgId'
        $state["$extension.userChoiceHash"] = Read-RegistryValue 'User' $choice 'Hash'
    }
    $state | ConvertTo-Json -Depth 6 -Compress
}

function Assert-Defaults([string]$Stage) {
    if ((Association-State) -cne $script:expectedDefaults) {
        throw "Default file associations changed during $Stage."
    }
    Assert-Value $fixtureKey $fixtureName ''
}

function Invoke-Hidden([string]$Executable, [string[]]$Arguments) {
    # Inno may delegate uninstall work to a child; -Wait includes its descendants.
    $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WindowStyle Hidden -PassThru -Wait
    try {
        $process.Refresh()
        if ($process.ExitCode -ne 0) { throw "Silent installer operation failed: exit code $($process.ExitCode)." }
    } finally {
        $process.Dispose()
    }
}

function Invoke-Setup([string]$Tasks, [string]$Stage) {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/NOICONS',
        ('/DIR="' + $installDirectory + '"'), ('/LOG="' + (Join-Path $logRoot "shell-integration-$Stage.log") + '"'), $Tasks)
    Invoke-Hidden $installerFile $arguments
    if (-not (Test-Path -LiteralPath $appFile -PathType Leaf)) { throw 'The installer did not install ExEditor.exe.' }
    if (Get-Process -Name ExEditor -ErrorAction SilentlyContinue) { throw 'The silent installer unexpectedly launched the app.' }
}

function Assert-Menus {
    foreach ($key in $menuKeys) {
        Assert-Value $key '' 'Edit with ExEditor'
        Assert-Value $key 'Icon' $expectedIcon
        Assert-Value $key 'MultiSelectModel' 'Document'
        Assert-Value ($key + '\command') '' $expectedCommand
    }
    Assert-Value $menuKeys[1] 'AppliesTo' 'System.FileExtension:=".rtfd"'
}

function Assert-OpenWith {
    Assert-Value $applicationKey 'FriendlyAppName' 'ExEditor'
    Assert-Value ($applicationKey + '\shell\open\command') '' $expectedCommand
    Assert-Value $documentKey '' 'ExEditor document'
    Assert-Value ($documentKey + '\DefaultIcon') '' $expectedIcon
    Assert-Value ($documentKey + '\shell\open\command') '' $expectedCommand
    foreach ($extension in $extensions) {
        Assert-Value "Software\Classes\$extension\OpenWithProgids" 'ExEditor.Document' ''
        Assert-Value ($applicationKey + '\SupportedTypes') $extension ''
    }
}

function Invoke-Uninstall([string]$Stage) {
    $uninstaller = Join-Path $installDirectory 'unins000.exe'
    if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) { throw 'The per-user uninstaller is missing.' }
    Invoke-Hidden $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
        ('/LOG="' + (Join-Path $logRoot "shell-integration-$Stage.log") + '"'))
}

try {
    foreach ($key in ($menuKeys + @($applicationKey, $documentKey, $uninstallKey))) { Assert-KeyAbsent $key }
    if (Get-Process -Name ExEditor -ErrorAction SilentlyContinue) { throw 'The runner already has ExEditor running.' }
    foreach ($extension in $extensions) {
        if ((Read-RegistryValue 'User' "Software\Classes\$extension\OpenWithProgids" 'ExEditor.Document').Exists) {
            throw 'Use a clean runner without existing ExEditor Open With registrations.'
        }
    }
    foreach ($extension in $defaultExtensions) {
        $originalDefaults[$extension] = Read-RegistryValue 'User' "Software\Classes\$extension"
        if (-not $originalDefaults[$extension].Exists) {
            $key = $registry.User.CreateSubKey("Software\Classes\$extension")
            try { $key.SetValue('', "ShellCheck.Existing$extension", [Microsoft.Win32.RegistryValueKind]::String) }
            finally { $key.Dispose() }
        }
    }
    $key = $registry.User.CreateSubKey($fixtureKey)
    try { $key.SetValue($fixtureName, '', [Microsoft.Win32.RegistryValueKind]::String); $fixtureCreated = $true }
    finally { $key.Dispose() }
    $script:expectedDefaults = Association-State

    Invoke-Setup '/MERGETASKS="openwith"' 'install-default'
    Assert-Menus
    Assert-OpenWith
    Assert-Defaults 'initial installation'
    $results.Add('Default context menu and Open With registration passed with spaces and Unicode in the installation path.')

    Invoke-Setup '/TASKS="openwith"' 'deselect-menu'
    foreach ($key in $menuKeys) { Assert-KeyAbsent $key }
    Assert-OpenWith
    Assert-Defaults 'context-menu task deselection'
    $results.Add('Deselecting the context-menu task removed both owned menu keys.')

    Invoke-Setup '/TASKS="openwith,contextmenu"' 'reenable-menu'
    Assert-Menus
    Assert-OpenWith
    Assert-Defaults 'context-menu re-enabling'
    $results.Add('Re-enabling the context-menu task restored the registered verbs.')

    Invoke-Uninstall 'uninstall'
    $finishedUninstall = $true
    foreach ($key in ($menuKeys + @($applicationKey, $documentKey, $uninstallKey))) { Assert-KeyAbsent $key }
    foreach ($extension in $extensions) {
        if ((Read-RegistryValue 'User' "Software\Classes\$extension\OpenWithProgids" 'ExEditor.Document').Exists) {
            throw "Uninstall left an ExEditor Open With value for $extension."
        }
    }
    Assert-Defaults 'uninstallation'
    $results.Add('Uninstall removed ExEditor-owned registrations and preserved existing defaults and other Open With values.')
    Write-Output 'Installer shell-integration checks passed without launching ExEditor.'
} finally {
    if (-not $finishedUninstall -and (Test-Path -LiteralPath (Join-Path $installDirectory 'unins000.exe') -PathType Leaf)) {
        try { Invoke-Uninstall 'cleanup'; $finishedUninstall = $true }
        catch { Write-Warning "Installer cleanup failed: $_" }
    }
    foreach ($extension in $originalDefaults.Keys) {
        $key = $registry.User.CreateSubKey("Software\Classes\$extension")
        try {
            $original = $originalDefaults[$extension]
            if ($original.Exists) { $key.SetValue('', $original.Data, [Microsoft.Win32.RegistryValueKind]$original.Kind) }
            else { $key.DeleteValue('', $false) }
        } finally { $key.Dispose() }
    }
    if ($fixtureCreated) {
        $key = $registry.User.OpenSubKey($fixtureKey, $true)
        if ($null -ne $key) { try { $key.DeleteValue($fixtureName, $false) } finally { $key.Dispose() } }
    }
    foreach ($base in $registry.Values) { $base.Dispose() }
    [ordered]@{ checks = @($results); completed_uninstall = $finishedUninstall } |
        ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $logRoot 'shell-integration-results.json') -Encoding utf8
    if (Test-Path -LiteralPath $testRoot) {
        $resolvedCleanup = (Resolve-Path -LiteralPath $testRoot).ProviderPath
        $cleanupItem = Get-Item -LiteralPath $resolvedCleanup
        if (-not $resolvedCleanup.StartsWith($runnerTemp + '\', [StringComparison]::OrdinalIgnoreCase) -or
            $resolvedCleanup -ne $testRoot -or ($cleanupItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing cleanup outside the unique RUNNER_TEMP directory.'
        }
        Remove-Item -LiteralPath $resolvedCleanup -Recurse -Force
    }
}
