# Changelog

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


