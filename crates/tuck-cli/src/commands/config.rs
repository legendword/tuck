use colored::Colorize;
use tuck_core::config::Config;
use tuck_core::error::TuckError;

use crate::ConfigAction;

pub fn run(action: ConfigAction) -> Result<(), TuckError> {
    match action {
        ConfigAction::Show => {
            let config = Config::load()?;
            println!("{} {}", "Config file:".bold(), Config::path().display());
            println!(
                "  default_prefix: {}",
                config
                    .default_prefix
                    .as_deref()
                    .unwrap_or("(not set)")
            );
            println!(
                "  default_drive:  {}",
                config
                    .default_drive
                    .as_deref()
                    .unwrap_or("(not set)")
            );
            Ok(())
        }
        ConfigAction::SetPrefix { value } => {
            let mut config = Config::load()?;
            if value.is_empty() {
                config.default_prefix = None;
                println!("Cleared default prefix.");
            } else {
                config.default_prefix = Some(value.clone());
                println!("Set default prefix to '{}'.", value);
            }
            config.save()?;
            Ok(())
        }
        ConfigAction::SetDrive { value } => {
            let mut config = Config::load()?;
            if value.is_empty() {
                config.default_drive = None;
                println!("Cleared default drive.");
            } else {
                config.default_drive = Some(value.clone());
                println!("Set default drive to '{}'.", value);
            }
            config.save()?;
            Ok(())
        }
    }
}
