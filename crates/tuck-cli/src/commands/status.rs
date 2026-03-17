use std::path::Path;

use colored::Colorize;
use humansize::{format_size, BINARY};
use tuck_core::drive;
use tuck_core::error::TuckError;
use tuck_core::verify;

pub fn run(path: &str) -> Result<(), TuckError> {
    let drives = drive::list_drives()?;

    if drives.is_empty() {
        println!("{}", "No external drives connected.".yellow());
        return Ok(());
    }

    let target = Path::new(path);
    let mut found = false;

    for d in &drives {
        if let Some(entry) = verify::check_status(target, d)? {
            found = true;
            let kind = if entry.is_directory {
                "directory"
            } else {
                "file"
            };
            let size = format_size(entry.size_bytes, BINARY);
            let date = entry.archived_at.format("%Y-%m-%d %H:%M");

            println!(
                "{} {} is archived on drive '{}'",
                "Found:".green().bold(),
                entry.original_path.display(),
                entry.drive_name
            );
            println!("  Type:     {}", kind);
            println!("  Size:     {}", size);
            println!("  Archived: {}", date);
            println!("  Files:    {}", entry.checksums.len());

            let local_exists = entry.original_path.exists();
            if local_exists {
                println!(
                    "  Local:    {}",
                    "still exists".yellow()
                );
            } else {
                println!(
                    "  Local:    {}",
                    "removed (archived only)".dimmed()
                );
            }
        }
    }

    if !found {
        println!(
            "{} is {} on any connected drive.",
            path,
            "not archived".dimmed()
        );
    }

    Ok(())
}
