use std::marker::PhantomData;

use crate::langs::{
    TextKey,
    english::{ENGLISH_DICT, EnglishDictionary},
};

/// A thread-safe source of translations, including dictionaries with owned strings.
pub trait Dictionary: Send + Sync {
    /// Iterates this dictionary's entries in an unspecified order.
    fn items(&self) -> Box<dyn Iterator<Item = (TextKey, &str)> + Send + '_>;
    fn get(&self, key: TextKey) -> Option<&str>;
}

/// A selected dictionary with English fallback for missing keys.
pub struct LanguagePack<T = RuntimeLanguage>
where
    T: Language,
{
    dictionary: Box<dyn Dictionary>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> LanguagePack<T>
where
    T: Language,
{
    /// Accesses the selected dictionary directly, without fallback.
    pub fn dict(&self) -> &dyn Dictionary {
        self.dictionary.as_ref()
    }

    pub(super) fn new(dictionary: Box<dyn Dictionary>) -> Self {
        Self {
            dictionary,
            _marker: PhantomData,
        }
    }

    /// Removes the compile-time language marker for runtime language switching.
    pub fn into_runtime(self) -> LanguagePack {
        LanguagePack::new(self.dictionary)
    }

    /// Looks up a translation, falling back to English when it is absent.
    /// An empty translation is an intentional value and does not trigger fallback.
    pub fn get(&self, key: TextKey) -> Option<&str> {
        self.dictionary
            .get(key)
            .or_else(|| ENGLISH_DICT.get(&key).copied())
    }
}
/// A compile-time label; dictionary contents are not validated by this marker.
pub trait Language {}
/// A pack whose language is selected at runtime.
pub struct RuntimeLanguage;
impl Language for RuntimeLanguage {}

impl Default for LanguagePack {
    fn default() -> Self {
        Self::new(Box::new(EnglishDictionary::new()))
    }
}
