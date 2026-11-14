[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ExecutablePath,
    [string]$LogDirectory = 'logs'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $IsWindows -or -not $env:RUNNER_TEMP) {
    throw 'Native startup checks run only on a Windows GitHub Actions runner.'
}
$executable = (Resolve-Path -LiteralPath $ExecutablePath).ProviderPath
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$logs = [IO.Path]::GetFullPath((Join-Path $projectRoot $LogDirectory))
if (-not $logs.StartsWith($projectRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Startup evidence must stay inside the project.'
}
$runnerTemp = (Resolve-Path -LiteralPath $env:RUNNER_TEMP).ProviderPath.TrimEnd('\')
$testRoot = [IO.Path]::GetFullPath((Join-Path $runnerTemp ('ExEditor native check ' + [guid]::NewGuid().ToString('N'))))
if (-not $testRoot.StartsWith($runnerTemp + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Native startup data must stay inside RUNNER_TEMP.'
}
New-Item -ItemType Directory -Path $testRoot, $logs -Force | Out-Null
$data = Join-Path $testRoot 'Isolated app data'
New-Item -ItemType Directory -Path $data | Out-Null
@{ spellcheck = $false; autocomplete = $false; appearance = 'Light' } | ConvertTo-Json |
    Set-Content -LiteralPath (Join-Path $data 'preferences.json') -Encoding utf8NoBOM
$markdown = Join-Path $testRoot 'Long Markdown café.md'
$paragraphs = foreach ($section in 1..120) {
    "## Section $section`n`n" + ('A native document with **bold**, *italic*, Unicode café 😀 and [a safe link](https://example.org). ' * 15) + "`n`n"
}
[IO.File]::WriteAllText($markdown, "# Native Markdown check`n`n" + ($paragraphs -join ''), [Text.UTF8Encoding]::new($false))
$previousData = $env:EXEDITOR_DATA_DIR
$primary = $null
$forwarder = $null
$results = [ordered]@{ source_commit = $env:GITHUB_SHA; fixture_bytes = (Get-Item -LiteralPath $markdown).Length }
function Wait-Document([Diagnostics.Process]$Process, [string]$Title) {
    $deadline = [DateTime]::UtcNow.AddSeconds(40)
    do {
        $Process.Refresh()
        if ($Process.HasExited) { throw "The native application exited during startup (exit $($Process.ExitCode))." }
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero -and $Process.MainWindowTitle.Contains($Title)) { return }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "The native document window did not become ready: $Title"
}
function Measure-NativeMemory([Diagnostics.Process]$Process) {
    $samples = foreach ($sample in 1..6) {
        Start-Sleep -Milliseconds 500
        $Process.Refresh()
        if ($Process.HasExited) { throw 'The native application exited during the memory sample.' }
        [pscustomobject]@{ working_set_bytes = $Process.WorkingSet64; private_bytes = $Process.PrivateMemorySize64 }
    }
    [ordered]@{
        working_set_bytes = ($samples.working_set_bytes | Measure-Object -Maximum).Maximum
        private_bytes = ($samples.private_bytes | Measure-Object -Maximum).Maximum
    }
}
function Save-NativeScreenshot([Diagnostics.Process]$Process, [string]$Name) {
    try {
        Add-Type -AssemblyName System.Drawing
        if (-not ('ExEditorStartupCapture' -as [type])) {
            Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class ExEditorStartupCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);
}
"@
        }
        $rect = [ExEditorStartupCapture+Rect]::new()
        if (-not [ExEditorStartupCapture]::GetWindowRect($Process.MainWindowHandle, [ref]$rect)) {
            throw 'The native window bounds are unavailable.'
        }
        $width = $rect.Right - $rect.Left
        $height = $rect.Bottom - $rect.Top
        if ($width -le 0 -or $height -le 0 -or $width -gt 4096 -or $height -gt 4096) {
            throw 'The native window has invalid capture bounds.'
        }
        $bitmap = [Drawing.Bitmap]::new($width, $height)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, [Drawing.Size]::new($width, $height))
            $bitmap.Save((Join-Path $logs $Name), [Drawing.Imaging.ImageFormat]::Png)
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
    } catch {
        $results['capture_warning_' + $Name] = $_.Exception.Message
        Write-Warning "Native screenshot capture unavailable: $_"
    }
}
try {
    $env:EXEDITOR_DATA_DIR = $data
    # Only the GitHub runner displays this test window; no input is sent to a user's desktop.
    $primary = Start-Process -FilePath $executable -WindowStyle Normal -PassThru `
        -RedirectStandardOutput (Join-Path $logs 'native-startup.stdout.log') `
        -RedirectStandardError (Join-Path $logs 'native-startup.stderr.log')
    Wait-Document $primary 'Untitled'
    $initialWindow = $primary.MainWindowHandle
    $results.initial = Measure-NativeMemory $primary
    Save-NativeScreenshot $primary 'native-startup-initial.png'
    $forwarder = Start-Process -FilePath $executable -ArgumentList ('"' + $markdown + '"') -WindowStyle Hidden -PassThru
    if (-not $forwarder.WaitForExit(15000)) { throw 'The second launch did not hand the file to the existing application.' }
    if ($forwarder.ExitCode -ne 0) { throw "File forwarding failed (exit $($forwarder.ExitCode))." }
    Wait-Document $primary ([IO.Path]::GetFileName($markdown))
    if ($primary.MainWindowHandle -ne $initialWindow) { throw 'Opening a file replaced the existing document window.' }
    $results.markdown = Measure-NativeMemory $primary
    Save-NativeScreenshot $primary 'native-startup-markdown.png'
    $shell = New-Object -ComObject WScript.Shell
    if (-not $shell.AppActivate($primary.Id)) { throw 'The GitHub test window could not be activated.' }
    # These inputs are sent only to the isolated app on the GitHub runner.
    $shell.SendKeys('^a')
    Start-Sleep -Milliseconds 700
    Save-NativeScreenshot $primary 'native-selection.png'
    $shell.SendKeys('^{ADD}')
    $shell.SendKeys('^{ADD}')
    $shell.SendKeys('^{ADD}')
    Start-Sleep -Milliseconds 700
    Save-NativeScreenshot $primary 'native-zoom-in.png'
    $shell.SendKeys('^{SUBTRACT}')
    $shell.SendKeys('^{SUBTRACT}')
    $shell.SendKeys('^{SUBTRACT}')
    Start-Sleep -Milliseconds 700
    Save-NativeScreenshot $primary 'native-zoom-reset.png'
    $shell.SendKeys('^{,}')
    Start-Sleep -Milliseconds 1000
    $results.keyboard_visual_evidence = @('native-selection.png', 'native-zoom-in.png', 'native-zoom-reset.png')
    $results.working_set_delta_bytes = $results.markdown.working_set_bytes - $results.initial.working_set_bytes
    $results.same_window_handoff = $true
    $results.started = $true
    $results.memory_is_observation_only = $true
    Write-Output 'Native Windows startup and same-window Markdown handoff passed.'
    Write-Output ($results | ConvertTo-Json -Depth 5)
} finally {
    $env:EXEDITOR_DATA_DIR = $previousData
    foreach ($owned in @($forwarder, $primary)) {
        if ($null -ne $owned) {
            try { if (-not $owned.HasExited) { $owned.Kill($true); $owned.WaitForExit(5000) | Out-Null } }
            finally { $owned.Dispose() }
        }
    }
    $results | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $logs 'native-startup-results.json') -Encoding utf8
}
