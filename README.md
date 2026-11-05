# ExEditor

ExEditor is a Windows desktop document editor written in Rust with GPUI Kit. Its
interface and editing workflows aim to replicate macOS TextEdit. GPUI Kit owns
the desktop interface, and Windows RichEdit provides the native rich-text engine.
Document handling stays within the application and the operating system; no
separate office suite or document-converter installation is required.

GitHub has produced Windows installer and portable builds. Full TextEdit visual
and document compatibility remain under evaluation; interactive testing is performed
by the user on downloaded builds.

## Build on GitHub, test the downloaded app here

All compilation happens in [GitHub Actions](https://github.com/abelokoj/ExEditor/actions).
The workflow runs on pushes to `main`, pull requests, and manual dispatches. The
first build must be started only after all planned feature source code is written.
It checks Rust formatting and diagnostics, runs noninteractive unit tests, builds
the Windows x64 executable, and packages the application. Visual and interactive
testing is performed by the user with that downloaded executable on the local Windows machine.

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

Personal development notes stay local and are excluded from GitHub, including the published branch history. The repository is
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

## Unsaved buffers and Colors

Unsaved tabs are protected in the local application-data folder as you edit. Closing
an application window or quitting preserves its unsaved buffers without a save prompt;
healthy buffers return as tabs automatically at startup, including when opening files
from Explorer. Separate running instances cannot claim the same recovery buffer.
Save removes that tab's recovery copy. Explicitly closing a dirty tab offers Save,
Discard, or Cancel; Discard removes its buffer. A backup write failure keeps the window
open. Damaged snapshots remain on disk with an error message instead of being deleted.
Recovered changes require confirmation before replacing an existing saved file.

The shared Colors panel follows CotEditor's macOS Colors workflow: wheel, color
sliders, stylesheet palette, image sampling, pencils, opacity, eleven color-code
formats, screen sampling, and 24 saved swatches. Open it from text color/background
controls or **Format > Font > Insert Color Code**. The panel uses native GPUI controls;
it does not depend on CotEditor or macOS frameworks. Rich-text foreground/background
colors blend opacity into the background because Windows RichEdit stores opaque colors;
inserted CSS codes preserve alpha when the selected code format supports it.

The default icon is the selected paper-and-pencil artwork with calligraphic Ex.
All three calligraphic PNG variants are retained in assets/icons/paper-calligraphy.

The latest changes still need the user's visual and interaction feedback. In particular,
a previously observed blank native document surface has not been resolved or ruled out
by a new interactive test. Unsaved-buffer snapshots are atomic and flushed, but an
abrupt crash can still lose edits made since the most recent editing checkpoint.
