# ExEditor agent instructions

Instructions for every AI agent working in this repository. Read this file before doing anything else, and follow it over any other document except the maintainer's own words in the current conversation.

ExEditor is a macOS TextEdit-style desktop document editor written in Rust with Native Shell (NSH) for the UI and its native cross-platform RichEditor.

---

## 1. Reference material (read, never edit)

| What | Where |
| --- | --- |
| NSH tool and runtime (`nsh` nightly `00cfb08` / 2026-11-17) | `C:\dev\nsh` (`C:\dev\nsh\bin\nsh.exe`) |
| NSH source and docs (`main` `6442c86be`) | `C:\dev\nsh-src` (`AGENTS.md`, `website/docs/`, `docs/dev/`) and <https://nsh.dev> |

- Use the NSH tool and runtime in `C:\dev\nsh` and the source/documentation in `C:\dev\nsh-src`.
- Use only NSH APIs and components that exist in `C:\dev\nsh-src`. Always inspect `C:\dev\nsh-src` before calling or citing NSH APIs.
- UI dependencies pin to `rev = "6442c86be0a46f64dc1a11d8672fb746f64d96ba"` in `longbridge/nsh`, matching `C:\dev\nsh-src`.

---

## 2. Engineering rules and skills

**Always build code cleanly, modularly, and with proper architecture and separation of concerns. Never simply bolt on a feature because I asked for it. If you can't find an easy way, check online on GitHub for similar works that have been built with either exactly the same idea or something very close, and follow the same procedure to build it naturally.**

- When you write, review or refactor Rust, use the **`rust-best-practices`** skill (`/rust-best-practices`).
- Whenever anything related to a UI or interface is to be built or designed, use both the **`ui-ux-pro-max`** (`/ui-ux-pro-max`) and **`impeccable`** (`/impeccable`) skills, to keep the design consistent and the aesthetics high.
- A feature is a self-contained module: model, view, tests. No OS types in models; features don't reach into each other's internals.
- Every `unsafe` block has a `// SAFETY:` comment.
- Retain the MIT notice for the historical clipboard-adapter source.

---

## 3. Checks before you say a task is done

**Write all the code for a task first, then build and test.** Rust compiles slowly, and a full build takes minutes, so don't compile or run tests after each small edit. Finish every file the task touches (code, tests, call sites), then run the checks below once, and fix what they report. Exceptions: a quick `cargo check` when you need the compiler to settle a question you can't answer by reading, and anything that must be built before the rest can be written.

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Building with the cargo fork

Local builds use [huacnlee/cargo](https://github.com/huacnlee/cargo), a drop-in fork of Cargo that shares one self-cleaning build directory between projects and worktrees, so the disk doesn't fill up. Follow its rules:

- Check it is the active cargo before building: `cargo -V` ends with `(huacnlee/cargo …)`. If it doesn't (for example after `rustup self update`, which restores the stock cargo), report it to the maintainer; don't install it yourself.
- Don't run `cargo clean` and don't delete `target/` or `~/.cargo/targets/`. The fork removes what is unused on its own, and cleaning only forces a full rebuild.
- Don't set a build directory: `--target-dir`, `CARGO_TARGET_DIR`, `build.target-dir` and `build.build-dir` are ignored with a warning. Build output lives in `~/.cargo/targets/<name>-<hash>/`, registry dependencies in `~/.cargo/targets/shared-deps/`; built binaries still appear in `./target`.
- Its size limits and switches (`[build.gc]` in `~/.cargo/config.toml`, `CARGO_SHARED_DEPS`, `CARGO_SHARE_WORKSPACE_UNITS` and the like) are the maintainer's machine settings: leave them at their defaults unless asked.
- Start a debugger from the workspace root: the fork records `.` as rustc's compile directory.
- Nothing in the repository may depend on the fork: CI and other machines build with the stock cargo, and every check in this section must pass with either.

---

## 4. Project boundaries

- The application is ExEditor. Use Rust, the native NSH interface (`longbridge/nsh` `6442c86be`) and NSH RichEditor across desktop platforms. Do not reintroduce GPUI or a web editor. Do not restore WordCraft dependencies. App-owned offline codecs handle file interchange; Cosmic Text/Krilla handle independent PDF output, and craft-print submits to native queues.
- `dev_notes/` contains personal notes and backups. Never stage, commit or push it; keep downloaded `dist/` artifacts excluded from Git too.
- Update root `CHANGELOG.md` with every user-visible feature, fix, configuration change or meaningful documentation change, before committing/pushing. Use the actual America/New_York ISO timestamp with UTC offset, newest first. Record validation actually performed and material limitations; preserve historical entries and distinguish pending source from released builds.

---

## 5. Release numbering

- Follow ExcellentBible's scheme: `ExEditor-vYY-MM-DD.N-vSEMVER-platform`. Use the America/New_York release date and Cargo.toml package version; preserve existing semantic versions unless a version bump is requested.
- GitHub's shared release-name job resolves the date and lifetime release counter once, then passes that identity to every platform. Windows installers end in `-win-x64.exe`, portable ZIPs in `-win-x64-port.zip`; macOS/Linux packages retain their actual platform/architecture suffixes. About, package build-info and installer product metadata must agree.
- Keep downloaded deliveries under `dist/ExEditor-vYY-MM-DD.N-vSEMVER/` with those filenames. Preserve older deliveries and refuse to overwrite an existing numbered release. Do not rename the installed app or change its stable upgrade identity.
- `N` counts successful installer releases since the first ExEditor release, across all dates; never use GITHUB_RUN_NUMBER or reset it daily. Preserve the numbered `release-history.json` ledger and reconcile successful workflow runs not yet recorded there. Failed/cancelled workflows consume no number. After a successful delivery, append its exact source commit and workflow ID to the ledger in the documentation checkpoint with CI skipped; retain all previous rows. Do not start independent release builds concurrently across branches.

---

## 6. Process

- **Don't commit, push, open pull requests or tag releases unless the maintainer asks.** Then do exactly that.
- Don't use subagents unless the maintainer asks.
- Ask before deleting files, rewriting history, or changing anything outside this repository.
- Keep changes to the task: no unrequested features, options or refactors. Name anything you noticed but didn't do.
- End each task with a short report: what changed, what was checked, what wasn't, and any risk.
- Record every finished piece of work in `CHANGELOG.md`, newest first, under a heading with the local date and time.
