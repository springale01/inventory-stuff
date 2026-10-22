#![doc = include_str!("../README.md")]

use std::sync::Arc;

use arc_swap::{ArcSwap, Guard};

use crate::{
    database::DatabaseSettings,
    langs::{Language, LanguagePack},
};

pub mod database;
pub mod langs;

/// Shared settings with atomic language replacement.
pub struct Settings {
    language: ArcSwap<LanguagePack>,
    database: DatabaseSettings,
}

impl Settings {
    pub fn new(pack: Option<LanguagePack>, database: DatabaseSettings) -> Self {
        let pack = pack.unwrap_or_default();

        Self {
            language: ArcSwap::from_pointee(pack),
            database,
        }
    }

    pub fn swap_lang<T: Language>(&self, pack: LanguagePack<T>) {
        self.language.store(Arc::new(pack.into_runtime()));
    }

    /// Returns a snapshot that remains valid if another thread swaps languages.
    pub fn lang(&self) -> Guard<Arc<LanguagePack>> {
        self.language.load()
    }

    pub fn database_settings(&self) -> &DatabaseSettings {
        &self.database
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new(None, DatabaseSettings::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langs::{English, EnglishDictionary, HashMapDictionary, TextKey};

    #[test]
    fn switching_languages_preserves_existing_snapshots_and_database_settings() {
        let database = DatabaseSettings {
            url: "sqlite::memory:".into(),
            max_connections: 1,
        };
        let settings = Settings::new(None, database);
        let before = settings.lang();
        let custom = HashMapDictionary::new([(TextKey::TuiName, "Custom inventory".into())]);
        settings.swap_lang(custom.into_language_pack());
        assert_eq!(before.get(TextKey::TuiName), Some("Inventory Manager"));
        assert_eq!(
            settings.lang().get(TextKey::TuiName),
            Some("Custom inventory")
        );
        let english: LanguagePack<English> = EnglishDictionary::new().into();
        settings.swap_lang(english);
        assert_eq!(
            settings.lang().get(TextKey::TuiName),
            Some("Inventory Manager")
        );
        assert_eq!(settings.database_settings().url, "sqlite::memory:");
        assert_eq!(settings.database_settings().max_connections, 1);
    }

    #[test]
    fn settings_can_be_shared_across_threads() {
        let settings = Arc::new(Settings::default());
        let shared = Arc::clone(&settings);
        std::thread::spawn(move || {
            shared.swap_lang(HashMapDictionary::default().into_language_pack());
            assert_eq!(
                shared.lang().get(TextKey::TuiName),
                Some("Inventory Manager")
            );
        })
        .join()
        .unwrap();
        assert_eq!(settings.database_settings().max_connections, 25);
    }
}
