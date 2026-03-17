pub mod add;
pub mod config;
pub mod list;
pub mod restore;
pub mod status;
pub mod verify;

use colored::Colorize;
use dialoguer::Confirm;
use tuck_core::error::TuckError;
use tuck_core::pending::{PendingKind, PendingOperation};

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

    PendingOperation::cleanup(drive_root, &op)?;
    println!("{}\n", "Cleaned up successfully.".green());
    Ok(())
}
