//! File-backed settings and language dictionaries.

pub mod language_reader;
pub mod settings_reader;
pub mod text_file;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SettingsIoError {
    #[error("Could not access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Invalid TOML in {path}: {source}")]
    Deserialize {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("Could not serialize settings: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("Invalid settings: {0}")]
    Invalid(&'static str),
}
