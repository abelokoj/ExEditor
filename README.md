# ExEditor

ExEditor is a Windows desktop document editor written in Rust with GPUI Kit. Its
interface and editing workflows aim to replicate macOS TextEdit. GPUI Kit owns
the desktop interface, with its `gpui-fast` feature selecting GPUI Fast for the
core, platform, components and assets. Existing `gpui_kit` imports use that
backend throughout the application. Windows RichEdit provides the native
rich-text engine.
Document handling stays within the application and the operating system; no
separate office suite or document-converter installation is required.

GitHub has produced Windows installer and portable builds. Full TextEdit visual
and document compatibility remain under evaluation; interactive testing uses downloaded GitHub builds in isolated local data folders.

## Build on GitHub, test the downloaded app here

All compilation happens in [GitHub Actions](https://github.com/abelokoj/ExEditor/actions).
The workflow runs on pushes to `main`, pull requests, and manual dispatches. The
first build must be started only after all planned feature source code is written.
It checks Rust formatting and diagnostics, runs noninteractive unit tests, builds
the Windows x64 executable, and packages the application. Visual and interactive
testing runs that downloaded executable on the local Windows machine in isolated data folders.

1. Open a successful **Build ExEditor** workflow run.
2. Download **ExEditor-windows-x64-<source commit>** from its Artifacts section.
3. Keep downloads under `dist/ExEditor-YYYY-M-D.N/`, matching the existing layout:
   installer, portable ZIP, extracted portable folder and build-records folder. Keep
   bundled DLLs, the proofing folder and `build-info.json` alongside `ExEditor.exe`.
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

The latest changes need visual and interaction checks on downloaded GitHub builds. In particular,
the earlier blank writing area was addressed by hosting RichEdit above GPUI; current
interactive behavior still needs user feedback. Unsaved-buffer snapshots are atomic and flushed, but an
abrupt crash can still lose edits made since the most recent editing checkpoint.

## Tabs and windows

Opening files through **Open with ExEditor** or **Edit with ExEditor** hands them to
an already running ExEditor and opens them as tabs in its most recently active
document window. Selecting multiple files opens them together. Reopening a file
already present in any document window selects its existing tab and preserves its
unsaved edits. File > Open, recent files and RTFD folder opens also use tabs.

Use **Window > New Window** when a separate document window is wanted. Under
**Window > Move Tab to Window**, choose another open document window to move the
current tab there. **Window > Move All Tabs to Window** moves the current window's
whole tab group into the chosen window, preserving its order and selected document.
An emptied source window closes. Moves keep the live native document controls,
including undo history, selection, formatting, unsaved changes and recovery ownership.
The commands apply to document windows in the same running ExEditor session.

The file-open handoff is scoped to the current Windows user and application-data
folder; a separate EXEDITOR_DATA_DIR creates an independent session for testing.
It uses a restricted local named pipe, bounded Unicode requests and retry IDs.
Normal shutdown defers closing when accepted file requests still need handling.
An abrupt process crash after acknowledging a request can interrupt that handoff;
the original file on disk remains intact and can be opened again.

## Editing shortcuts and document names

ExEditor supports familiar VS Code editing shortcuts, including moving/copying lines,
indent/outdent, line and block comments, bracket navigation, scrolling, Find/Replace,
Save All, reopening closed saved files, Markdown source/preview, and the searchable
Command Palette (F1 or Ctrl+Shift+P). **Help > Keyboard Shortcuts** opens a searchable native binding list. The full
reference, including aliases and chord instructions, ships as KEYBOARD_SHORTCUTS.txt.
Ctrl+P searches open and recent files; Ctrl+O opens the file picker. Ctrl+R opens
recent files, Ctrl+Space shows word completion, and Ctrl+Shift+PageUp/PageDown move
the current tab left/right. Ctrl+K starts a three-second chord; Escape cancels it.
Menu labels and both keyboard input paths share one binding catalog. Ctrl+Plus
zooms, Ctrl+Shift+period/comma change font size, and Ctrl+Shift+R toggles the ruler.
The source reference is [Microsoft's Windows shortcut sheet](https://code.visualstudio.com/shortcuts/keyboard-shortcuts-windows.pdf).
VS Code workspace/debugger commands and multiple simultaneous cursors are outside the
native editor's current capabilities. Rich-text formatting shortcuts remain available.

New documents receive names such as Untitled 1 and Untitled 2 across tabs and windows.
Change the word under **Settings > New Document > New document names**; for example,
Notes produces Notes 3 next. Existing unsaved documents keep their assigned names,
and recovery reserves their numbers before later new documents. Saved files display
their actual file names.

Display colors use RichEdit's temporary formatting; saved/recovered RTF retains its
original color identities. Markdown preview keeps a separate native view, preserving
the source buffer's undo history, selection and unsaved changes. Print/PDF suppresses
temporary display colors. Recovery writes run in the background with one pending
snapshot per tab; saving, discarding and closing drain earlier writes to prevent a stale
snapshot from returning.

## Markdown links and File Explorer

Markdown previews keep reading normally when they contain relative paths, local-file
links, or custom URL schemes. Unsupported destinations are displayed as inactive
labels; their complete destinations remain in the editable Markdown source. HTTP,
HTTPS, mail, telephone and document-anchor links retain supported hyperlink formatting.
Strict rich-document import continues to reject unsafe links.

The installer enables **Edit with ExEditor** in File Explorer. It supports Markdown,
rich document formats, and plain/source files with arbitrary extensions, without
changing existing default applications. On Windows 11 the command is in **Show more
options**. RTFD bundle folders receive the same command. Rerun the installer to change
the context-menu option; uninstalling removes ExEditor's own registrations. The optional
**Open with** registration includes all five supported Markdown extensions.

ExEditor currently requires Windows RichEdit. macOS and Linux applications and their
file-manager integrations are not implemented; they require native editor ports.

## Offline spelling, grammar and emoji

**Edit > Spelling and Grammar > Language Tools** (also available in Settings) selects
Harper English grammar and the spelling backend: Windows, Hunspell, Divvunspell or
Voikko. The spelling language is explicit. Import a matching Hunspell `.aff`/`.dic`
pair, a Divvun `.bhfst`/`.zhfst` dictionary, or a Voikko dictionary folder for the
chosen engine. Imported dictionaries retain the licenses of their dictionary
packages. Harper's curated English grammar resources are built in; disable grammar
for non-English documents. The bundled Voikko runtime and its source/license notices
ship with installer and portable packages. No separate office suite is required.

Checks run offline in the background. Check While Typing shows temporary wavy marks;
Show Suggestions offers corrections and explanations. A correction is applied only
while the complete checked document and its UTF-16 range still match. Marks do not
enter saved RTF, recovery buffers, or undo history. Language checks are bounded to
1 MiB of text and 500 findings. Harper checks plain English; Markdown markup is not
masked before language analysis.

Emoji use an embedded, pinned Google **Noto Color Emoji** font under the SIL Open Font
License. Whole Unicode graphemes are shaped against that font and its real color PNG
glyphs are drawn over RichEdit, including fully selected emoji. Nothing is downloaded
at runtime or installed into the system font directory. Ambiguous partial selections,
mixed backgrounds and table cells retain native drawing. The original Unicode stays
in both plain and rich documents. Interactive visual testing uses the downloaded GitHub binary in an isolated data folder.
