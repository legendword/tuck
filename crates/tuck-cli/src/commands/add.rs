use std::path::Path;

use colored::Colorize;
use dialoguer::Confirm;
use humansize::{format_size, BINARY};
use tuck_core::archive;
use tuck_core::drive;
use tuck_core::error::TuckError;

pub fn run(
    path: &str,
    drive_name: Option<&str>,
    dry_run: bool,
    no_confirm: bool,
    keep_local: bool,
) -> Result<(), TuckError> {
    let drive = drive::resolve_drive(drive_name)?;
    let plan = archive::plan_add(Path::new(path), &drive)?;

    let kind = if plan.is_directory { "directory" } else { "file" };
    let size = format_size(plan.size_bytes, BINARY);

    println!(
        "{} {} ({}, {})",
        "Archive:".bold(),
        plan.original_path.display(),
        kind,
        size
    );
    println!(
        "{} {}",
        "     To:".bold(),
        plan.archive_path.display()
    );

    if dry_run {
        println!("{}", "Dry run — no changes made.".yellow());
        return Ok(());
    }

    if !no_confirm {
        let msg = if keep_local {
            "Copy to external drive?"
        } else {
            "Copy to external drive and delete local copy?"
        };
        let confirmed = Confirm::new()
            .with_prompt(msg)
            .default(false)
            .interact()
            .map_err(|e| TuckError::Other(e.to_string()))?;
        if !confirmed {
            return Err(TuckError::Cancelled);
        }
    }

    print!("Copying and verifying checksums... ");
    let checksums = archive::execute_add(&plan)?;
    println!("{}", "done.".green());

    println!(
        "  {} file(s) archived, all checksums verified.",
        checksums.len().to_string().bold()
    );

    if !keep_local {
        archive::delete_local(&plan.original_path)?;
        println!(
            "  Local copy {}.",
            "deleted".red()
        );
    }

    println!("{}", "Archived successfully.".green().bold());
    Ok(())
}
