# ExEditor

ExEditor is a native Rust document editor whose interface and workflows follow macOS TextEdit. **WordCraft is the document engine; GPUI Kit with GPUI Fast draws the interface.** There is no browser editor or external LibreOffice dependency.

The WordCraft migration replaces the previous Windows RichEdit host. Its source includes native document input and IME, formatting, selection, undo/redo, tables, images, links, formatted clipboard operations, pagination, PDF export and system printing. Windows, macOS and Linux are GitHub build targets. Build validation and interactive acceptance are recorded in [CHANGELOG.md](CHANGELOG.md); source features are not a guarantee of complete TextEdit or Word compatibility.

## Builds and testing

All compilation, diagnostics and Rust tests run on [GitHub Actions](https://github.com/abelokoj/ExEditor/actions). Do not compile or run source tests locally. Finish feature source before starting a workflow. Pushes to `main` and `nsh-edit`, pull requests and manual dispatches run the build workflow.

Download artifacts from a successful run. Windows includes an installer and a portable package; macOS includes an application bundle; Linux includes a portable archive. Build-record artifacts contain diagnostics, test logs and the resolved lockfile. The package's `build-info.json` identifies the source commit and architecture, and `SHA256SUMS.txt` records its contents. macOS bundles use an ad-hoc signature, not a Developer ID or notarization.

Keep downloaded GitHub builds in `dist/ExEditor-YYYY-M-D.N/`, with the installer, portable archive/extracted folder and build records together. Windows packages must retain their bundled proofing runtime and licenses. Launch downloaded builds with an isolated `EXEDITOR_DATA_DIR` for interactive checks. Keep private development notes and source backups in ignored `dev_notes/`; never commit or push them.

For Linux, extract the archive and run `python3 install-desktop.py` in its folder to register that portable location in the current user's application menu. On macOS, place ExEditor.app in Applications and select it in Finder's Open With menu. On Windows, the installer offers Edit with ExEditor and Open With integration without taking over default associations.

## Editing

- Plain text and rich text with font family/size, bold, italic, underline, strikethrough, superscript/subscript, colors, paragraph alignment, spacing, indents, lists and tabs.
- Tables, links and embedded images; RTF/RTFD, HTML, DOCX and ODT documents. Legacy Word `.doc` imports are available; save an edited copy as DOCX or RTF.
- Find/replace, line editing and VS Code style desktop shortcuts. See [the complete shortcut reference](docs/KEYBOARD_SHORTCUTS.txt). Command is also accepted for Control-based application shortcuts on macOS.
- Automatic offline suggestions after three letters, using nearby document words and language keywords. Ctrl/Command+Space opens suggestions manually; arrows choose, Tab/Enter accepts, Escape dismisses. Disable automatic suggestions in Settings.
- Syntax coloring and light/dark code themes for recognized plain-text files. Screen colors do not rewrite saved document styles. Markdown has formatted reading and source modes.
- Rich clipboard copy/paste and explicit Paste Formatted HTML. Clipboard import never fetches remote media or reads linked local media. X11 rich copy provides HTML and text; other supported desktop clipboard adapters also publish RTF.
- Numbered untitled tabs with a configurable prefix, reopening files into existing tabs, and moving tabs between windows while retaining their editing sessions.
- Printer choice, copies, page ranges, duplex and grayscale settings, plus PDF export through the same document layout.

## Recovery and fonts

Unsaved documents retain automatic local snapshots. New snapshots include the complete WordCraft model alongside backward-compatible plain text/RTF content; older ExEditor snapshots remain readable. Recovery preserves original snapshots when opening fails. Safe saves detect outside modifications and retain version history.

Bundled fonts are Pretendard Std Regular/Bold with synthesized italics, Maple Mono NF Regular/Italic/Bold/Bold Italic, Inter Regular/Italic/Bold/Bold Italic and Noto Color Emoji. Font registration stays in the current process.

Harper provides offline grammar checking. Language Tools also supports Hunspell dictionaries, Divvun spelling and Voikko where its native runtime/dictionaries are available. Windows packages include the native proofing runtime. The Windows system dictionary remains a Windows-only provider; choose an offline provider on other platforms.

## Scope and compatibility

WordCraft's engine is used independently of its egui frontend. ExEditor retains its own interface; adopting the engine does not reproduce every WordCraft application feature. Imported headers, footers and text boxes remain in the document model and output, but this editor surface currently edits the main document body. Broad real-world document fidelity, native IME, screen-reader use and physical printers still require platform acceptance testing.

## Interface reference

The following captures show the previous released ExEditor interface. The WordCraft surface retains that chrome; these are not validation screenshots of the migrated engine.

![ExEditor document interface](docs/screenshots/exeditor-document.png)

## Attribution

WordCraft is pinned to [729cd0a5](https://github.com/storytold/wordcraft/tree/729cd0a5fc953bbedbfdec4f6af18ad6d1780b3b). See [the adapter attribution](assets/licenses/wordcraft/NOTICE.txt) and the bundled font/proofing license files. ExEditor is distributed under [MIT](LICENSE).
