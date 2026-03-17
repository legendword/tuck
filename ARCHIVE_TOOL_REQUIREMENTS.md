# Archive Tool Requirements

## Problem Statement

Existing backup tools (Time Machine, BorgBackup, restic, etc.) are designed as rolling backups with retention policies that eventually purge old snapshots. Sync tools (rsync, FreeFileSync) mirror deletions, making them unsafe for archiving. Neither category is well-suited for the core use case:

> Intentionally offload specific files/folders from a Mac to an external hard drive, free up local space by deleting the local copy, and reliably restore files to their exact original paths at a future date.

## Core Workflow

1. User selects file(s) or folder(s) to archive
2. Tool copies them to the external drive, preserving the original path structure
3. Tool records the original path in a manifest
4. Tool verifies the copy is intact before proceeding
5. Tool deletes the local copy (with confirmation)
6. At a future date, user restores a file/folder — tool recreates the full original path on the Mac

## CLI Commands

```
archive add <path> [--drive <drive-name>]   # Archive a file or folder
archive restore <path>                       # Restore by original path
archive list                                 # Show all archived items
archive status <path>                        # Check if a path is archived
archive verify                               # Verify archived files are intact on drive
```

## Manifest

- Stored on the external drive (travels with the archive, not tied to one Mac)
- Also optionally mirrored locally for quick `list`/`status` lookups
- Records per entry:
  - Original absolute path
  - Archive date
  - File/folder size
  - Checksum (for integrity verification)
  - External drive name/identifier

## Folder Structure on External Drive

Mirror the original Mac path structure for human browsability:

```
ExternalDrive/
  .archive-manifest.json       # Manifest file
  Users/username/Documents/ProjectX/
  Users/username/Pictures/2023/
  ...
```

## Safety Requirements

- Verify checksum of copied data before deleting local copy
- Require explicit confirmation before deleting local files
- `--dry-run` flag to preview what would happen without making changes
- Never silently overwrite existing files on either end

## Restore Behavior

- Recreate full directory path on Mac if it no longer exists
- Warn if a file already exists at the restore destination
- Support restoring a whole folder or individual files within an archived folder

## External Drive Handling

- Identify drives by name (volume label) rather than mount path (mount paths change)
- Graceful error if the specified drive is not currently connected
- Support multiple external drives (each with their own manifest)

## Out of Scope (for CLI v1)

- GUI
- Cloud storage destinations (S3, iCloud, etc.)
- Encryption
- Compression
- Scheduling / automation
- Cross-platform support (macOS only for now)

## Future Considerations

- GUI app (menu bar or standalone)
- `archive mount` — browse archived files without restoring
- Encryption of archived data
- Support for multiple Macs sharing one archive drive
