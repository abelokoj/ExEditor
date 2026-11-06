# Changelog

## 2026-11-06T17:32:33-05:00 - Empty-document line shortcuts and searchable native keyboard reference (local/uncommitted)

- GitHub run 42303795942 passed formatting, strict diagnostics and 151 of 152 tests. The prior Select All/fast typing, undo/redo, bold/italic/underline, navigation, chord, AltGr/focus/composition, owner-transfer and all-binding dispatch checks now pass. The remaining new line-edit test exposed RichEdit returning no rich fragment for an empty insertion range.
- Line transactions now skip an absent empty-range backup and remove the attempted insertion on rollback. Added empty-document insert-above/below, indentation, comment and single-undo regressions; selected-text and existing-fragment safeguards remain in place.
- Replaced the oversized shortcut message box with the existing searchable native GPUI panel. It lists canonical command bindings, searches both command names and keys, and keeps the existing keyboard navigation. The full alias/chord reference is bundled as KEYBOARD_SHORTCUTS.txt in installer/portable packages. Added reference/catalog and combined search regressions. No web editor or webview dependency.
- Local validation for this batch is source formatting and whitespace only. Combined GitHub regressions, packaging and installer checks remain pending; no local compilation or unit tests. Physical keyboard interaction and every international layout are not claimed as verified.

## 2026-11-06T17:19:12-05:00 - Select All excludes RichEdit's implicit paragraph from text operations (local/uncommitted fix)

- GitHub run 42302577247 passed formatting and strict diagnostics, and 149 of 151 tests. Every catalog binding passed through the real native message handler. Immediate Select All and Ctrl+Shift+End checks exposed that RichEdit reports its implicit final paragraph at text length + 1; passing that raw range to line-editing planners could reject valid selections.
- Clamp exported selections to actual text length, retain raw native ranges for formatting/printing internals, and exclude the implicit marker from selected plain text. Added empty, Unicode/emoji and trailing-newline regressions exercising Select All, selected text, line comments and undo.
- Local validation is source formatting and whitespace only. The corrected selection/typing/navigation regressions and combined installer remain pending on GitHub; no local compilation or unit-test execution.

## 2026-11-06T17:09:44-05:00 - GPUI Fast shortcut cancellation API (local/uncommitted correction)

- GitHub run 42301600686 passed formatting, then reported that prevent_default belongs to Window, not Context, in the selected GPUI Fast API. Corrected the document-window and menu keyboard handlers to call the Window method. The shortcut feature set is unchanged.
- Local source formatting only; compilation, strict diagnostics, regression execution and packaging remain pending on GitHub. No local compilation or unit-test execution. ExEditor retains its native GPUI Fast/RichEdit architecture; no web editor is being introduced.

## 2026-11-06T17:02:39-05:00 - Keyboard routing and VS Code editing bindings (local/uncommitted source)

- Consolidated 95 key combinations, aliases, menu labels and help into one catalog shared by the native document, GPUI window and menus. Added tracked window focus and document-focus restoration after toolbar/native-dialog interactions. Native shortcuts now dispatch synchronously before subsequent typing; busy modal updates retain the existing queued fallback. Callback state survives a shortcut closing its own tab/window, and transfers route to the new owner.
- Added Quick Open over open/recent documents (Ctrl+P), recent files (Ctrl+R), word completion (Ctrl+Space), tab reorder (Ctrl+Shift+PageUp/PageDown), menu access and print/page-setup chords. Corrected the zoom/font-size conflict and shifted punctuation handling; the ruler now uses Ctrl+Shift+R. Chord timeout/cancellation, AltGr input and focus/IME resets are consistent. Dialog fields retain their editing keys and Ctrl+Enter submissions are captured before field handlers.
- Added source regressions for every catalog entry through the real hidden Win32 key-message handler, immediate selection/typing/undo/redo/formatting, ordinary navigation, AltGr/composition/focus resets, fallback dispatch, replacement registrations, transferred owners and closing the active callback. Pure catalog, command-bridge and tab-order/buffer tests cover the remaining shared behavior. These tests do not inject input into the user's desktop.
- Local validation is source formatting and whitespace only; compilation, strict diagnostics, tests and packaging will run on GitHub after this completed feature batch is pushed. No local compilation or unit tests. Physical keyboard interactions, all international layouts and full VS Code behavior are not claimed: multicursor, workspace/language-server/debugger commands remain outside the current Windows RichEdit engine.

## 2026-11-06T02:15:26-05:00 — GPUI Fast, file-manager tabs and window moves ready for testing (recording time)

- GitHub run [42177182056](https://github.com/abelokoj/ExEditor/actions/runs/42177182056) completed formatting, strict diagnostics, all 134 regression tests, release application packaging and installer integration checks for committed application source a1116098da00476ecf4c384c7e971f53d9bf4455. The preceding batch entries retain their original pre-commit/pending status; those source changes are now committed and validated by this run.
- Downloaded the installer, portable ZIP/app and build records under dist/ExEditor-2026-11-6.1. Verified the source commit and all 44 packaged file checksums. This documentation-only follow-up does not change the tested executable.
- Ran only that downloaded portable app with synthetic files and separate application data. Three subsequent client launches exited successfully while the original process and document window remained in use. A Unicode/spaced path opened correctly, reopening it selected the same live native editor, and a two-file batch selected its last document. Source fixture hashes remained unchanged. The running main window class was GpuiFast::Window, confirming the selected backend in the executable.
- The initial smoke driver incorrectly counted only visible document surfaces as all tabs; the corrected check used window identity, selected text and native-editor reuse. The screen-capture safeguard refused capture because the target was not foreground, so no visual screenshot result is claimed. No typing or window-menu interactions were automated; those remain for user testing. All compilation and unit tests ran on GitHub, never locally.
- Window moves operate within one running session; close earlier builds before testing the new handoff behavior. The application remains Windows-only with its existing RichEdit document engine. NSH/editor-port work remains deferred. Personal notes, isolated test data, downloads and diagnostic artifacts stay ignored and unpublished.

## 2026-11-06T01:51:33-05:00 — Retry regression follows named-pipe readiness (local/uncommitted test correction)

- GitHub run 42176180950 passed formatting and strict diagnostics, then passed 133 of 134 tests. Native rich-text/selection/undo transfer, tab-order/buffer identity, canonical-file reuse, simultaneous launch election, concurrent batches, shutdown/re-election and queue bounds all passed.
- The remaining retry/malformed-message test called the one-attempt transport helper back-to-back and mistook its documented busy/reconnect RETRY response for the final protocol acknowledgement. The test now uses the same bounded readiness/retry behavior as production and still requires accepted duplicates to queue only once and malformed payloads to be rejected. Production IPC behavior is unchanged.
- Source formatting and whitespace checks only for this correction; GitHub rerun and combined installer remain pending. No local compilation or unit-test execution.

## 2026-11-06T01:42:01-05:00 — Strict checks for file handoff and window transfers (local/uncommitted correction)

- GitHub run 42175392691 reached both application and test source and reported three strict lint errors: nested transfer rollback conditions, fixed-size UTF-16 byte chunks and a cloned test slice. Applied the suggested equivalent forms without changing behavior.
- Local validation for this correction is source formatting and whitespace only. GitHub regression tests and the combined installer remain pending; no local compilation or unit-test execution. The GPUI Fast baseline run remains successful.

## 2026-11-06T01:33:11-05:00 — File-manager tabs and cross-window tab moves (local/uncommitted source)

- File-manager launches now elect or contact one ExEditor process per user/application-data folder and forward a bounded batch of absolute UTF-16 paths over a user-restricted local named pipe. Retry identities prevent duplicate delivery; an empty launch activates the existing editor. Normal last-window/quit paths check both queued and staged requests before closing.
- External opens target the most recently active document window, and reopening an existing canonical file selects its current tab without reloading unsaved edits. Ordinary Open, recent-file and RTFD folder commands also open tabs; explicit New Window retains its separate-window behavior.
- Added Window > Move Tab to Window and Move All Tabs to Window destination pickers using stable window identities. Transfers keep each live native document, Markdown source/preview pair, selection, undo history, queued recovery work and recovery lease. Tab order/selection are preserved and an emptied source window closes. Failed owner attachment retains the source and rolls back earlier attachments.
- Added regression source for native rich-text/selection/undo survival after the original owner closes, rejected transfers, logical tab order/buffer identity, canonical-file reuse, concurrent launches, Unicode/unpaired-surrogate payloads, malformed messages, queue bounds, shutdown/re-election and retry deduplication.
- The preceding GPUI Fast migration is committed as ba0748daafcacbbea23579a04c34d69ed7a926fd and GitHub run [42170660470](https://github.com/abelokoj/ExEditor/actions/runs/42170660470) completed successfully: formatting, strict diagnostics, regression tests, release packaging and installer integration checks passed. These additional tab/window changes have source formatting, manifest-resolution and whitespace checks only so far; their GitHub build/tests are pending. No local compilation or unit tests.
- Scope remains the current Windows application. NSH/editor-port work is deferred. Moves apply within one running session; earlier binaries without the handoff protocol cannot join it. A primary crash after IPC acknowledgement can interrupt a queued open request; original files are not modified by forwarding.

## 2026-11-06T00:45:43-05:00 — GPUI Fast throughout the existing interface (local/uncommitted)

- Enabled GPUI Kit's built-in gpui-fast dependency feature. Kit selects the Fast core and platform and propagates the same backend to Base, Component and bundled assets; all existing application imports already use Kit's facade.
- Kept the existing interface and Windows RichEdit document engine. This batch changes the rendering backend; it does not integrate NSH or implement the deferred cross-platform editor.
- Locked the Fast crate family to 0.1.13 and aligned its required shared taffy 0.15.0 and unicode-properties 0.1.3 versions. Other existing dependency versions were retained. Local dependency metadata confirms Fast is enabled in Kit, Base, Component and assets, including the Windows platform. Source whitespace checks passed; GitHub compilation, strict diagnostics, regression tests and installer checks are pending. No local compilation or unit-test execution.

## 2026-11-05T19:20:24-05:00 — Current Windows build ready for user testing (recording time)

- GitHub run [42133920727](https://github.com/abelokoj/ExEditor/actions/runs/42133920727) completed successfully for committed application source c5a7ac4295629e5efa60dd39123423d92db0d477. Formatting, strict Clippy, all 120 regression tests, release packaging and actual installer integration checks passed. The preceding batch entries describe their pre-commit state; those source changes are now committed and validated by this run.
- Downloaded installer, portable app and build records into dist/ExEditor-2026-11-5.7. Verified all 44 packaged file checksums and the recorded source commit. This documentation-only follow-up does not alter the tested application source.
- Ran the GitHub-built portable app locally with separate synthetic test data. Markdown containing relative/local/custom links opened without the previous error dialog, preserving heading/bold/italic presentation. Noto color faces, skin tone, heart, flag and crab were visible; the keycap sequence still showed a rendering artifact in the captured sample.
- Interactive typing/new-tab checks were not exercised: the automation safeguard refused input because the intended app was not foreground. Unchanged before/after text is not evidence of a successful input test. No local compilation or unit-test execution occurred. Full interactive assessment is left to the user's current-build testing.
- Parley is selected for the later shared cross-platform editor. That migration remains deferred until feedback on this Windows build; macOS/Linux applications are not delivered by this installer. Personal notes, downloaded binaries and smoke-test evidence remain ignored and unpublished.

## 2026-11-05T19:02:01-05:00 — Regression-driven link and dictionary fixes (local/uncommitted)

- GitHub strict checks passed and 117/120 tests passed. Fixed strict top-level HTML anchor validation; nested links were already validated and preview-only links still retain inactive labels.
- Selected Divvun's regular memory-mapped BHFST loader for bounded dictionaries, avoiding its chunked reader's demonstrated end-of-page panic while retaining the same spelling engine and real fixture test.
- Normalized the bundled Voikko DLL's absolute path/separators before restricted loading. Added runtime file/provenance evidence and native DLLs to the GitHub records so any remaining missing dependency can be identified precisely.
- The previously failing native style/undo marks, real Noto emoji and Harper/Hunspell tests all passed. This correction awaits GitHub rerun; no local compilation or ExEditor launch occurred.

## 2026-11-05T18:52:13-05:00 — Strict lint corrections and earlier build logs (local/uncommitted)

- GitHub type-checked the app and identified only strict lint errors in fixed-size pixel loops and single-range test slices. Used typed four-byte chunks and an explicit one-range slice without changing behavior.
- Publish build records before the large Cargo cache save, reducing diagnostic retrieval delays. Parley is the user's chosen future cross-platform foundation; that migration is explicitly deferred until after testing this current build.
- Validation: local formatting/whitespace only for this correction. Native Voikko packaging and app type checking succeeded on GitHub; regression tests, installer and local downloaded-binary testing remain pending. No local compilation.

## 2026-11-05T18:40:21-05:00 — Language-tool API and test-import corrections (local/uncommitted)

- GitHub accepted the Divvun memory-map alignment and reached ExEditor source. Imported the toolkit Selectable trait, used Divvun's inherent default constructor, and kept the test module's imports explicit to avoid GPUI attribute-macro recursion.
- Save the successfully built native package before application diagnostics so a later Rust error does not discard that cache. Local validation remains source formatting/whitespace only; GitHub tests and installer still pending. No local compilation or app launch.

## 2026-11-05T18:27:01-05:00 — Divvun archive dependency alignment (local/uncommitted correction)

- GitHub completed Voikko and dependency checks until Divvun rejected incompatible mmap-io Segment types (0.9.x versus 1.x). Aligned the vendored archive manifest with Divvun's 0.9.11 interface; retained the existing compression compatibility alias and documented both manifest-only changes.
- Validation: source/API inspection and dependency resolution only for this correction. GitHub application tests and downloadable build remain pending. No local compilation or app launch.

## 2026-11-05T18:16:06-05:00 — Native build metadata compatibility (local/uncommitted correction)

- GitHub successfully compiled the Voikko shared library, then failed while recording a removed MSYS2 package name. Record the actual complete installed package list instead of assuming gcc-libs remains a separate package.
- Cache the native package by its complete build/source/license/lock inputs; subsequent application-only corrections can reuse it. No local compilation or application launch occurred; GitHub app compilation and tests remain pending.

## 2026-11-05T18:08:58-05:00 — Markdown opening, Explorer integration, Noto emoji and offline language tools (local/uncommitted source)

- Markdown previews preserve unsupported, relative, local and custom-scheme link labels instead of rejecting the whole document. Source destinations remain intact; inactive links cannot launch unsafe schemes. Strict rich-document conversion still rejects unsafe links; permitted schemes now compare without case sensitivity and reject controls/quotes.
- Added per-user Edit with ExEditor Explorer verbs for files and RTFD bundle folders, enabled by default in the installer without changing default applications. Fixed named Open With values and added all Markdown extensions. Windows 11 uses Show more options; macOS/Linux native apps and file-manager integration remain unimplemented.
- Embedded pinned Google Noto Color Emoji with verified OFL/provenance. Whole-grapheme shaping draws the font's actual color bitmap glyphs, including full selections, with bounded caches and stable font/DPI/zoom geometry. Ambiguous partial selections, table cells and mixed backgrounds retain native drawing.
- Integrated offline Harper English grammar, Hunspell-compatible Spellbook dictionaries, pinned Divvunspell finite-state dictionaries and the packaged VFST-only Voikko shared runtime. Language Tools selects language/backend/local dictionary. Checks run off the interface thread with a 1 MiB text/500-finding bound; suggestions verify the complete checked document before applying Unicode-safe corrections. Temporary wavy marks preserve saved RTF, original styles and undo history.
- Vendored the exact box-format source with its original licenses and a manifest-only compression-wrapper alias to resolve Harper/Divvun's conflicting native LZMA dependencies. The lockfile resolves successfully. Native Voikko and proofing source/license notices are built and packaged on GitHub; no separately installed office suite is used.
- Added regression source for link fallback/safety, real Noto color glyphs and sequences, selected glyphs, rich-text marks, stale background checks, Harper grammar, Hunspell affixes/encoding, a real Divvun fixture and the actual packaged Voikko ABI/version. Added GitHub-only installer install/task/uninstall/default-preservation checks.
- Validation so far: local source formatting, whitespace checks, PowerShell static parsing and dependency resolution only. No local app compilation or unit-test execution. GitHub build/runtime results are pending. The user now authorizes running/testing downloaded GitHub binaries locally in isolated test data; all downloads belong under dist/ using its dated layout. Personal dev_notes remain ignored and unpublished.

## 2026-11-05T16:58:10-05:00 — GitHub installer validation (recording time)

- GitHub run [42115720539](https://github.com/abelokoj/ExEditor/actions/runs/42115720539) validated committed application source f40961777f2f97d97e24c9d6f9f75af030aea8ba: formatting and strict Clippy passed, all 97 regression tests passed, and the release executable, portable package and installer were built and uploaded successfully.
- [Installer artifact](https://github.com/abelokoj/ExEditor/actions/runs/42115720539/artifacts/13457326222) includes numbered unsaved titles and their configurable prefix, recovered title metadata, VS Code-style editing commands, stable emoji metrics, and the document-safety fixes recorded above. The installer was downloaded locally without launching it.
- This entry records the completed build/test/package steps; GitHub cache cleanup was still running at recording time. No local compilation or interactive/visual testing was performed. Interactive testing belongs to the user. Multiple cursors and VS Code workspace/debugger features remain unsupported.
- This documentation-only follow-up does not change the installer source; its commit skips a redundant build. Personal dev_notes remain ignored and unpublished.

## 2026-11-05T16:46:07-05:00 — Atomic rich line-edit undo (local/uncommitted)

- Kept rich fragment imports inside the caller-owned undo collection; nested stream collections had prematurely ended grouping, so one Undo only reversed the last fragment. Normal standalone rich insertions retain their own collection.
- Extended the native regression to check the original bold style after Undo and the full moved text after one Redo.
- Validation: GitHub formatting and strict Clippy passed; 96 of 97 tests passed, exposing this undo failure. This correction has local formatting/whitespace checks only; its GitHub rerun is pending. No local compilation or interactive testing.

## 2026-11-05T16:38:12-05:00 — Command palette diagnostic corrections (local/uncommitted)

- Simplified the palette command variant and conditional invocation after GitHub strict Clippy reported enum_variant_names and collapsible_if. No shortcut behavior changed.
- Validation: GitHub accepted the native pointer correction and reached Clippy; regression tests remain pending. Local formatting and whitespace checks only; no local compilation or application launch.

## 2026-11-05T16:34:31-05:00 — Native editor build correction (local/uncommitted)

- Gave the RichEdit COM out-pointer its explicit native type after GitHub reported E0282.
- Validation: the preceding GitHub run passed formatting but stopped at compilation; tests and packaging did not run. Local source formatting/whitespace checks only; no local build or application launch.

## 2026-11-05T16:25:16-05:00 — Document safety, stable emoji, editing shortcuts and numbered names (local source batch)

- Kept loaded and acknowledged external-file versions separate so declining Reload cannot silently authorize overwriting another application's changes.
- Replaced destructive dark-color substitutions and RTF color-table rewriting with native temporary display formatting. Typed TOM interfaces now manage formatting/undo resources; print/PDF hides temporary colors with automatic restoration.
- Markdown preview uses a separate native view while retaining the source editor's contents, undo history, selection and pending edits. Save As rich text deliberately converts the preview into the new editable document.
- Fixed incremental highlighting after appended comments/strings and parser failures. Added complete UTF-16 style comparisons for append/delete/Unicode/long-line cases.
- Stabilized emoji geometry with native font size, per-monitor DPI and zoom, rather than caret positions inside a sequence. Preserve local backgrounds and native table/mixed-background rendering; clip to reserved cluster bounds, pair every Direct2D draw and release/reset failed resources. Cached native layout settings avoid repeated reflow while idle.
- Added VS Code-style line movement/copy/deletion/insertion, indentation, line/block comments, bracket navigation, scrolling, Find shortcuts, Save All, reopening saved files, full screen, Ctrl+K chords, searchable Command Palette and keyboard help. Rich fragment provenance preserves styling through line edits; multiple simultaneous cursors and VS Code workspace/debugger features remain unsupported.
- New documents receive process-wide numbered titles (Untitled 1, Untitled 2). Settings accepts a validated naming prefix with live examples. Assigned names persist in backward-compatible recovery metadata, and restored numbers advance later naming.
- Recovery snapshots write off the interface thread with one outstanding job per tab. Exclusive leases survive pending writes; save/discard/close drain earlier snapshots so stale work cannot resurrect a buffer.
- Validation performed locally: source/API inspection, rustfmt and whitespace checks only. Regression-test source covers native color identity, preview undo/selection, rich line movement, print cleanup, writer ownership, editing plans, shortcuts, emoji geometry and names. No local compilation, application launch or interactive testing; GitHub build/check result pending. Personal dev_notes remain ignored and excluded.

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


