# ExEditor agent instructions

## Build tool

- Follow the build guidance in [huacnlee/cargo](https://github.com/huacnlee/cargo). Use the Cargo already installed on the execution machine through `cargo` on `PATH`; the user's Windows installation is this fork. Do not rebuild Cargo or replace the user's installation.
- Before a build, inspect `Get-Command cargo` (Windows) or `command -v cargo` (Unix), and `cargo -V`. The fork identifies itself with `huacnlee/cargo`. If another Cargo is selected, report the mismatch rather than silently bypassing or replacing the installed tool.
- Invoke `cargo` directly so the installed fork handles builds. Preserve `rust-toolchain.toml` and its pinned Rust compiler; do not switch to nightly for speed or invoke a different Cargo through `rustup run`.
- Keep existing build output. Never run `cargo clean`, remove `target/`, or delete the fork's shared caches as a routine troubleshooting step. Let the fork manage cache retention.
- Do not set `--target-dir`, `CARGO_TARGET_DIR`, `build.target-dir`, or `build.build-dir`: this fork manages its own build directories. Keep packaging pointed at the normal `target/` outputs.
- Preserve shared dependency/worktree caching and the project's release settings. If changing CI cache configuration, account for the fork's `~/.cargo/targets/` storage. Do not change global Cargo configuration without an explicit request.
- Build distributable ExEditor binaries with `cargo build --locked --release --bin exeditor` in the authorized build environment. Keep formatting, strict diagnostics, tests, and package checks intact.

## Build and test location

- Compilation, source checks and source tests run on GitHub, not on the user's device. Selecting the installed Cargo does not change this existing restriction.
- GitHub may build/package Windows, macOS and Linux. Execute tests and native application checks on Windows only until the user changes that scope.
- Local source inspection, formatting and tool-version checks are allowed. Finish feature source before starting that feature's GitHub validation.
- Save downloaded GitHub builds and their provenance under `dist/ExEditor-vYY-MM-DD.N-vSEMVER/`, following the existing installer, portable-package and build-record naming pattern.

## Project boundaries

- The application is ExEditor. Use Rust, the native nsh interface and nsh RichEditor across desktop platforms. Do not reintroduce GPUI or a web editor. WordCraft remains only for document/print codecs.
- `dev_notes/` contains personal notes and backups. Never stage, commit or push it; keep downloaded `dist/` artifacts excluded from Git too.
- Update root `CHANGELOG.md` with every user-visible feature, fix, configuration change or meaningful documentation change, before committing/pushing. Use the actual America/New_York ISO timestamp with UTC offset, newest first. Record validation actually performed and material limitations; preserve historical entries and distinguish pending source from released builds.

## Release numbering

- Follow ExcellentBible's scheme: `ExEditor-vYY-MM-DD.N-vSEMVER-platform`. Use the America/New_York release date and Cargo.toml package version; preserve existing semantic versions unless a version bump is requested.
- GitHub's shared release-name job resolves the date and workflow-run counter once, then passes that identity to every platform. Windows installers end in `-win-x64.exe`, portable ZIPs in `-win-x64-port.zip`; macOS/Linux packages retain their actual platform/architecture suffixes. About, package build-info and installer product metadata must agree.
- Keep downloaded deliveries under `dist/ExEditor-vYY-MM-DD.N-vSEMVER/` with those filenames. Preserve older deliveries and refuse to overwrite an existing numbered release. Do not rename the installed app or change its stable upgrade identity.
