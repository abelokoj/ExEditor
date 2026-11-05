# Changelog

## 2026-11-05 — Color emoji, scrolling tabs, plain-text font on open, Markdown layout

- Emoji show in color, as on phones and in other apps, in plain, rich, code and Markdown documents. Windowed RichEdit can only draw them in black and white, so the editor now redraws each visible emoji on top in color with Direct2D and DirectWrite (Segoe UI Emoji). Whole sequences such as flags, skin tones, keycaps and family emoji are drawn as one picture. Selected emoji keep RichEdit's own highlight drawing.
- When the tabs no longer fit the window, the tab strip scrolls sideways (mouse wheel or trackpad) instead of squeezing the tabs. The tab you switch to or open scrolls into view, and the "+" button stays visible at the end.
- Fixed plain and code files opening in the rich-text font (Pretendard) instead of the plain-text font (Maple Mono NF). Opening a file and restoring a recovered buffer now apply the plain-text font and size from Settings.
- Markdown view: nested list items are indented, the stray blank line before a nested list is gone, paragraphs have a little space after them, and body text uses the rich-text font instead of Arial. List items no longer start with a stray space. HTML import shares the list and whitespace fixes.
- Checked in build 2026-11-5.5 (isolated data folder): code coloring with Automatic and Dracula, dark document paper, dark title bar, and the Markdown view. The live theme preview was not exercised by hand.

## 2026-11-05 — Dark documents, code coloring, Markdown, menus that run, closing

- Fixed menus and settings that did nothing (Interface Size among them). A message box's modal loop could run the window's 100 ms poll while the window was busy; that failed update stopped the poll for good, so queued menu commands never ran. The poll now skips that tick and stops only when the window is gone.
- Interface Size now scales gpui-kit buttons and labels too: every window sets its rem size from the interface scale.
- Dark mode:
  - Buttons, tooltips, inputs and scroll bars follow the app's dark appearance (gpui-kit theme sync).
  - The Colors panel has a dark palette, matching the toolbar.
  - View > Dark Document Background (on by default) shows documents on dark paper. Automatic text is shown light and still saved as automatic color, so files look right in other apps.
- Code coloring for about 170 languages (syntect with bat's grammar set via two-face), detected by file name, extension, or first line (shebangs). Coloring is incremental: only edited lines and the lines whose state they change are recolored, without undo steps or a modified mark. Files over 2 MB and lines over 10,000 characters stay uncolored.
- View > Code Theme: Automatic plus 28 editor themes (Catppuccin, Dracula, GitHub, Gruvbox, Monokai Extended, Nord, One Half, Solarized, Zenburn and more). Hovering or arrowing over a theme previews it on the open code document; clicking or Enter keeps it, and closing the menu any other way returns to the saved theme. The menu opens on the current theme.
- Changing the font in Settings now shows at once in plain documents and in empty rich documents (rich text that already has styling keeps it, as in TextEdit).
- Markdown (.md, .markdown) opens formatted: headings, emphasis, code, nested lists, tables, task lists, footnotes; remote images show their alt text. View > Show Markdown Source switches to editable source, and saving always writes the Markdown source.
- Open shows All Files, Documents (now with Markdown) and Source Code.
- Closing:
  - The app quits when its last document window closes, even if helper windows are hidden.
  - A buffer that cannot be protected now asks whether to close anyway instead of trapping the window open.
  - Unedited documents no longer write recovery buffers.
  - Declining "Document changed on disk" no longer asks again every three seconds.
- Plain documents stay one style, and paste stays plain text. RichEdit itself stays in rich mode so code can be colored.
- Hardening: HTML and OpenDocument imports refuse nesting deeper than 128 levels instead of overflowing the stack; a lone surrogate in the text no longer blocks saving or closing; HTML list items holding nested lists import as nested lists; Markdown and HTML code use the bundled monospace font.
- Testing: EXEDITOR_DATA_DIR points preferences and recovery buffers at another folder for isolated test runs.
- Validation: formatted with rustfmt 1.99.1. Nothing compiled or tested locally; the GitHub build is pending.

## 2026-11-05 — Startup format error, window dragging, TextEdit shortcuts, interface size (local)

- Fixed "Windows could not apply the character formatting" at startup with plain text as the default. Plain-text RichEdit rejects selection formatting, so font and size now apply to the whole plain document. Character and paragraph styling commands do nothing in plain text, matching TextEdit's disabled items.
- Fixed windows that could not be dragged. GPUI's start_window_move does nothing on Windows. The document title bar, Colors and Settings now mark their title areas as drag regions (HTCAPTION); the title-bar buttons keep their clicks.
- Shortcuts follow TextEdit (Cmd becomes Ctrl) and also work while the window rather than the document has focus:
  - Format: Ctrl+B/I/U, Ctrl+Shift+C (Show Colors), Ctrl++ / Ctrl+- (Bigger / Smaller), Ctrl+{ / Ctrl+| / Ctrl+} (align left / center / right).
  - Edit: Ctrl+Shift+Z (Redo), Ctrl+Shift+V (Paste and Match Style), Ctrl+J (Jump to Selection), Ctrl+; and Ctrl+Shift+; (spelling).
  - View: zoom moved to Ctrl+Shift+. / Ctrl+Shift+, so Ctrl++ / Ctrl+- resize the font as in TextEdit.
- From the TextEdit gap audit:
  - Format > Font > Copy Style / Paste Style, shared across windows, with no shortcut because Ctrl+Alt is AltGr on many layouts.
  - Ctrl+R toggles the ruler instead of RichEdit's hidden align-right, and Ctrl+Shift+W toggles Wrap to Page.
  - Jump to Selection now scrolls to the selection.
  - Complete no longer shows an "Esc" shortcut that did nothing.
- Custom font sizes: Format > Font > Other Size… and the toolbar size menu's Other… accept any size from 1 to 1000 points. Bigger and Smaller step by one point, and the toolbar shows fractional sizes.
- View > Interface Size scales the whole interface: 75–200% presets or a custom percentage (50–300%). The setting is saved, and the document's own zoom is separate.
- Toolbar matches TextEdit's controls:
  - B I U S buttons draw their own effects.
  - The text background control is a color well.
  - Line spacing has an icon.
  - Icon buttons have names in their tooltips.
- Validation: formatting checked with the pinned 1.99.1 rustfmt, and GPUI behavior checked against gpui-pre 0.3.11, gpui-pre-windows 0.3.11 and gpui-component 0.9.0 sources. No local compilation or app testing performed; the GitHub build is pending.

## 2026-11-05 — Typing restored: document editor hosted above GPUI (local fix)

- Fixed the blank, untypeable document area. GPUI draws through a topmost DirectComposition target with no GDI redirection surface (`WS_EX_NOREDIRECTIONBITMAP`), so the RichEdit child window was invisible and unreachable. RichEdit now lives in a borderless popup owned by the GPUI window, placed over the editor rectangle in screen coordinates and moved with the window; it hides with a hidden or minimized owner.
- Activation passing between the window and its document host no longer marks the GPUI window inactive, so smart quotes/dashes and active-window chrome keep working while typing. Activation from other apps is still forwarded. The first document focuses itself when it appears.
- Added a regression test for host placement and child sizing.
- Colors panel color codes now follow CotEditor's WFColorCode 4.0.2 (the package CotEditor pins) instead of general CSS parsing:
  - Output: `rgb(r,g,b)` and `hsl(h,s%,l%)` without spaces. HSL/HWB values are whole numbers (gray keeps hue 0). Alpha has at most two decimals, so 50% reads `0.5`, not `0.502`.
  - Parsing follows CotEditor's grammar. Function names must be lowercase. Legacy `rgb()` takes 0–255 integers only, and legacy `hsl()` requires `%`. `hwb()` has no comma form and there is no `hwba()`. Out-of-range values, signs, exponents, units, `transparent`, bare hex without `#`, and other color spaces are rejected instead of clamped.
  - Keywords ignore alpha when matching a name. Hex case follows the code field's current text.
  - Short hex still falls back to exact long hex where CotEditor would leave the field unchanged.
- Validation: formatting checked with the pinned 1.99.1 rustfmt. Windows API names checked against windows-sys 0.61.2 and GPUI's window code (gpui-pre-windows 0.3.11), and color conversions checked against csscolorparser 0.9.0. No local compilation or app testing performed; the GitHub build is pending.

## 2026-11-05T02:47:31-05:00 — Windows resource linking and color-field Enter behavior (local fixes)

- Removed ExEditor's duplicate process-manifest resource: GPUI already embeds the Windows asInvoker, common-controls and PerMonitorV2 settings. Retained the selected application icon and version resources; removed the now-unused manifest template.
- Made Enter in the color-code field parse/update the selected color, matching CotEditor. Apply/Insert and Ctrl+Enter commit the color action separately.
- Validation: GitHub run 41989293275 passed formatting and strict Clippy but linking the test binary failed because two manifest resources shared ID 1; no regression tests or installer ran. Inspected both resource definitions and formatted this fix. No local compilation or app testing performed.

## 2026-11-05T02:43:16-05:00 — Strict Rust lint integration (local fixes)

- Applied the four let-chain changes requested by strict Clippy in GitHub run 41988878649. The earlier compiler errors are resolved; this run stopped at lint checks before tests and packaging.
- Validation: inspected remote diagnostics and formatted the corrected source. No local compilation or app testing performed; replacement online build pending.

## 2026-11-05T02:39:41-05:00 — Colors panel compiler integration (local fixes)

- Imported the GPUI disabled-control trait and selection index type, annotated mouse events and color-channel bit operations to resolve the seven compiler errors in GitHub run 41988334949.
- Validation: inspected the remote diagnostics and formatted the corrected source. The initial remote run passed formatting but did not reach tests, compilation or packaging. No local application build or launch performed; the next remote result is pending.
- Verified the local push guard rejects the former notes-bearing history and accepts the rewritten notes-free branch. Private notes remain local.

## 2026-11-05T02:34:10-05:00 — Colors panel, protected buffers and selected icon (local source batch)

- Connected a shared CotEditor-style Colors panel to foreground/background controls and color-code insertion. Implemented all five modes, CSS formats, opacity, saved swatches, image sampling and a native screen sampler that runs off the app thread. Formatting alpha is blended because RichEdit stores opaque text colors; alpha-capable inserted CSS codes retain it.
- Automatically restore protected unsaved buffers as tabs at every startup. Snapshot edits at editing checkpoints and flush all unsaved tabs before window close or quit without Save prompts; preserve damaged snapshots, enforce exclusive ownership between running instances, and block closing if backup writing fails. Explicit tab Discard removes its buffer. Recovered files require confirmation before overwriting an existing saved file.
- Selected calligraphic Ex paper-and-pencil variant 1 as the default executable, taskbar, installer and shortcut icon; retained all three PNG variants. Added the previously omitted Windows resource build hook.
- Kept personal dev_notes files local and installed a local pre-push guard rejecting branches that contain them anywhere in history. The published history was already rewritten under the user's explicit approval.
- Validation performed locally: source formatting, dependency resolution only (no compilation), icon frame/alpha inspection, tracked-path and history inspection; all 52 local note files were hash-checked unchanged. Regression-test source was written; the GitHub build/check result is pending.
- No app launch or interactive/visual testing performed. User owns app testing. Full TextEdit parity is not established, and the previously observed blank native document surface remains unverified. An abrupt crash can lose edits since the last editing checkpoint; disk failures are reported instead of silently dropping buffers.

## 2026-11-05T02:20:37-05:00 — Private notes removed from published history (recording time)

- Rewrote the published main branch at the user's request to remove dev_notes/ from every commit. Local personal notes remain intact and the folder remains ignored for future pushes.
- Validation: the filtered history contains no dev_notes paths. This maintenance changes published commit IDs; older clones must use the rewritten history before pushing again. Copies previously downloaded by others and GitHub caches cannot be recalled by rewriting a branch.
- No application build, launch or test performed; feature work remains in progress locally.

## 2026-11-05T02:13:10-05:00 — Personal notes excluded from GitHub (local privacy fix)

- Excluded dev_notes/ and local icon-generation prompt records from Git tracking. Remove previously tracked personal notes from the published branch while preserving every local file.
- Removed README links to private development notes. Earlier published history removal requires the user's separate decision because it changes existing commit IDs.
- Validation: inspected tracked paths and ignore rules only. No application build, launch or test performed.

## 2026-11-05T01:59:31-05:00 — Calligraphic paper-and-pencil icon variants (local, uncommitted)

- User clarified that the chosen base is original option 1, the paper-and-pencil icon. Generated three upper-left inscriptions: calligraphic Ex, calligraphic ExEditor, and LaTeX-inspired ExEditor.
- Saved three transparent PNG previews and their exact prompts under assets/icons/paper-calligraphy. No variant selected, converted to an application icon, integrated or released.
- Validation: visually inspected the three generated previews. Variant 2 has a rough transparent perimeter; a requested cleanup pass failed because the image-generation usage limit was reached.
- No application build, launch or automated test performed in this icon batch. App testing remains with the user pending feedback.

## 2026-11-05T01:49:38-05:00 — Icon concepts and user-led testing (local, uncommitted)

- Generated three transparent ExEditor app icon concepts: document/pencil, blue text/caret, and Ex monogram. Saved original PNG concepts and exact prompts in assets/icons/options; no icon selected or integrated yet.
- User requested to perform app testing personally and provide feedback. Further automated app testing is deferred to that feedback; the previously observed blank document surface remains unresolved.
- Validation: visually inspected the three generated concepts. No additional app build, launch, or test was performed in this icon batch.

## 2026-11-05T01:31:52-05:00 — Remote checks passed; resource hook staged locally

- GitHub run 41979561408 passed formatting, strict Clippy and the noninteractive regression suite; release compilation and packaging are still running. No installer or local UI result claimed yet.
- Include the existing Windows resource build hook in the next source batch so version metadata and the DPI manifest are actually embedded; the hook was written earlier but omitted from the initial Git staging list.
- No application compilation or Rust test execution occurred locally.

## 2026-11-05T01:17:59-05:00 — Rust diagnostics cleanup (local changes)

- Connected encoding fallback, custom line spacing, color presets, and first-line/hanging indents; removed an unused toolbar receiver and moved native helper tests after implementation items.
- Applied GitHub-reported let-chain and chunk-iteration improvements; passed a raw mutable output pointer for the private Windows font registration boundary.
- Added a local UI evidence batch for downloaded artifacts. Validation: GitHub run 41978641841 resolved the previous API type errors but failed strict lint checks before tests/build/package. No app built locally; next remote pass pending.


## 2026-11-05T01:09:40-05:00 — Workflow restoration and dependency lock (local fix)

- Restored the build workflow after GitHub rejected run 41978493552 before scheduling a job; corrected accidental non-YAML content introduced in the previous batch.
- Added the first GitHub-generated Cargo.lock to preserve the resolved dependency versions. Compiler fixes from the first run remain in source.
- Validation: GitHub run status and downloaded first-run build records inspected. No successful app build or local UI test yet; a new GitHub run is required.


## 2026-11-05T01:08:21-05:00 — First GitHub diagnostics (local fixes, not yet validated)

- Corrected eight GPUI/Windows API integration errors reported by GitHub run 41978002534; keep native window handles distinct from raw OS handles and adapt ruler/accessibility APIs.
- Preserve dependency compilation cache on failed runs to shorten later GitHub build iterations; keep downloaded diagnostic files out of source control.
- Validation: first remote formatting check passed; remote compiler failed before tests/package/installer. No app built locally. Next remote pass pending; no installer artifact exists yet.


## 2026-11-05T01:03:27-05:00 — Initial source implementation (local, uncommitted)

- Added ExEditor, a Rust + GPUI Kit Windows editor with TextEdit-style document controls, native RichEdit, file handling, recovery, settings, typography, spelling/speech, and printing/PDF.
- Added GitHub-only build/check/artifact workflow, portable package and per-user installer source. No separate office-suite dependency.
- Bundled Pretendard Std, Maple Mono NF and Inter with OFL notices; Pretendard italic synthesis explicitly approved by the user.
- Renamed ExcellentNotes folder to ExEditor, recorded repository/stack/build choices in dev_notes, and retained real TextEdit reference screenshots.
- Validation: source/API/documentation reads and font metadata inspection. No application build, test or launch performed yet. Initial Rust formatting started only after all feature source was written.
- Limitations: no exact-parity claim; Windows-specific implementation, Apple services absent, binary .doc and advanced document content unsupported. See dev_notes/known-differences.md. GitHub artifacts and local visual testing remain pending.


