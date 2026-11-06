use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Persistable language selection, independent of its dictionary's display name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LanguageSource {
    #[default]
    BuiltInEnglish,
    File {
        path: PathBuf,
    },
}
