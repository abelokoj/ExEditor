[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ExecutablePath,
    [string]$LogDirectory = 'logs',
    [ValidateSet('Light', 'Dark')][string]$Appearance = 'Light',
    [ValidateRange(1, 120)][int]$Sections = 120
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
@{ spellcheck = $false; autocomplete = $false; appearance = $Appearance } | ConvertTo-Json |
    Set-Content -LiteralPath (Join-Path $data 'preferences.json') -Encoding utf8NoBOM
$markdown = Join-Path $testRoot 'Long Markdown café.md'
$paragraphs = foreach ($section in 1..$Sections) {
    "## Section $section`n`n" + ('A native document with **bold**, *italic*, Unicode café 😀 and [a safe link](https://example.org). ' * 15) + "`n`n"
}
[IO.File]::WriteAllText($markdown, "# Native Markdown check`n`n" + ($paragraphs -join ''), [Text.UTF8Encoding]::new($false))
$previousData = $env:EXEDITOR_DATA_DIR
$primary = $null
$forwarder = $null
$results = [ordered]@{ source_commit = $env:GITHUB_SHA; fixture_bytes = (Get-Item -LiteralPath $markdown).Length; appearance = $Appearance }
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
function Save-NativeScreenshot([Diagnostics.Process]$Process, [string]$Name, [IntPtr]$Window = [IntPtr]::Zero) {
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string className, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int metric);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("gdi32.dll")] static extern IntPtr CreateRectRgn(int left, int top, int right, int bottom);
    [DllImport("user32.dll")] static extern int GetWindowRgn(IntPtr window, IntPtr region);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr value);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr window, int attribute, out uint value, int size);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr window, System.Text.StringBuilder name, int size);
    public static string WindowDetails(IntPtr window) {
        var name = new System.Text.StringBuilder(256); GetClassName(window, name, 256);
        var region = CreateRectRgn(0,0,0,0); uint corner; int hr = DwmGetWindowAttribute(window,33,out corner,4);
        try { return window.ToInt64() + ";" + name + ";region=" + GetWindowRgn(window,region) + ";corner=" + corner + ";dwm=" + hr; }
        finally { DeleteObject(region); }
    }
    public static bool Rounded(IntPtr window) {
        var region = CreateRectRgn(0,0,0,0); uint corner; int hr = DwmGetWindowAttribute(window,33,out corner,4);
        try { return GetWindowRgn(window, region) == 3 || (hr >= 0 && corner == 2); }
        finally { DeleteObject(region); }
    }
    [DllImport("user32.dll")] static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public static void ZoomKey(byte key) {
        keybd_event(0x11, 0, 0, UIntPtr.Zero);
        try {
            System.Threading.Thread.Sleep(150);
            keybd_event(key, 0, 0, UIntPtr.Zero);
            System.Threading.Thread.Sleep(50);
            keybd_event(key, 0, 2, UIntPtr.Zero);
        } finally { keybd_event(0x11, 0, 2, UIntPtr.Zero); }
        System.Threading.Thread.Sleep(250);
    }
    public static void WheelZoom(IntPtr window) {
        Rect rect; if (!GetWindowRect(window, out rect)) throw new InvalidOperationException("Missing test window");
        int x = rect.Left + 100, y = rect.Top + 200;
        keybd_event(0x11, 0, 0, UIntPtr.Zero);
        try {
            SetCursorPos(x, y);
            System.Threading.Thread.Sleep(250);
            mouse_event(0x0800, 0, 0, 120, UIntPtr.Zero);
            System.Threading.Thread.Sleep(500);
        } finally { keybd_event(0x11, 0, 2, UIntPtr.Zero); }
    }
}
"@
        }
        if ($Window -eq [IntPtr]::Zero) { $Window = $Process.MainWindowHandle }
        $owner = 0u
        [ExEditorStartupCapture]::GetWindowThreadProcessId($Window, [ref]$owner) | Out-Null
        if ($owner -ne $Process.Id) { throw 'The capture window is not owned by the test process.' }
        $rect = [ExEditorStartupCapture+Rect]::new()
        if (-not [ExEditorStartupCapture]::GetWindowRect($Window, [ref]$rect)) {
            throw 'The native window bounds are unavailable.'
        }
        $width = [Math]::Min($rect.Right - $rect.Left, [ExEditorStartupCapture]::GetSystemMetrics(0) - 32)
        $height = [Math]::Min($rect.Bottom - $rect.Top, [ExEditorStartupCapture]::GetSystemMetrics(1) - 64)
        # Keep the whole native window visible on the runner's small desktop.
        [ExEditorStartupCapture]::SetWindowPos($Window, [IntPtr]::Zero, 16, 16, $width, $height, 0x14) | Out-Null
        Start-Sleep -Milliseconds 400
        [ExEditorStartupCapture]::GetWindowRect($Window, [ref]$rect) | Out-Null
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
    $env:EXEDITOR_NATIVE_DIAGNOSTICS = '1' 
    # Only the GitHub runner displays this test window; no input is sent to a user's desktop.
    $primary = Start-Process -FilePath $executable -WindowStyle Normal -PassThru `
        -RedirectStandardOutput (Join-Path $logs 'native-startup.stdout.log') `
        -RedirectStandardError (Join-Path $logs 'native-startup.stderr.log')
    Wait-Document $primary 'Untitled'
    $initialWindow = $primary.MainWindowHandle
    $results.initial = Measure-NativeMemory $primary
    Save-NativeScreenshot $primary 'native-startup-initial.png'
    $results.rounded_window = [ExEditorStartupCapture]::Rounded($initialWindow)
    $results.window_details = [ExEditorStartupCapture]::WindowDetails($initialWindow)
    if (-not $results.rounded_window) { throw 'The native document window has no rounded corners.' }
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
    Start-Sleep -Milliseconds 1500
    Save-NativeScreenshot $primary 'native-selection.png'
    [ExEditorStartupCapture]::ZoomKey(0xbb)
    Start-Sleep -Milliseconds 400
    Save-NativeScreenshot $primary 'native-main-key-zoom.png'
    if ((Get-FileHash (Join-Path $logs 'native-selection.png')).Hash -eq (Get-FileHash (Join-Path $logs 'native-main-key-zoom.png')).Hash) {
        throw 'Ctrl+plus did not visibly zoom the selected document.'
    }
    [ExEditorStartupCapture]::ZoomKey(0x6b)
    [ExEditorStartupCapture]::ZoomKey(0x6b)
    Start-Sleep -Milliseconds 1500
    Save-NativeScreenshot $primary 'native-zoom-in.png'
    if ((Get-FileHash (Join-Path $logs 'native-main-key-zoom.png')).Hash -eq (Get-FileHash (Join-Path $logs 'native-zoom-in.png')).Hash) {
        throw 'Ctrl+keypad plus did not visibly zoom the selected document.'
    }
    [ExEditorStartupCapture]::ZoomKey(0xbd)
    [ExEditorStartupCapture]::ZoomKey(0x6d)
    [ExEditorStartupCapture]::ZoomKey(0x6d)
    Start-Sleep -Milliseconds 1500
    Save-NativeScreenshot $primary 'native-zoom-reset.png'
    [ExEditorStartupCapture]::WheelZoom($initialWindow)
    Start-Sleep -Milliseconds 1500
    Save-NativeScreenshot $primary 'native-wheel-zoom.png'
    if ((Get-FileHash (Join-Path $logs 'native-zoom-reset.png')).Hash -eq (Get-FileHash (Join-Path $logs 'native-wheel-zoom.png')).Hash) {
        throw 'Ctrl+wheel did not visibly zoom the selected document.'
    }
    $shell.SendKeys('^,')
    Start-Sleep -Milliseconds 1000
    $settingsWindow = [ExEditorStartupCapture]::FindWindow('NshWindow', 'ExEditor Settings')
    if ($settingsWindow -ne [IntPtr]::Zero) {
        Save-NativeScreenshot $primary 'native-settings.png' $settingsWindow
        $results.settings_opened = $true
    } else { throw 'Ctrl+comma did not open the native Settings window.' }
    if (Select-String -LiteralPath (Join-Path $logs 'native-startup.stderr.log') -Pattern '^thread .*panicked' -Quiet) {
        throw 'The native application reported a runtime panic during keyboard checks.'
    }
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
