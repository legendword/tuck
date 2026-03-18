use colored::Colorize;
use dialoguer::Confirm;
use tuck_core::error::TuckError;
use tuck_core::update;

use super::CliProgress;

pub fn run(check: bool, force: bool) -> Result<(), TuckError> {
    println!("Checking for updates...");

    let info = update::check_for_update(env!("CARGO_PKG_VERSION"))?;

    if !info.is_newer {
        println!(
            "Already up to date ({}).",
            format!("v{}", info.current_version).bold()
        );
        return Ok(());
    }

    println!(
        "Update available: {} -> {}",
        format!("v{}", info.current_version).dimmed(),
        format!("v{}", info.latest_version).green().bold()
    );

    if check {
        return Ok(());
    }

    if !force {
        let confirmed = Confirm::new()
            .with_prompt("Install update?")
            .default(true)
            .interact()
            .map_err(|e| TuckError::Other(e.to_string()))?;
        if !confirmed {
            return Err(TuckError::Cancelled);
        }
    }

    let progress = CliProgress::new();
    update::execute_update(&info, Some(&progress))?;

    println!(
        "{}",
        format!("Updated to v{}. Restart tuck to use the new version.", info.latest_version)
            .green()
            .bold()
    );
    Ok(())
}
