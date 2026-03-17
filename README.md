# Tuck

A macOS CLI tool for intentionally archiving files and folders to an external drive, freeing up local space, and reliably restoring them to their exact original paths later.

Unlike backup tools (which use retention policies that purge old snapshots) or sync tools (which mirror deletions), Tuck treats your external drive as permanent, intentional storage — files stay archived until you explicitly restore them.

## Install

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

## How it works

- **Drive detection**: Scans `/Volumes/`, skips the boot volume and hidden entries. Auto-detects if one drive is connected; asks you to specify if multiple.
- **Path mapping**: Strips leading `/` and mirrors the directory structure on the drive. `/Users/you/Documents/foo.txt` → `/Volumes/Drive/Users/you/Documents/foo.txt`.
- **Checksums**: BLAKE3, streamed in 64KB chunks. Files are hashed before copy, hashed again after copy, and compared. Stored per-file for granular verification.
- **Manifest**: A `.tuck-manifest.json` file on the drive root tracks all archived entries with original paths, timestamps, sizes, and checksums. Written atomically (write `.tmp`, then rename).
- **Symlinks**: Skipped with a warning (v1).

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
  crates/
    tuck-core/             # Library — all logic, no CLI concerns
      src/
        lib.rs
        error.rs           # TuckError, TuckResult, IoContext
        manifest.rs        # Manifest load/save, entry management
        checksum.rs        # BLAKE3 hashing
        drive.rs           # /Volumes/ scanning, drive resolution
        copy.rs            # Recursive copy with metadata preservation
        archive.rs         # Plan and execute archive operations
        restore.rs         # Plan and execute restore operations
        verify.rs          # Checksum verification, status checks
    tuck-cli/              # Binary — CLI interface only
      src/
        main.rs            # Clap arg parsing, dispatch
        commands/
          add.rs
          restore.rs
          list.rs
          status.rs
          verify.rs
```

The workspace is split into `tuck-core` (library) and `tuck-cli` (binary) so the core logic can be reused by a future Swift GUI via FFI.

## License

TBD
