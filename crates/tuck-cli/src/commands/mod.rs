pub mod add;
pub mod config;
pub mod list;
pub mod restore;
pub mod status;
pub mod update;
pub mod verify;

use colored::Colorize;
use dialoguer::{Confirm, Select};
use indicatif::{ProgressBar, ProgressStyle};
use std::cell::RefCell;
use tuck_core::config::Config;
use tuck_core::drive::{self, DriveInfo};
use tuck_core::error::TuckError;
use tuck_core::pending::{CleanupPlan, PendingKind, PendingOperation};
use tuck_core::progress::Progress;

/// Make text that comes from the drive (manifest entries, marker contents,
/// volume names) safe to print. See [`tuck_core::safety::printable`].
pub use tuck_core::safety::printable as safe;

/// Progress bar wrapper that implements tuck_core::progress::Progress.
pub struct CliProgress {
    bar: RefCell<ProgressBar>,
}

impl CliProgress {
    pub fn new() -> Self {
        Self {
            bar: RefCell::new(ProgressBar::hidden()),
        }
    }
}

impl Progress for CliProgress {
    fn start_phase(&self, phase: &str, total_bytes: u64) {
        // Replace the hidden/finished bar with a new visible one
        if total_bytes == 0 {
            let bar = ProgressBar::new_spinner();
            bar.set_style(
                ProgressStyle::default_spinner()
                    .template("  {spinner:.cyan} {msg}")
                    .unwrap(),
            );
            bar.set_message(format!("{}...", phase));
            bar.enable_steady_tick(std::time::Duration::from_millis(100));
            *self.bar.borrow_mut() = bar;
        } else {
            let bar = ProgressBar::new(total_bytes);
            bar.set_style(
                ProgressStyle::default_bar()
                    .template("  {msg} [{wide_bar:.cyan/dim}] {bytes}/{total_bytes} ({eta})")
                    .unwrap()
                    .progress_chars("=> "),
            );
            bar.set_message(phase.to_string());
            *self.bar.borrow_mut() = bar;
        }
    }

    fn advance(&self, bytes: u64) {
        self.bar.borrow().inc(bytes);
    }

    fn finish_phase(&self) {
        self.bar.borrow().finish_and_clear();
    }
}

/// Resolve a drive, prompting the user to select one interactively if not specified.
pub fn resolve_drive_interactive(config: &Config, drive_name: Option<&str>, prefix: Option<&str>) -> Result<DriveInfo, TuckError> {
    let effective_name = config.resolve_drive_name(drive_name);
    let effective_prefix = config.resolve_prefix(prefix);

    match drive::resolve_drive(effective_name, effective_prefix) {
        Ok(drive) => Ok(drive),
        Err(TuckError::NoDriveSpecified) => {
            let drives = drive::list_drives()?;
            match drives.len() {
                0 => Err(TuckError::NoDriveFound),
                1 => {
                    let drive = drives.into_iter().next().unwrap();
                    drive.with_prefix(effective_prefix)
                }
                _ => {
                    let names: Vec<String> = drives.iter().map(|d| safe(&d.name)).collect();
                    let selection = Select::new()
                        .with_prompt("Select a drive")
                        .items(&names)
                        .interact()
                        .map_err(|e| TuckError::Other(e.to_string()))?;
                    let drive = drives.into_iter().nth(selection).unwrap();
                    drive.with_prefix(effective_prefix)
                }
            }
        }
        Err(e) => Err(e),
    }
}

/// Check for a pending (interrupted) operation on the drive root.
/// If found, show exactly what cleaning it up would delete and ask first.
/// Returns Ok(()) if no pending op or if cleanup succeeded.
///
/// The marker is read from the drive, so it is never acted on as written:
/// `PendingOperation::inspect` decides what, if anything, may be deleted.
pub fn check_pending(drive_root: &std::path::Path) -> Result<(), TuckError> {
    let status = match PendingOperation::inspect(drive_root)? {
        Some(status) => status,
        None => return Ok(()),
    };

    match &status.op {
        Some(op) => {
            let action = match op.kind {
                PendingKind::Add => "archive",
                PendingKind::Restore => "restore",
            };
            println!(
                "\n{} A previous {} operation was interrupted:",
                "Warning:".yellow().bold(),
                action
            );
            println!("  Local path: {}", safe(op.original_path.display()));
            println!("  Started:    {}\n", op.started_at.format("%Y-%m-%d %H:%M:%S"));
        }
        None => println!(
            "\n{} The drive has a marker for an interrupted operation that tuck cannot read.\n",
            "Warning:".yellow().bold()
        ),
    }

    let prompt = match &status.plan {
        CleanupPlan::Remove(target) => {
            println!(
                "  Cleaning up will {} this path and everything in it:",
                "permanently delete".red()
            );
            println!("    {}", safe(target.display()));
            match status.op.as_ref().map(|op| op.kind) {
                Some(PendingKind::Restore) => {
                    println!("  It is the partial local copy from a restore started on this Mac.");
                    println!("  The archive on the drive is still intact.");
                }
                _ => {
                    println!("  It is the partial copy on the drive.");
                    println!("  The local files were not deleted and are still intact.");
                }
            }
            "Delete it and clear the interrupted operation?"
        }
        CleanupPlan::DiscardMarker { reason } => {
            println!("  Nothing will be deleted: {}.", safe(reason));
            println!("  Only the marker file on the drive will be removed.");
            "Remove the marker?"
        }
    };

    println!();
    let confirmed = Confirm::new()
        .with_prompt(prompt)
        .default(false)
        .interact()
        .map_err(|e| TuckError::Other(e.to_string()))?;

    if !confirmed {
        return Err(TuckError::Other(
            "Cannot proceed with a pending operation. Clean it up first.".to_string(),
        ));
    }

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner:.cyan} Cleaning up...")
            .unwrap(),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let result = status.resolve(drive_root);

    spinner.finish_and_clear();
    result?;
    println!("{}\n", "Cleaned up successfully.".green());
    Ok(())
}
