use std::{collections::HashMap, sync::LazyLock};

use crate::langs::{
    TextKey,
    language::{Dictionary, Language, LanguagePack},
};

pub static ENGLISH_DICT: LazyLock<HashMap<TextKey, &'static str>> = LazyLock::new(|| {
    let mut map = HashMap::new();

    map.insert(TextKey::TuiName, "Inventory Manager");

    map
});

#[derive(Debug, Clone, Copy, Default)]
pub struct EnglishDictionary;

impl EnglishDictionary {
    pub fn new() -> Self {
        Self
    }
}

impl Dictionary for EnglishDictionary {
    fn items(&self) -> Box<dyn Iterator<Item = (TextKey, &str)> + Send + '_> {
        Box::new(ENGLISH_DICT.iter().map(|(&key, &text)| (key, text)))
    }
    fn get(&self, key: TextKey) -> Option<&str> {
        ENGLISH_DICT.get(&key).copied()
    }
}

impl From<EnglishDictionary> for LanguagePack<English> {
    fn from(value: EnglishDictionary) -> Self {
        LanguagePack::new(Box::new(value))
    }
}

pub struct English;
impl Language for English {}
