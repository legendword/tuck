# CLAUDE.md

## Project overview

Tuck is a macOS CLI tool for archiving files/folders to an external drive and restoring them later. It's written in Rust as a Cargo workspace.

## Build & test

```bash
source "$HOME/.cargo/env"   # if cargo isn't in PATH
cargo check                 # type-check
cargo build                 # compile
cargo test                  # run all unit tests (currently 33)
cargo run --bin tuck -- <command>  # run the CLI
```

All tests are in-module `#[cfg(test)]` blocks using `tempfile` for isolation. No external services or drives needed for tests.

## Architecture

**Cargo workspace** with two crates:
- `tuck-core` (library) — all business logic, no UI concerns. Returns `TuckResult<T>` everywhere.
- `tuck-cli` (binary) — thin CLI layer using clap, colored output, dialoguer prompts. Dispatches to `tuck-core`.

This split exists so `tuck-core` can later be wrapped via FFI for a Swift/macOS GUI.

### Key modules in tuck-core

- `error.rs` — `TuckError` enum, `IoContext` trait for wrapping `std::io::Error` with path info
- `manifest.rs` — JSON manifest (``.tuck-manifest.json`) on the drive root. Atomic writes via .tmp+rename.
- `checksum.rs` — BLAKE3 streaming hashes, 64KB chunks. Hash/verify functions accept `Option<&dyn Progress>`.
- `progress.rs` — `Progress` trait (`start_phase`, `advance`, `finish_phase`). Core modules accept `Option<&dyn Progress>` to report byte-level progress. CLI implements it with `indicatif` progress bars.
- `config.rs` — loads/saves `~/.config/tuck/config.json` with `default_prefix` and `default_drive`. CLI commands load config and use `resolve_prefix()`/`resolve_drive_name()` to merge CLI flags with config defaults (CLI flag wins).
- `drive.rs` — scans `/Volumes/`, filters boot volume symlinks. `resolve_drive(name, prefix)` is the main entry point. `DriveInfo` has `mount_path` (physical mount) and `root_path` (effective tuck root, = `mount_path` or `mount_path/prefix`). All other modules use `root_path`. Also provides `check_space()` for disk space validation (used by `plan_add` and `plan_restore`).
- `copy.rs` — recursive copy preserving mtime via `filetime`. Skips symlinks. Accepts `Option<&dyn Progress>`.
- `pending.rs` — `PendingOperation` marker written to `.tuck-pending.json` before copy starts, cleared on success. If interrupted, the next command detects it and prompts cleanup. Handles both add (removes partial on drive) and restore (removes partial local copy).
- `archive.rs` — `plan_add` validates, `execute_add` does pending→hash→copy→verify→manifest→clear pending
- `restore.rs` — `plan_restore` finds entry, `execute_restore` does verify→pending→copy→manifest→clear pending
- `update.rs` — self-update via GitHub Releases API. `check_for_update` compares local version against latest release. `execute_update` downloads the binary with progress reporting and atomically replaces the current executable. Uses `ureq` for HTTP.
- `verify.rs` — `verify_entry`/`verify_all` check stored checksums against files on drive

### CLI commands (tuck-cli)

`add`, `restore`, `list`, `status`, `verify`, `update`, `config` — each in its own file under `src/commands/`. Shared helpers in `commands/mod.rs` include `CliProgress` (indicatif wrapper implementing `Progress` trait) and `check_pending` (detects interrupted operations).

### Release & distribution

- **GitHub Actions** (`.github/workflows/release.yml`) — on `v*` tag push, builds a universal macOS binary (aarch64 + x86_64 via `lipo`), publishes to GitHub Releases as `tuck-macos-universal`.
- **Install script** (`install.sh`) — `curl -fsSL .../install.sh | sh` downloads the latest release binary to `/usr/local/bin`.
- **Self-update** — `tuck update` checks the GitHub Releases API, downloads the new binary with progress, and atomically replaces itself.

## Conventions

- All paths stored in the manifest are canonicalized (absolute, symlinks resolved)
- Archive path on drive = drive root_path + original path with leading `/` stripped (root_path = mount_path when no prefix, mount_path/prefix otherwise)
- Error handling: library returns `TuckResult<T>`, CLI maps `TuckError::exit_code()` to process exit codes (0=ok, 1=general, 2=drive, 3=checksum, 4=cancelled)
- Disk space: `plan_add` checks drive space, `plan_restore` checks local disk space (finds nearest existing ancestor if target doesn't exist yet). Uses `fs2::available_space`.
- Symlinks within archived directories are skipped with a warning
- Progress reporting: core functions accept `Option<&dyn Progress>`, pass `None` in tests. CLI passes `Some(&CliProgress)` for user-facing operations

## Testing with a fake drive

Unit tests use `tempfile::TempDir`. For manual CLI testing, create a directory under `/Volumes/`:
```bash
sudo mkdir /Volumes/TestDrive
# ... run tuck commands with --drive TestDrive ...
sudo rm -rf /Volumes/TestDrive
```
