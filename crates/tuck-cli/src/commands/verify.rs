use colored::Colorize;
use tuck_core::config::Config;
use tuck_core::error::TuckError;
use tuck_core::verify;

use super::CliProgress;

pub fn run(drive_name: Option<&str>, prefix: Option<&str>) -> Result<(), TuckError> {
    let config = Config::load()?;
    let drive = super::resolve_drive_interactive(&config, drive_name, prefix)?;

    super::check_pending(&drive.root_path)?;

    println!(
        "Verifying all archives on drive '{}'...\n",
        drive.name
    );

    let progress = CliProgress::new();
    let results = verify::verify_all(&drive, Some(&progress))?;

    if results.is_empty() {
        println!("No archived entries to verify.");
        return Ok(());
    }

    let mut all_ok = true;

    for result in &results {
        if result.is_ok() {
            println!(
                "  {} {} ({}/{} files passed)",
                "PASS".green().bold(),
                result.original_path.display(),
                result.passed,
                result.total_files
            );
        } else {
            all_ok = false;
            println!(
                "  {} {} ({} failures)",
                "FAIL".red().bold(),
                result.original_path.display(),
                result.failed.len()
            );
            for f in &result.failed {
                let file_desc = if f.relative_path.is_empty() {
                    "(root file)".to_string()
                } else {
                    f.relative_path.clone()
                };
                println!(
                    "       {} expected {}, got {}",
                    file_desc,
                    f.expected.dimmed(),
                    f.actual.red()
                );
            }
        }
    }

    println!();
    if all_ok {
        println!("{}", "All checksums verified successfully.".green().bold());
        Ok(())
    } else {
        Err(TuckError::ChecksumMismatch {
            path: "multiple files".into(),
            expected: "matching checksums".to_string(),
            actual: "mismatches found".to_string(),
        })
    }
}
