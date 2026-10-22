mod english;
mod hashmap;
mod language;

pub use english::{English, EnglishDictionary};
pub use hashmap::{CustomLanguage, HashMapDictionary};
pub use language::{Dictionary, Language, LanguagePack, RuntimeLanguage};

/// Stable identifiers shared by every language dictionary.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextKey {
    TuiName,
}

impl TextKey {
    /// All supported keys. Add new keys here and to the English dictionary.
    pub const ALL: &'static [Self] = &[Self::TuiName];
}

#[cfg(test)]
mod tests;
