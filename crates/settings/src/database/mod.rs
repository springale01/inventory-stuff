use serde::{Deserialize, Serialize};

/// Connection configuration. Persistence owns pool creation and migrations.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseSettings {
    pub url: String,
    pub max_connections: u32,
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        Self {
            url: "sqlite://database/sqlite.db".into(),
            max_connections: 25,
        }
    }
}
