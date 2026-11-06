use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::{SettingsIoError, text_file::TextFile};
use crate::langs::{HashMapDictionary, LanguagePack, TextKey};

/// On-disk dictionary format. Missing translations use the pack's English fallback.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageModel {
    pub name: String,
    pub entries: HashMap<TextKey, String>,
}

pub struct LanguageIO {
    file: TextFile,
}

impl LanguageIO {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            file: TextFile::new(path),
        }
    }

    pub async fn load(&self) -> Result<LanguagePack, SettingsIoError> {
        let text = self
            .file
            .load_file()
            .await
            .map_err(|source| SettingsIoError::Io {
                path: self.file.path().to_owned(),
                source,
            })?;
        let model: LanguageModel =
            toml::from_str(&text).map_err(|source| SettingsIoError::Deserialize {
                path: self.file.path().to_owned(),
                source,
            })?;
        if model.name.trim().is_empty() {
            return Err(SettingsIoError::Invalid(
                "language display name must not be empty",
            ));
        }
        Ok(HashMapDictionary::new(model.entries, model.name)
            .into_language_pack()
            .into_runtime())
    }
}
