use std::collections::HashMap;

use crate::langs::language::Language;

use super::{Dictionary, LanguagePack, TextKey};

/// An owned dictionary for translations loaded or assembled at runtime.
/// Duplicate keys use the last supplied value.
#[derive(Debug, Clone, Default)]
pub struct HashMapDictionary {
    entries: HashMap<TextKey, String>,
}

impl HashMapDictionary {
    pub fn new(entries: impl IntoIterator<Item = (TextKey, String)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    pub fn into_language_pack(self) -> LanguagePack<CustomLanguage> {
        self.into()
    }
}

impl Dictionary for HashMapDictionary {
    fn items(&self) -> Box<dyn Iterator<Item = (TextKey, &str)> + Send + '_> {
        Box::new(self.entries.iter().map(|(&key, text)| (key, text.as_str())))
    }

    fn get(&self, key: TextKey) -> Option<&str> {
        self.entries.get(&key).map(String::as_str)
    }
}

impl From<HashMapDictionary> for LanguagePack<CustomLanguage> {
    fn from(value: HashMapDictionary) -> Self {
        Self::new(Box::new(value))
    }
}

pub struct CustomLanguage;
impl Language for CustomLanguage {}
