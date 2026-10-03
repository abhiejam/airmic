use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// The daemon's `config.toml`, also served as `Settings` over IPC (docs/ipc.md §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub set_default_source: bool,
    pub control_port: u16,
    pub audio_port: u16,
    pub transcription: Transcription,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Transcription {
    pub enabled: bool,
    pub model: TranscriptionModel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum TranscriptionModel {
    #[default]
    #[serde(rename = "base.en")]
    BaseEn,
    #[serde(rename = "small.en")]
    SmallEn,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            set_default_source: true,
            control_port: airmic_proto::CONTROL_PORT,
            audio_port: airmic_proto::AUDIO_PORT,
            transcription: Transcription::default(),
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

    /// Writes the config file through a temp file and a rename, so a crash never leaves it half written.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string(self)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
    }
}
