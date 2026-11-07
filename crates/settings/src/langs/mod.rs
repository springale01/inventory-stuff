mod english;
mod hashmap;
mod language;
mod source;

pub use english::{English, EnglishDictionary};
pub use hashmap::{CustomLanguage, HashMapDictionary};
pub use language::{Dictionary, Language, LanguagePack, RuntimeLanguage};
pub use source::LanguageSource;

/// Stable identifiers shared by every language dictionary.
#[derive(
    Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TextKey {
    TuiName,
    InventoryPageTitle,
    SettingsPageTitle,
}

impl TextKey {
    /// All supported keys. Add new keys here and to the English dictionary.
    pub const ALL: &'static [Self] = &[
        Self::TuiName,
        Self::InventoryPageTitle,
        Self::SettingsPageTitle,
    ];
}

#[cfg(test)]
mod tests;
