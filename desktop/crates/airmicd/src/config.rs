use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub control_port: u16,
    pub audio_port: u16,
    pub set_default_source: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            control_port: airmic_proto::CONTROL_PORT,
            audio_port: airmic_proto::AUDIO_PORT,
            set_default_source: true,
        }
    }
}

/// Returns `~/.config/airmic`.
pub fn config_dir() -> anyhow::Result<PathBuf> {
    directories::ProjectDirs::from("", "", "airmic")
        .map(|d| d.config_dir().to_path_buf())
        .context("no home directory")
}

impl Config {
    /// Loads the config file, or the defaults when it does not exist.
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }
}
