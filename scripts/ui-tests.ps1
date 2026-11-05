[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Application,
    [Parameter(Mandatory)][string]$WinApp,
    [string]$EvidenceDirectory = 'dev_notes/ui-artifacts'
)
$ErrorActionPreference = 'Stop'
$env:WINAPP_CLI_TELEMETRY_OPTOUT = '1'
$previousWorkflow = $env:WINAPP_UI_WORKFLOW_ID
$env:WINAPP_UI_WORKFLOW_ID = [guid]::NewGuid().ToString('N')
$results = [Collections.Generic.List[object]]::new()
$evidence = [IO.Path]::GetFullPath($EvidenceDirectory)
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$applicationPath = (Resolve-Path -LiteralPath $Application).Path
$process = Start-Process -FilePath $applicationPath -PassThru
$appPid = $process.Id
function Invoke-Ui([string[]]$Arguments) {
    $output = & $WinApp ui @Arguments -a "$appPid"
    if ($LASTEXITCODE -ne 0) { throw "UI command failed: $($Arguments -join ' ')" }
    $output
}
function Check([string]$Name,[scriptblock]$Body) {
    try { & $Body | Out-Null; $results.Add(@{name=$Name;status='PASS'}) }
    catch { $results.Add(@{name=$Name;status='FAIL';detail="$_"}) }
}
function Capture([string]$Name) {
    Invoke-Ui @('screenshot','--output',(Join-Path $evidence "$Name.png"),'--focus','--json') | Out-Null
    if (-not (Test-Path -LiteralPath (Join-Path $evidence "$Name.png"))) { throw 'Screenshot not saved.' }
}
try {
    Check 'Document window launches' {
        $deadline = [DateTime]::UtcNow.AddSeconds(20)
        do {
            Start-Sleep -Milliseconds 250
            $windows = Invoke-Ui @('list-windows','--json') | ConvertFrom-Json
            if ($process.HasExited) { throw 'The application exited before its document window appeared.' }
        } until ($windows -or [DateTime]::UtcNow -gt $deadline)
        if (-not $windows) { throw 'No application window appeared.' }
        Capture '01-empty-rich'
    }
    Check 'Document supports typing and Unicode' {
        Invoke-Ui @('inspect','--json') | Set-Content -LiteralPath (Join-Path $evidence 'initial-tree.json')
        Invoke-Ui @('focus','Document')
        Invoke-Ui @('send-keys','ExEditor visual test — café 😀','--verbatim','--via','send-input')
        $value = Invoke-Ui @('get-value','Document','--json')
        if (($value -join "`n") -notmatch 'ExEditor visual test') { throw 'Typed document content was not exposed.' }
        Capture '02-typed-rich'
    }
    Check 'Bold formatting and undo' {
        Invoke-Ui @('send-keys','ctrl+a ctrl+b','--via','send-input')
        Capture '03-bold'
        Invoke-Ui @('send-keys','ctrl+z','--via','send-input')
    }
    Check 'Settings and embedded font choices' {
        Invoke-Ui @('send-keys','ctrl+,','--via','send-input')
        Start-Sleep -Milliseconds 500
        Capture '04-settings'
        Invoke-Ui @('send-keys','escape','--via','send-input')
    }
    Check 'Independent document tabs' {
        Invoke-Ui @('send-keys','ctrl+shift+n','--via','send-input')
        Start-Sleep -Milliseconds 300
        Capture '05-new-tab'
        Invoke-Ui @('send-keys','ctrl+shift+tab','--via','send-input')
        Capture '06-original-tab'
    }
    Check 'Final UI tree available' {
        Invoke-Ui @('inspect','--interactive','--json') | Set-Content -LiteralPath (Join-Path $evidence 'final-tree.json')
    }
} finally {
    try { & $WinApp ui yield | Out-Null } finally { $env:WINAPP_UI_WORKFLOW_ID = $previousWorkflow }
    [ordered]@{app=$applicationPath;pid=$appPid;captured_at=[DateTimeOffset]::Now.ToString('o');results=@($results.ToArray())} |
        ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $evidence 'results.json') -Encoding utf8
}
$failed = @($results | Where-Object status -eq 'FAIL').Count
$results | Format-Table
if ($failed) { exit 1 }
