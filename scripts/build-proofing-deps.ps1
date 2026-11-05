[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Msys2Root,
    [string]$Destination = 'artifacts/proofing'
)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') {
    throw 'Native proofing dependencies are built by GitHub Actions only. This script does not build on the testing computer.'
}
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$destinationPath = [IO.Path]::GetFullPath((Join-Path $projectRoot $Destination))
if (-not $destinationPath.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The proofing output directory must remain inside the ExEditor checkout.'
}
$bashPath = Join-Path $Msys2Root 'usr/bin/bash.exe'
if (-not (Test-Path -LiteralPath $bashPath -PathType Leaf)) { throw 'MSYS2 bash was not found at the configured path.' }
$workPath = Join-Path $env:RUNNER_TEMP ('exeditor-proofing-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workPath, $destinationPath -Force | Out-Null
$sourceOutput = Join-Path $destinationPath 'source'
$licenseOutput = Join-Path $destinationPath 'licenses'
New-Item -ItemType Directory -Path $sourceOutput, $licenseOutput -Force | Out-Null

function Get-PinnedSource([string]$Url, [string]$ExpectedSha256, [string]$FileName) {
    $target = Join-Path $sourceOutput $FileName
    Invoke-WebRequest -Uri $Url -OutFile $target
    $actual = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $ExpectedSha256) { throw ('Source checksum mismatch: ' + $FileName) }
    return $target
}

$voikkoCommit = '4b838726d6e11af235b5aba50dcc056bc17c8176'
$divvunCommit = '7c3400754ef3f3e39b1c725f0ea18c1acd749b0d'
$voikkoSource = Get-PinnedSource "https://codeload.github.com/voikko/corevoikko/tar.gz/$voikkoCommit" '72d449a512f447776e9cba539915978c6d148d5b0fb94d84748345b2c4448c9d' "corevoikko-$voikkoCommit.tar.gz"
$null = Get-PinnedSource "https://codeload.github.com/divvun/divvunspell/tar.gz/$divvunCommit" '2240e306c6260e099817dc2cf0a4113ded074f3f16dccae3d9d5edc951d5170f' "divvunspell-$divvunCommit.tar.gz"
$null = Get-PinnedSource 'https://static.crates.io/crates/harper-core/harper-core-3.0.0.crate' 'f3e3c3af455f1325a03746c19efa0bbd60a2cf65da3fa93505e4ac675184ebda' 'harper-core-3.0.0.crate'
$null = Get-PinnedSource 'https://static.crates.io/crates/spellbook/spellbook-0.4.2.crate' '0d204abcbdf8e88729306a8d0ca01a79d8a49969fe1f84696909d6dbc4321c1a' 'spellbook-0.4.2.crate'
$null = Get-PinnedSource 'https://static.crates.io/crates/harper-brill/harper-brill-3.0.0.crate' 'dc30f9c4ce40d83ef9beef39126d803b9014ef8cc88581dca3a693b8eaf3586c' 'harper-brill-3.0.0.crate'
$null = Get-PinnedSource 'https://static.crates.io/crates/harper-pos-utils/harper-pos-utils-3.0.0.crate' 'adf827c2f3ecd4a52077c86ff07e328cd34bca01acf09ea9054ab4ad077dc302' 'harper-pos-utils-3.0.0.crate'
$boxCommit = 'bff1974bdc3290759a86c5cebd123178a3b882f7'
$null = Get-PinnedSource "https://codeload.github.com/bbqsrc/box/tar.gz/$boxCommit" '8fbb1018494219da81a46bc182b55f2dcdb5681c013e4a8134e71bc8a7d7704a' "box-format-upstream-$boxCommit.tar.gz"
$liblzmaSource = Get-PinnedSource 'https://static.crates.io/crates/liblzma/liblzma-0.4.9.crate' 'aa980ea6744074a5b5e0ac6b164aca94fc970121869b3e78ae12a2b0097dbea7' 'liblzma-0.4.9.crate'
$patchedBoxSource = Join-Path $sourceOutput 'box-format-exeditor-patched.tar.gz'
& tar -czf $patchedBoxSource -C (Join-Path $projectRoot 'vendor') 'box-format'
if ($LASTEXITCODE -ne 0) { throw 'Could not archive the corresponding patched box-format source.' }
Copy-Item -LiteralPath (Join-Path $projectRoot 'vendor/box-format/PATCHES.md') -Destination (Join-Path $sourceOutput 'box-format-PATCHES.md')
Copy-Item -LiteralPath (Join-Path $projectRoot 'Cargo.lock') -Destination (Join-Path $sourceOutput 'ExEditor-Cargo.lock')
$liblzmaExtract = Join-Path $workPath 'liblzma-source'
New-Item -ItemType Directory -Path $liblzmaExtract -Force | Out-Null
& tar -xzf $liblzmaSource -C $liblzmaExtract
if ($LASTEXITCODE -ne 0) { throw 'Could not read original liblzma source notices.' }
foreach ($notice in Get-ChildItem -LiteralPath (Join-Path $liblzmaExtract 'liblzma-0.4.9') -Filter 'LICENSE*' -File) {
    Copy-Item -LiteralPath $notice.FullName -Destination (Join-Path $licenseOutput ('liblzma-' + $notice.Name + '.txt'))
}
Copy-Item -Path (Join-Path $projectRoot 'assets/licenses/proofing/*.txt') -Destination $licenseOutput
Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $sourceOutput 'build-proofing-deps.ps1')

$nativeBuild = @"
set -euo pipefail
work=`$(cygpath -u "`$EXEDITOR_PROOFING_WORK")
out=`$(cygpath -u "`$EXEDITOR_PROOFING_DEST")
source_archive=`$(cygpath -u "`$EXEDITOR_VOIKKO_SOURCE")
export PATH="/mingw64/bin:/usr/bin:`$PATH"
tar -xzf "`$source_archive" -C "`$work"
cd "`$work/corevoikko-4b838726d6e11af235b5aba50dcc056bc17c8176/libvoikko"
autoreconf -fi
./configure --host=x86_64-w64-mingw32 --disable-static --enable-shared \
  --disable-hfst --disable-vislcg3 --disable-lttoolbox --enable-vfst \
  --disable-external-dicts --disable-buildtools --disable-testtools \
  CXXFLAGS="-O2 -ffile-prefix-map=`$work=." LDFLAGS="-Wl,--no-insert-timestamp"
make -j2 CXXFLAGS="-O2 -Wall -pedantic -ffile-prefix-map=`$work=."
test -f src/.libs/libvoikko-1.dll
cp src/.libs/libvoikko-1.dll "`$out/libvoikko-1.dll"
declare -A copied
queue=("libvoikko-1.dll")
while ((`${#queue[@]})); do
  current=`${queue[0]}
  queue=("`${queue[@]:1}")
  while IFS= read -r dependency; do
    dependency=`${dependency%`$'\r'}
    [[ -n "`$dependency" ]] || continue
    lowercase=`${dependency,,}
    [[ -z `${copied[`$lowercase]+x} ]] || continue
    copied[`$lowercase]=1
    if [[ -f "/mingw64/bin/`$dependency" ]]; then
      cp "/mingw64/bin/`$dependency" "`$out/`$dependency"
      queue+=("`$dependency")
      owner=`$(pacman -Qoq "/mingw64/bin/`$dependency")
      mkdir -p "`$out/licenses/`$owner"
      notices=0
      while IFS= read -r notice; do
        if [[ -f "`$notice" ]]; then
          cp "`$notice" "`$out/licenses/`$owner/`${notice##*/}"
          notices=`$((notices+1))
        fi
      done < <(pacman -Ql "`$owner" | awk '`$2 ~ /\/share\/licenses\// { print `$2 }')
      if ((notices==0)); then
        echo "No original license notices found for runtime package `$owner" >&2
        exit 1
      fi
    else
      case "`$lowercase" in
        api-ms-*|ext-ms-*|kernel32.dll|ntdll.dll|user32.dll|advapi32.dll|gdi32.dll|shell32.dll|shlwapi.dll|ole32.dll|oleaut32.dll|crypt32.dll|bcrypt.dll|ws2_32.dll|msvcrt.dll|ucrtbase.dll|version.dll|winmm.dll|imm32.dll|comdlg32.dll|rpcrt4.dll|secur32.dll|setupapi.dll) ;;
        *) echo "Unpackaged native runtime dependency: `$dependency" >&2; exit 1 ;;
      esac
    fi
  done < <(objdump -p "`$out/`$current" | sed -n 's/^[[:space:]]*DLL Name: //p')
done
pacman -Q mingw-w64-x86_64-gcc mingw-w64-x86_64-gcc-libs > "`$out/source/native-toolchain.txt"
"@

$nativeScriptPath = Join-Path $workPath 'build-native.sh'
[IO.File]::WriteAllText($nativeScriptPath, $nativeBuild, [Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath $nativeScriptPath -Destination (Join-Path $sourceOutput 'build-native.sh')
$oldSystem = $env:MSYSTEM
try {
    $env:MSYSTEM = 'MINGW64'
    $env:EXEDITOR_PROOFING_WORK = $workPath
    $env:EXEDITOR_PROOFING_DEST = $destinationPath
    $env:EXEDITOR_VOIKKO_SOURCE = $voikkoSource
    $env:EXEDITOR_PROOFING_SCRIPT = $nativeScriptPath
    & $bashPath -lc 'exec bash "$(cygpath -u "$EXEDITOR_PROOFING_SCRIPT")"'
    if ($LASTEXITCODE -ne 0) { throw "The native Voikko build failed with exit code $LASTEXITCODE." }
} finally {
    $env:MSYSTEM = $oldSystem
    Remove-Item Env:EXEDITOR_PROOFING_WORK, Env:EXEDITOR_PROOFING_DEST, Env:EXEDITOR_VOIKKO_SOURCE, Env:EXEDITOR_PROOFING_SCRIPT -ErrorAction SilentlyContinue
}
if (-not (Test-Path -LiteralPath (Join-Path $destinationPath 'libvoikko-1.dll') -PathType Leaf)) { throw 'The native build did not produce libvoikko-1.dll.' }
$runtime = Get-ChildItem -LiteralPath $destinationPath -Filter '*.dll' | ForEach-Object {
    @{ file = $_.Name; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
}
$provenance = @{
    generated_at = [DateTimeOffset]::UtcNow.ToString('o')
    libvoikko_version = '4.3.3'
    libvoikko_commit = $voikkoCommit
    libvoikko_source_sha256 = '72d449a512f447776e9cba539915978c6d148d5b0fb94d84748345b2c4448c9d'
    features = @('VFST', 'shared library', 'external dictionary search disabled')
    excluded_backends = @('HFST', 'Lttoolbox', 'VISLCG3')
    core_license_selected = 'LGPL-2.1-or-later'
    harper_core = '3.0.0 (Apache-2.0)'
    spellbook = '0.4.2 (MPL-2.0)'
    divvun_fst_commit = $divvunCommit
    box_format_upstream_commit = $boxCommit
    box_format_patch = 'manifest-only xz2 alias to liblzma >= 0.4.4, static native library, default bindgen disabled'
    box_format_patched_source_sha256 = (Get-FileHash -LiteralPath $patchedBoxSource -Algorithm SHA256).Hash.ToLowerInvariant()
    liblzma = '0.4.9 (MIT OR Apache-2.0)'
    runtime = @($runtime)
}
$provenance | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $destinationPath 'provenance.json') -Encoding utf8
@(
    'ExEditor offline proofing components',
    '',
    'Harper Core 3.0.0: Apache-2.0. Its curated English dictionary and grammar rules are compiled into ExEditor.',
    'Spellbook 0.4.2: MPL-2.0. Original crate source is under source/. ExEditor uses the unmodified Hunspell-compatible parser.',
    'Divvun FST: MIT OR Apache-2.0, commit 7c3400754ef3f3e39b1c725f0ea18c1acd749b0d. Only the library is linked; its separately licensed command-line tools are not built.',
    'Libvoikko 4.3.3: the core is used under LGPL-2.1-or-later and supplied as a replaceable shared library. VFST is enabled; GPL-only Lttoolbox sources and HFST/VISLCG3 are not linked. Original source and build scripts are under source/. Upstream tri-license headers remain intact. Rebuilding this DLL does not require rebuilding ExEditor.',
    'Box-format: MIT OR Apache-2.0, upstream commit bff1974bdc3290759a86c5cebd123178a3b882f7. The sole upstream-file change aliases its xz2 dependency to the maintained liblzma wrapper. Original and complete patched source archives, PATCHES.md, and the resolved lockfile are under source/.',
    'Liblzma 0.4.9: MIT OR Apache-2.0. Its crate source and original license texts are included. The LZMA native library is linked statically.',
    'Additional native runtimes retain their original package notices under licenses/.',
    '',
    'No document text is sent to a service. Additional Hunspell, Divvun and Voikko dictionaries are selected from local files. Dictionary licenses are separate from engine licenses; no external language dictionaries are redistributed by this package.'
) | Set-Content -LiteralPath (Join-Path $destinationPath 'NOTICE.txt') -Encoding utf8
Write-Output ('Packaged native proofing dependencies: ' + $destinationPath)
