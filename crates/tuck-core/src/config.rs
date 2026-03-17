use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{IoContext, TuckResult};

const CONFIG_DIR: &str = ".config/tuck";
const CONFIG_FILENAME: &str = "config.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Default prefix subfolder to use on the drive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_prefix: Option<String>,

    /// Default drive name to use when not specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_drive: Option<String>,
}

impl Config {
    /// Path to the config file: ~/.config/tuck/config.json
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(CONFIG_DIR)
            .join(CONFIG_FILENAME)
    }

    /// Load config from disk. Returns default config if file doesn't exist.
    pub fn load() -> TuckResult<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path).io_context(&path)?;
        serde_json::from_str(&data).map_err(|e| crate::error::TuckError::Other(
            format!("Invalid config at {}: {}", path.display(), e),
        ))
    }

    /// Save config to disk, creating the directory if needed.
    pub fn save(&self) -> TuckResult<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).io_context(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| crate::error::TuckError::Other(e.to_string()))?;
        fs::write(&path, &data).io_context(&path)?;
        Ok(())
    }

    /// Resolve the effective prefix: CLI flag takes priority, then config default.
    pub fn resolve_prefix<'a>(&'a self, cli_prefix: Option<&'a str>) -> Option<&'a str> {
        cli_prefix.or(self.default_prefix.as_deref())
    }

    /// Resolve the effective drive name: CLI flag takes priority, then config default.
    pub fn resolve_drive_name<'a>(&'a self, cli_drive: Option<&'a str>) -> Option<&'a str> {
        cli_drive.or(self.default_drive.as_deref())
    }
}
