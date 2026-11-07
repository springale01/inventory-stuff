#![doc = include_str!("../README.md")]

use std::sync::Arc;

use arc_swap::{ArcSwap, Guard};

pub mod database;
pub mod io;
pub mod langs;
pub mod runtime;

pub use database::DatabaseSettings;
pub use io::{LanguageIO, LanguageModel, SettingsIO, SettingsIoError, SettingsModel, TextFile};
pub use langs::{
    CustomLanguage, Dictionary, English, EnglishDictionary, HashMapDictionary, Language,
    LanguagePack, LanguageSource, RuntimeLanguage, TextKey,
};
pub use runtime::RuntimeSettings;

/// Shared settings with atomic language replacement.
pub struct Settings {
    language: ArcSwap<LanguagePack>,
    database: DatabaseSettings,
    runtime: RuntimeSettings,
}

impl Settings {
    pub fn new(
        pack: Option<LanguagePack>,
        database: DatabaseSettings,
        runtime: RuntimeSettings,
    ) -> Self {
        let pack = pack.unwrap_or_default();

        Self {
            language: ArcSwap::from_pointee(pack),
            database,
            runtime,
        }
    }

    pub fn swap_lang<T: Language>(&self, pack: LanguagePack<T>) {
        self.language.store(Arc::new(pack.into_runtime()));
    }

    /// Returns a snapshot that remains valid if another thread swaps languages.
    pub fn lang(&self) -> Guard<Arc<LanguagePack>> {
        self.language.load()
    }

    /// An owned snapshot for responses or other long-lived consumers.
    /// Later language swaps do not change this snapshot.
    pub fn language_snapshot(&self) -> Arc<LanguagePack> {
        self.language.load_full()
    }

    pub fn database_settings(&self) -> &DatabaseSettings {
        &self.database
    }

    pub fn runtime_settings(&self) -> &RuntimeSettings {
        &self.runtime
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new(
            None,
            DatabaseSettings::default(),
            RuntimeSettings::default(),
        )
    }
}
