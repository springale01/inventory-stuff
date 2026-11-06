use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{SettingsIoError, language_reader::LanguageIO, text_file::TextFile};
use crate::{
    Settings,
    database::DatabaseSettings,
    langs::{LanguagePack, LanguageSource},
    runtime::RuntimeSettings,
};

pub struct SettingsIO {
    file: TextFile,
}

impl SettingsIO {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            file: TextFile::new(path.unwrap_or_else(|| PathBuf::from("settings.toml"))),
        }
    }

    pub async fn load(&self) -> Result<SettingsModel, SettingsIoError> {
        let text = match self.file.load_file().await {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SettingsModel::default());
            }
            Err(source) => {
                return Err(SettingsIoError::Io {
                    path: self.file.path().to_owned(),
                    source,
                });
            }
        };
        let model: SettingsModel =
            toml::from_str(&text).map_err(|source| SettingsIoError::Deserialize {
                path: self.file.path().to_owned(),
                source,
            })?;
        model.validate()?;
        Ok(model)
    }

    pub async fn save(&self, model: &SettingsModel) -> Result<(), SettingsIoError> {
        model.validate()?;
        let text = toml::to_string_pretty(model)?;
        self.file
            .save(text)
            .await
            .map_err(|source| SettingsIoError::Io {
                path: self.file.path().to_owned(),
                source,
            })
    }

    /// Relative dictionary paths are resolved against the settings directory.
    /// The original path remains in the model for subsequent saves.
    pub async fn load_language(
        &self,
        source: &LanguageSource,
    ) -> Result<LanguagePack, SettingsIoError> {
        match source {
            LanguageSource::BuiltInEnglish => Ok(LanguagePack::default()),
            LanguageSource::File { path } => {
                if path.as_os_str().is_empty() {
                    return Err(SettingsIoError::Invalid(
                        "dictionary path must not be empty",
                    ));
                }
                let base = self.file.path().parent().unwrap_or_else(|| Path::new("."));
                LanguageIO::new(base.join(path)).load().await
            }
        }
    }

    /// Construction is fallible and async because custom languages require IO.
    pub async fn build_settings(&self, model: &SettingsModel) -> Result<Settings, SettingsIoError> {
        model.validate()?;
        let pack = self.load_language(&model.language).await?;
        Ok(Settings::new(
            Some(pack),
            model.database_settings.clone(),
            model.runtime_settings.clone(),
        ))
    }
}

/// Persisted configuration; keep this model when saving, rather than deriving a
/// language source from the runtime pack's display name.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsModel {
    #[serde(alias = "language_pack", deserialize_with = "deserialize_language")]
    pub language: LanguageSource,
    pub runtime_settings: RuntimeSettings,
    pub database_settings: DatabaseSettings,
}

impl SettingsModel {
    pub fn validate(&self) -> Result<(), SettingsIoError> {
        if self.database_settings.max_connections == 0 {
            return Err(SettingsIoError::Invalid(
                "max_connections must be greater than zero",
            ));
        }
        if self.database_settings.url.trim().is_empty() {
            return Err(SettingsIoError::Invalid("database URL must not be empty"));
        }
        if let LanguageSource::File { path } = &self.language {
            if path.as_os_str().is_empty() {
                return Err(SettingsIoError::Invalid(
                    "dictionary path must not be empty",
                ));
            }
        }
        Ok(())
    }
}

// Read the earlier `language_pack = "English"` format without silently accepting
// misspellings or discarding unrelated configuration. Saves use the source table.
fn deserialize_language<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<LanguageSource, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Selection {
        Source(LanguageSource),
        Legacy(String),
    }
    match Selection::deserialize(deserializer)? {
        Selection::Source(source) => Ok(source),
        Selection::Legacy(name) if name.trim().eq_ignore_ascii_case("english") => {
            Ok(LanguageSource::BuiltInEnglish)
        }
        Selection::Legacy(_) => Err(serde::de::Error::custom(
            "unknown language; use a built-in or file source",
        )),
    }
}
