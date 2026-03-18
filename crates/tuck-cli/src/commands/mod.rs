pub mod add;
pub mod config;
pub mod list;
pub mod restore;
pub mod status;
pub mod update;
pub mod verify;

use colored::Colorize;
use dialoguer::Confirm;
use indicatif::{ProgressBar, ProgressStyle};
use tuck_core::error::TuckError;
use tuck_core::pending::{PendingKind, PendingOperation};
use tuck_core::progress::Progress;

/// Progress bar wrapper that implements tuck_core::progress::Progress.
pub struct CliProgress {
    bar: ProgressBar,
}

impl CliProgress {
    pub fn new() -> Self {
        Self {
            bar: ProgressBar::hidden(),
        }
    }
}

impl Progress for CliProgress {
    fn start_phase(&self, phase: &str, total_bytes: u64) {
        if total_bytes == 0 {
            // Use a spinner for indeterminate operations (e.g. deleting)
            self.bar.set_style(
                ProgressStyle::default_spinner()
                    .template("  {spinner:.cyan} {msg}")
                    .unwrap(),
            );
            self.bar.set_message(format!("{}...", phase));
            self.bar.set_length(0);
            self.bar.enable_steady_tick(std::time::Duration::from_millis(100));
        } else {
            self.bar.set_style(
                ProgressStyle::default_bar()
                    .template("  {msg} [{wide_bar:.cyan/dim}] {bytes}/{total_bytes} ({eta})")
                    .unwrap()
                    .progress_chars("=> "),
            );
            self.bar.set_message(phase.to_string());
            self.bar.set_length(total_bytes);
            self.bar.set_position(0);
            self.bar.reset_eta();
        }
    }

    fn advance(&self, bytes: u64) {
        self.bar.inc(bytes);
    }

    fn finish_phase(&self) {
        self.bar.finish_and_clear();
    }
}

/// Check for a pending (interrupted) operation on the drive root.
/// If found, prompt the user to clean up before continuing.
/// Returns Ok(()) if no pending op or if cleanup succeeded.
pub fn check_pending(drive_root: &std::path::Path) -> Result<(), TuckError> {
    let pending = PendingOperation::load(drive_root)?;
    let op = match pending {
        Some(op) => op,
        None => return Ok(()),
    };

    let action = match op.kind {
        PendingKind::Add => "archive",
        PendingKind::Restore => "restore",
    };

    println!(
        "\n{} A previous {} operation was interrupted:",
        "Warning:".yellow().bold(),
        action
    );
    println!("  Source: {}", op.original_path.display());
    println!("  Target: {}", op.archive_path.display());
    println!("  Started: {}\n", op.started_at.format("%Y-%m-%d %H:%M:%S"));

    match op.kind {
        PendingKind::Add => {
            println!(
                "  This will {} the partial copy on the drive.",
                "remove".red()
            );
            println!("  The local file was not deleted and is still intact.");
        }
        PendingKind::Restore => {
            println!(
                "  This will {} the partial local copy.",
                "remove".red()
            );
            println!("  The archive on the drive is still intact.");
        }
    }

    println!();
    let confirmed = Confirm::new()
        .with_prompt("Clean up the incomplete operation?")
        .default(true)
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

    PendingOperation::cleanup(drive_root, &op)?;

    spinner.finish_and_clear();
    println!("{}\n", "Cleaned up successfully.".green());
    Ok(())
}
