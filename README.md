# Tuck

A macOS CLI tool for intentionally archiving files and folders to an external drive, freeing up local space, and reliably restoring them to their exact original paths later. **macOS only** (relies on `/Volumes/` for drive detection).

Unlike backup tools (which use retention policies that purge old snapshots) or sync tools (which mirror deletions), Tuck treats your external drive as permanent, intentional storage — files stay archived until you explicitly restore them.

## Install

### macOS app

Download `Tuck.dmg` from the [latest release](https://github.com/legendword/tuck/releases/latest) and drag Tuck to Applications.

### CLI

```bash
curl -fsSL https://raw.githubusercontent.com/legendword/tuck/main/install.sh | sh
```

This downloads the latest universal binary from [GitHub Releases](https://github.com/legendword/tuck/releases) and installs it to `/usr/local/bin`.

To update to the latest version:

```bash
tuck update
```

### From source

Requires [Rust](https://rustup.rs/).

```bash
cargo install --path crates/tuck-cli
```

## Usage

### Archive a file or folder

```bash
tuck add ~/Documents/BigProject --drive MyDrive
```

This will:
1. Hash all files (BLAKE3)
2. Copy to the drive (preserving directory structure and modification times)
3. Verify checksums on the destination
4. Delete the local copy

Flags:
- `--drive <name>` — specify drive (auto-detected if only one connected)
- `--prefix <folder>` — use a subfolder on the drive as root (see [Using a prefix](#using-a-prefix))
- `--dry-run` — preview without making changes
- `--no-confirm` — skip confirmation prompt
- `--keep-local` — archive without deleting the local copy

### Restore

```bash
tuck restore ~/Documents/BigProject
```

Restores files to their exact original path. Verifies checksums before restoring.

Flags:
- `--force` — overwrite if local path already exists
- `--keep-archive` — keep the copy on the drive after restoring
- `--dry-run` — preview without making changes

### List archived entries

```bash
tuck list
```

### Check if a path is archived

```bash
tuck status ~/Documents/BigProject
```

### Verify archive integrity

```bash
tuck verify
```

Checks BLAKE3 checksums of all archived files on the drive.

### Update

```bash
tuck update          # download and install the latest version
tuck update --check  # check for updates without installing
```

### Using a prefix

If your external drive contains other files, or you share a drive between multiple Macs, use `--prefix` to scope all tuck data under a subfolder:

```bash
tuck add ~/Documents/BigProject --prefix tuck-macbook
tuck list --prefix tuck-macbook
tuck restore ~/Documents/BigProject --prefix tuck-macbook
```

This stores everything under `/Volumes/Drive/tuck-macbook/` instead of the drive root — including the manifest and all archived files. Each prefix gets its own independent manifest, so two machines can use the same drive with different prefixes (e.g. `tuck-macbook` and `tuck-imac`) without interfering.

To avoid passing `--prefix` every time, set a default:

```bash
tuck config set-prefix tuck-macbook
```

Now all commands automatically use `tuck-macbook` as the prefix. You can still override it with `--prefix` on any individual command. Similarly, `tuck config set-drive MyDrive` sets a default drive.

```bash
tuck config show     # view current defaults
tuck config set-prefix ""  # clear the default
```

Config is stored at `~/.config/tuck/config.json`.

## How it works

- **Drive detection**: Scans `/Volumes/`, skips the boot volume and hidden entries. Auto-detects if one drive is connected; asks you to specify if multiple.
- **Path mapping**: Strips leading `/` and mirrors the directory structure on the drive. `/Users/you/Documents/foo.txt` → `/Volumes/Drive/Users/you/Documents/foo.txt`. With `--prefix myprefix`, the root becomes `/Volumes/Drive/myprefix/`.
- **Checksums**: BLAKE3, streamed in 64KB chunks. Files are hashed before copy, hashed again after copy, and compared. Stored per-file for granular verification.
- **Manifest**: A `.tuck-manifest.json` file on the drive root tracks all archived entries with original paths, timestamps, sizes, and checksums. Written atomically (write `.tmp`, then rename).
- **Disk space**: Both `add` and `restore` check available disk space before starting and fail early if insufficient.
- **Symlinks**: Skipped with a warning (v1).
- **Self-update**: `tuck update` checks GitHub Releases for a newer version, downloads the binary, and atomically replaces the current executable.

## Testing locally

Run via cargo:

```bash
cargo run --bin tuck -- add ~/file.txt --drive MyDrive
cargo run --bin tuck -- list --drive MyDrive
```

To simulate a drive without a real external disk:

```bash
sudo mkdir /Volumes/TestDrive
cargo run --bin tuck -- add /tmp/test.txt --drive TestDrive
sudo rm -rf /Volumes/TestDrive
```

Run unit tests:

```bash
cargo test
```

## Project structure

```
tuck/
  Cargo.toml              # Workspace root
  install.sh              # One-line install script
  build-ffi.sh            # Build universal FFI lib + Swift bindings
  .github/workflows/
    release.yml            # CI: build CLI + macOS app, publish GitHub Release
  crates/
    tuck-core/             # Library — all logic, no CLI/UI concerns
      src/
        lib.rs
        error.rs           # TuckError, TuckResult, IoContext
        manifest.rs        # Manifest load/save, entry management
        checksum.rs        # BLAKE3 hashing
        config.rs          # Persistent config (~/.config/tuck/config.json)
        drive.rs           # /Volumes/ scanning, drive resolution, disk space checks
        copy.rs            # Recursive copy with metadata preservation
        pending.rs         # Interrupted operation recovery
        progress.rs        # Progress trait for byte-level reporting
        archive.rs         # Plan and execute archive operations
        restore.rs         # Plan and execute restore operations
        update.rs          # Self-update via GitHub Releases
        verify.rs          # Checksum verification, status checks
    tuck-cli/              # Binary — CLI interface only
      src/
        main.rs            # Clap arg parsing, dispatch
        commands/
          add.rs
          restore.rs
          list.rs
          status.rs
          update.rs
          verify.rs
    tuck-ffi/              # Static library — UniFFI bridge to Swift
      src/
        lib.rs
        types.rs           # Mirror types with uniffi::Record
        error.rs           # FfiTuckError with uniffi::Error
        progress.rs        # Callback interface for progress reporting
        functions.rs       # Exported FFI functions
  TuckApp/                 # SwiftUI macOS app
    project.yml            # xcodegen project spec
    TuckApp/               # Swift sources
    Generated/             # Auto-generated Swift bindings (from build-ffi.sh)
```

The workspace is split into three crates:
- `tuck-core` (library) — all business logic
- `tuck-cli` (binary) — CLI interface
- `tuck-ffi` (staticlib) — UniFFI wrapper exposing `tuck-core` to Swift

The macOS app lives in `TuckApp/` and uses SwiftUI with the FFI bindings.

### Building the macOS app

Requires [Rust](https://rustup.rs/) and [xcodegen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`).

```bash
./build-ffi.sh                        # build universal static lib + generate Swift bindings
cd TuckApp && xcodegen generate       # generate .xcodeproj from project.yml
xcodebuild build -project TuckApp.xcodeproj -scheme TuckApp
```

**Important:** After changing any Rust code (in `tuck-core` or `tuck-ffi`), you must re-run `./build-ffi.sh` before rebuilding the app. The script builds the FFI library for both `aarch64` and `x86_64`, creates a universal binary via `lipo`, and generates the Swift bindings at `TuckApp/Generated/`.

## License

[MIT](LICENSE)
