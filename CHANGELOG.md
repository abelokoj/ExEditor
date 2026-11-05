# Changelog

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
