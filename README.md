# ExEditor

ExEditor is a Windows desktop document editor written in Rust with GPUI Kit. Its
interface and editing workflows aim to replicate macOS TextEdit. GPUI Kit owns
the desktop interface, and Windows RichEdit provides the native rich-text engine.
Document handling stays within the application and the operating system; no
separate office suite or document-converter installation is required.

The implementation and TextEdit parity checks are in progress. No successful
build, file-compatibility result, or visual-parity result is claimed yet.

## Build on GitHub, test the downloaded app here

All compilation happens in [GitHub Actions](https://github.com/abelokoj/ExEditor/actions).
The workflow runs on pushes to `main`, pull requests, and manual dispatches. The
first build must be started only after all planned feature source code is written.
It checks Rust formatting and diagnostics, runs noninteractive unit tests, builds
the Windows x64 executable, and packages the application. Visual and interactive
testing uses that downloaded executable on the local Windows machine.

1. Open a successful **Build ExEditor** workflow run.
2. Download **ExEditor-windows-x64-<source commit>** from its Artifacts section.
3. Extract the entire archive into a new folder. Keep any bundled DLLs and
   `build-info.json` alongside `ExEditor.exe`.
4. Launch `ExEditor.exe` and use test documents for the visual and behavior checks.
5. Compare with the selected TextEdit reference, record findings, change source,
   and download the next GitHub build. Do not compile the application locally.

The separate **ExEditor-build-records-<source commit>** artifact contains workflow
logs and the resolved `Cargo.lock`. On the first build, commit that returned
lockfile to make subsequent `--locked` builds reproducible. Artifacts remain
available for 14 days. They are workflow downloads, not public releases.

The project keeps build provenance in `build-info.json` and file checksums in
`SHA256SUMS.txt`. Rust 1.99.1 is pinned in source. Visual matching and document
compatibility must be measured against actual TextEdit reference documents;
the toolkit alone does not establish parity with Apple-only services or macOS
rendering.

Personal development notes stay local and are excluded from GitHub. The repository is
[abelokoj/ExEditor](https://github.com/abelokoj/ExEditor).

## Installer and document fonts

The **ExEditor-installer-windows-x64-<commit>** Actions artifact contains
`ExEditor-Setup-x64.exe`, a per-user installer. It leaves existing file defaults
and user documents alone. The portable artifact remains available separately.

Embedded document fonts are Pretendard Std (rich text), Maple Mono NF (plain
text), and Inter (optional). Genuine Regular/Bold faces of Pretendard use
runtime italic synthesis, approved by the user. Maple Mono NF and Inter include
Regular/Italic/Bold/BoldItalic. Font OFL notices ship with the package.

Legacy .doc and some advanced import/export content are unsupported. Full TextEdit compatibility has not been established.
