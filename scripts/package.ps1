[CmdletBinding()]
param(
    [string]$ReleaseDirectory = 'target/release',
    [string]$Destination = 'artifacts/ExEditor',
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{40}$')]
    [string]$Commit
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$targetRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'target'))
$artifactRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'artifacts'))
$releaseRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot $ReleaseDirectory))
$packageRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot $Destination))
$separator = [IO.Path]::DirectorySeparatorChar
if (-not $releaseRoot.StartsWith($targetRoot + $separator, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'ReleaseDirectory must be inside this project\target.'
}
if (-not $packageRoot.StartsWith($artifactRoot + $separator, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Destination must be inside this project\artifacts.'
}
if (Test-Path -LiteralPath $packageRoot) {
    throw 'The package destination already exists; choose a new directory.'
}

$applicationPath = Join-Path $releaseRoot 'exeditor.exe'
if (-not (Test-Path -LiteralPath $applicationPath -PathType Leaf)) {
    throw 'The compiled exeditor.exe is missing.'
}

$proofingRoot = Join-Path $artifactRoot 'proofing'
foreach ($required in @('libvoikko-1.dll', 'provenance.json', 'licenses', 'source')) {
    if (-not (Test-Path -LiteralPath (Join-Path $proofingRoot $required))) {
        throw "The built proofing runtime is missing $required."
    }
}
$proofingItem = Get-Item -LiteralPath $proofingRoot
$proofingLinks = @(Get-ChildItem -LiteralPath $proofingRoot -Recurse -Force -Attributes ReparsePoint)
if (($proofingItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $proofingLinks.Count -gt 0) {
    throw 'The proofing runtime must contain files and directories, not links to outside paths.'
}

New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
Copy-Item -LiteralPath $applicationPath -Destination (Join-Path $packageRoot 'ExEditor.exe')
Get-ChildItem -LiteralPath $releaseRoot -Filter '*.dll' -File | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $packageRoot
}
$bundledProofing = Join-Path $packageRoot 'proofing'
New-Item -ItemType Directory -Path $bundledProofing -Force | Out-Null
Get-ChildItem -LiteralPath $proofingRoot -Force | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $bundledProofing -Recurse
}
foreach ($documentName in @('README.md', 'LICENSE', 'CHANGELOG.md', 'Cargo.lock')) {
    $documentPath = Join-Path $projectRoot $documentName
    if (Test-Path -LiteralPath $documentPath -PathType Leaf) {
        Copy-Item -LiteralPath $documentPath -Destination $packageRoot
    }
}


Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/KEYBOARD_SHORTCUTS.txt') -Destination $packageRoot

foreach ($family in @('pretendard-std', 'maple-mono-nf', 'inter', 'noto-color-emoji')) {
    $licenseSource = Join-Path $projectRoot "assets\fonts\$family\OFL.txt"
    if (-not (Test-Path -LiteralPath $licenseSource -PathType Leaf)) {
        throw "The $family font license is missing."
    }
    $licenseDirectory = Join-Path $packageRoot "licenses\fonts\$family"
    New-Item -ItemType Directory -Path $licenseDirectory -Force | Out-Null
    Copy-Item -LiteralPath $licenseSource -Destination (Join-Path $licenseDirectory 'OFL.txt')
}
Copy-Item -LiteralPath (Join-Path $projectRoot 'assets/licenses/wordcraft') -Destination (Join-Path $packageRoot 'licenses/wordcraft') -Recurse
$toolchainText = Get-Content -LiteralPath (Join-Path $projectRoot 'rust-toolchain.toml') -Raw
$rustVersion = [regex]::Match($toolchainText, 'channel\s*=\s*"([^"]+)"').Groups[1].Value
$buildInfo = [ordered]@{
    application = 'ExEditor'
    repository = 'https://github.com/abelokoj/ExEditor'
    source_commit = $Commit.ToLowerInvariant()
    built_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    target = 'x86_64-pc-windows-msvc'
    rust_toolchain = $rustVersion
    workflow_run = $env:GITHUB_RUN_ID
}
$buildInfo | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $packageRoot 'build-info.json') -Encoding utf8

$packagedFiles = @(Get-ChildItem -LiteralPath $packageRoot -File -Recurse | Sort-Object FullName)
$packageBytes = ($packagedFiles | Measure-Object -Property Length -Sum).Sum
if ($packageBytes -gt 2GB) {
    throw 'The uncompressed package exceeds the 2 GiB packaging limit.'
}
$hashLines = foreach ($packagedFile in $packagedFiles) {
    $relativePath = [IO.Path]::GetRelativePath($packageRoot, $packagedFile.FullName).Replace('\', '/')
    $hash = (Get-FileHash -LiteralPath $packagedFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $relativePath"
}
$hashLines | Set-Content -LiteralPath (Join-Path $packageRoot 'SHA256SUMS.txt') -Encoding utf8
Write-Output "Packaged ExEditor from $Commit ($([Math]::Round($packageBytes / 1MB)) MiB)."


