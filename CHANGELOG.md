# Changelog

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
