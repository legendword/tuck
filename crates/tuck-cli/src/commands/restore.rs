use std::path::Path;

use colored::Colorize;
use dialoguer::Confirm;
use humansize::{format_size, BINARY};
use tuck_core::config::Config;
use tuck_core::error::TuckError;
use tuck_core::restore;

use super::CliProgress;

pub fn run(
    path: &str,
    drive_name: Option<&str>,
    prefix: Option<&str>,
    dry_run: bool,
    force: bool,
    keep_archive: bool,
) -> Result<(), TuckError> {
    let config = Config::load()?;
    let drive = super::resolve_drive_interactive(&config, drive_name, prefix)?;
    super::check_pending(&drive.root_path)?;
    let plan = restore::plan_restore(Path::new(path), &drive)?;

    let kind = if plan.entry.is_directory {
        "directory"
    } else {
        "file"
    };
    let size = format_size(plan.entry.size_bytes, BINARY);

    println!(
        "{} {}",
        "Restore:".bold(),
        plan.archive_path.display()
    );
    println!(
        "{} {} ({}, {})",
        "     To:".bold(),
        plan.original_path.display(),
        kind,
        size
    );

    if plan.local_exists && !force {
        return Err(TuckError::AlreadyExists(plan.original_path.clone()));
    }
    if plan.local_exists && force {
        println!(
            "  {}",
            "Local path exists — will be overwritten (--force).".yellow()
        );
    }

    if dry_run {
        println!("{}", "Dry run — no changes made.".yellow());
        return Ok(());
    }

    let msg = if keep_archive {
        "Restore to original location?"
    } else {
        "Restore to original location and remove archive copy?"
    };
    let confirmed = Confirm::new()
        .with_prompt(msg)
        .default(false)
        .interact()
        .map_err(|e| TuckError::Other(e.to_string()))?;
    if !confirmed {
        return Err(TuckError::Cancelled);
    }

    let progress = CliProgress::new();
    restore::execute_restore(&plan, keep_archive, Some(&progress))?;

    println!("{}", "Restored successfully.".green().bold());
    Ok(())
}
