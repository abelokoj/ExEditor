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
@{ spellcheck = $false; autocomplete = $false; appearance = $Appearance; window_controls = 'Right'; interface_scale = 100; show_line_numbers = $false; line_number_width = 48.0 } | ConvertTo-Json |
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
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref Point point);
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
function Get-NativeNamedElements([Diagnostics.Process]$Process, [string]$Name) {
    $root = [Windows.Automation.AutomationElement]::FromHandle($Process.MainWindowHandle)
    $condition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $Name)
    foreach ($element in $root.FindAll([Windows.Automation.TreeScope]::Descendants, $condition)) {
        if (-not $element.Current.IsOffscreen) { $element }
    }
}
function Wait-NativeNamedElements([Diagnostics.Process]$Process, [string]$Name, [int]$Count) {
    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    do {
        $Process.Refresh()
        if ($Process.HasExited) { throw "The native application exited while waiting for $Name." }
        $elements = @(Get-NativeNamedElements $Process $Name)
        if ($elements.Count -eq $Count) { return $elements }
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Expected $Count visible native elements named '$Name'; found $($elements.Count)."
}
function Get-NativeBounds($Element) {
    $bounds = $Element.Current.BoundingRectangle
    if ($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0) { throw 'A native control has no visible bounds.' }
    [pscustomobject]@{ left = $bounds.Left; top = $bounds.Top; width = $bounds.Width; height = $bounds.Height }
}
function Save-RequiredNativeScreenshot([Diagnostics.Process]$Process, [string]$Name) {
    Save-NativeScreenshot $Process $Name
    if (-not (Test-Path -LiteralPath (Join-Path $logs $Name))) { throw "Native regression screenshot was not saved: $Name" }
}
function Send-NativeChord([byte]$Key, [bool]$Control) {
    [ExEditorStartupCapture]::ZoomKey(0x4b)
    if ($Control) { [ExEditorStartupCapture]::ZoomKey($Key) }
    else { $shell.SendKeys(([string][char]$Key).ToLowerInvariant()) }
    Start-Sleep -Milliseconds 650
}
function Assert-NativeClose([double]$Actual, [double]$Expected, [double]$Tolerance, [string]$Description) {
    if ([Math]::Abs($Actual - $Expected) -gt $Tolerance) { throw "$Description was $Actual; expected $Expected within $Tolerance." }
}
function Get-NativePaneHeaders([Diagnostics.Process]$Process, [int]$Count) {
    $headers = for ($index = 1; $index -le $Count; $index++) {
        $element = @(Wait-NativeNamedElements $Process "Choose document for pane $index" 1)
        Get-NativeBounds $element[0]
    }
    for ($index = $Count + 1; $index -le 4; $index++) {
        Wait-NativeNamedElements $Process "Choose document for pane $index" 0 | Out-Null
    }
    $headers
}
function Assert-NativeCaption([Diagnostics.Process]$Process, [double]$Scale) {
    $client = [ExEditorStartupCapture+Point]::new()
    if (-not [ExEditorStartupCapture]::ClientToScreen($Process.MainWindowHandle, [ref]$client)) { throw 'The native client origin is unavailable.' }
    $window = [ExEditorStartupCapture+Rect]::new()
    [ExEditorStartupCapture]::GetWindowRect($Process.MainWindowHandle, [ref]$window) | Out-Null
    $controls = foreach ($name in @('Minimize', 'Zoom', 'Close')) {
        $element = @(Wait-NativeNamedElements $Process $name 1)
        Get-NativeBounds $element[0]
    }
    $file = @(Wait-NativeNamedElements $Process 'File' 1)
    $menu = Get-NativeBounds $file[0]
    # The22px menu control is centered in its24px row immediately below the31px caption.
    $captionHeight = ($menu.top - $client.Y) / $Scale - 1.0
    Assert-NativeClose $captionHeight 31.0 1.5 'The logical title-row height'
    foreach ($control in $controls) {
        Assert-NativeClose ($control.width / $Scale) 15.6 1.5 'A traffic-light width'
        Assert-NativeClose ($control.height / $Scale) 15.6 1.5 'A traffic-light height'
        Assert-NativeClose (($control.top + $control.height / 2 - $client.Y) / $Scale) 15.5 1.5 'A traffic-light vertical center'
    }
    if (-not ($controls[0].left -lt $controls[1].left -and $controls[1].left -lt $controls[2].left)) {
        throw 'Right-side native controls are not ordered minimize, maximize, close.'
    }
    $rightGap = ($window.Right - ($controls[2].left + $controls[2].width)) / $Scale
    if ($controls[0].left -lt ($window.Left + $window.Right) / 2 -or $rightGap -lt 0 -or $rightGap -gt 32) {
        throw 'The red Close control is not at the far-right end of the title row.'
    }
    $image = [Drawing.Bitmap]::new((Join-Path $logs 'native-startup-initial.png'))
    try {
        $colors = @(@(255, 189, 46), @(40, 200, 64), @(255, 95, 87))
        for ($index = 0; $index -lt 3; $index++) {
            $control = $controls[$index]
            $pixel = $image.GetPixel([int]($control.left + $control.width / 2 - $window.Left), [int]($control.top + $control.height / 2 - $window.Top))
            Assert-NativeClose $pixel.R $colors[$index][0] 16 'A traffic-light red channel'
            Assert-NativeClose $pixel.G $colors[$index][1] 16 'A traffic-light green channel'
            Assert-NativeClose $pixel.B $colors[$index][2] 16 'A traffic-light blue channel'
        }
    } finally { $image.Dispose() }
    [ordered]@{ order = @('yellow minimize', 'green maximize', 'red close'); title_height_logical = $captionHeight; circles = @($controls); right_gap_logical = $rightGap }
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
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $nativeScale = [ExEditorStartupCapture]::GetDpiForWindow($initialWindow) / 96.0
    if ($nativeScale -le 0) { throw 'The native window DPI is unavailable.' }
    $results.right_caption = Assert-NativeCaption $primary $nativeScale
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
    if ($Appearance -eq 'Dark') {
        $image = [Drawing.Bitmap]::new((Join-Path $logs 'native-startup-markdown.png'))
        try {
            # The generated fixture's first heading lies below its title/menu area.
            $visibleHeadingPixels = 0
            foreach ($y in 85..115) { foreach ($x in 20..340) {
                $pixel = $image.GetPixel($x, $y)
                if ($pixel.R -gt 150 -and $pixel.G -gt 150 -and $pixel.B -gt 150) { $visibleHeadingPixels++ }
            } }
            if ($visibleHeadingPixels -lt 40) { throw 'The generated heading is not readable on the dark document.' }
            $results.dark_heading_visible = $true
        } finally { $image.Dispose() }
    }
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
    # Display changes must keep native editing mounted and preserve the original window.
    $editorBounds = Get-NativeBounds (@(Wait-NativeNamedElements $primary 'Document editor' 1)[0])
    Send-NativeChord 0x4c $false
    $gutterBounds = Get-NativeBounds (@(Wait-NativeNamedElements $primary 'Document editor' 1)[0])
    Assert-NativeClose (($gutterBounds.left - $editorBounds.left) / $nativeScale) 48.0 2.0 'The line-number gutter width'
    Save-RequiredNativeScreenshot $primary 'native-line-numbers.png'
    Send-NativeChord 0x4c $false
    $hiddenBounds = Get-NativeBounds (@(Wait-NativeNamedElements $primary 'Document editor' 1)[0])
    Assert-NativeClose $hiddenBounds.left $editorBounds.left (2.0 * $nativeScale) 'The hidden gutter editor origin'
    $results.line_number_toggle = $true

    [ExEditorStartupCapture]::ZoomKey(0xdc)
    $columns = @(Get-NativePaneHeaders $primary 2)
    if (-not ($columns[0].left -lt $columns[1].left)) { throw 'Two-column split did not arrange panes horizontally.' }
    Assert-NativeClose $columns[0].top $columns[1].top (2.0 * $nativeScale) 'Two-column header alignment'
    Wait-NativeNamedElements $primary 'Document editor' 2 | Out-Null
    Save-RequiredNativeScreenshot $primary 'native-split-columns.png'
    Send-NativeChord 0x47 $true
    $grid = @(Get-NativePaneHeaders $primary 4)
    if (-not ($grid[0].left -lt $grid[1].left -and $grid[0].top -lt $grid[2].top -and $grid[2].left -lt $grid[3].left)) {
        throw 'The four-pane layout is not arranged as a two-by-two grid.'
    }
    Assert-NativeClose $grid[0].top $grid[1].top (2.0 * $nativeScale) 'Top grid-row alignment'
    Assert-NativeClose $grid[2].top $grid[3].top (2.0 * $nativeScale) 'Bottom grid-row alignment'
    Wait-NativeNamedElements $primary 'Document editor' 4 | Out-Null
    Save-RequiredNativeScreenshot $primary 'native-split-grid.png'
    Send-NativeChord 0x31 $true
    Get-NativePaneHeaders $primary 0 | Out-Null
    Wait-NativeNamedElements $primary 'Document editor' 1 | Out-Null
    Save-RequiredNativeScreenshot $primary 'native-single-pane.png'
    $primary.Refresh()
    if ($primary.MainWindowHandle -ne $initialWindow) { throw 'Changing split views replaced the existing window.' }
    $results.split_views = @('same document: two columns', 'open tabs: four-pane grid', 'return to single pane')
    $results.split_visual_evidence = @('native-line-numbers.png', 'native-split-columns.png', 'native-split-grid.png', 'native-single-pane.png')
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
