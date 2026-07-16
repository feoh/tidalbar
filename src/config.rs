use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppConfig {
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub preferred_quality: QualityPreference,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityPreference {
    Low,
    High,
    Lossless,
    #[default]
    HiResLossless,
    DolbyAtmos,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine the platform configuration directory")]
    NoConfigDirectory,
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("could not create {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not encode configuration: {0}")]
    Encode(#[from] toml::ser::Error),
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        toml::from_str(&contents).map_err(|source| ConfigError::Parse { path, source })
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = config_path()?;
        let parent = path.parent().ok_or(ConfigError::NoConfigDirectory)?;
        fs::create_dir_all(parent).map_err(|source| ConfigError::CreateDirectory {
            path: parent.to_path_buf(),
            source,
        })?;
        let contents = toml::to_string_pretty(self)?;
        fs::write(&path, contents).map_err(|source| ConfigError::Write { path, source })
    }
}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    ProjectDirs::from("com", "feoh", "tidalbar")
        .map(|dirs| dirs.config_dir().join("config.toml"))
        .ok_or(ConfigError::NoConfigDirectory)
}

pub fn config_path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_as_toml() {
        let config = AppConfig {
            client_id: Some("public-client-id".to_owned()),
            redirect_uri: Some("http://127.0.0.1:47831/oauth/callback".to_owned()),
            preferred_quality: QualityPreference::Lossless,
        };

        let encoded = toml::to_string(&config).expect("config serializes");
        let decoded: AppConfig = toml::from_str(&encoded).expect("config parses");

        assert_eq!(decoded, config);
        assert!(!encoded.contains("secret"));
    }
}
