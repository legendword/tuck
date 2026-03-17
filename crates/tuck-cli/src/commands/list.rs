use colored::Colorize;
use humansize::{format_size, BINARY};
use tuck_core::config::Config;
use tuck_core::drive;
use tuck_core::error::TuckError;
use tuck_core::manifest::Manifest;

pub fn run(drive_name: Option<&str>, prefix: Option<&str>) -> Result<(), TuckError> {
    let config = Config::load()?;
    let drive = drive::resolve_drive(
        config.resolve_drive_name(drive_name),
        config.resolve_prefix(prefix),
    )?;
    let manifest = Manifest::load(&drive.root_path)?;

    if manifest.entries.is_empty() {
        println!("No archived entries on drive '{}'.", drive.name);
        return Ok(());
    }

    println!(
        "{} ({} entries on '{}'):\n",
        "Archived files".bold(),
        manifest.entries.len(),
        drive.name
    );

    for entry in &manifest.entries {
        let kind = if entry.is_directory { "dir " } else { "file" };
        let size = format_size(entry.size_bytes, BINARY);
        let date = entry.archived_at.format("%Y-%m-%d %H:%M");
        let files = entry.checksums.len();

        println!(
            "  [{}] {} ({}, {} file(s), archived {})",
            kind.dimmed(),
            entry.original_path.display().to_string().bold(),
            size,
            files,
            date
        );
    }

    println!();
    Ok(())
}
