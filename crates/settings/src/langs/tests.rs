use std::collections::HashMap;

use super::*;

#[test]
fn english_covers_all_keys_and_iteration_matches_lookup() {
    let dictionary = EnglishDictionary::new();
    let entries: HashMap<_, _> = dictionary.items().collect();
    assert_eq!(entries.len(), TextKey::ALL.len());
    for &key in TextKey::ALL {
        let text = dictionary.get(key).expect("English must cover every key");
        assert!(!text.is_empty());
        assert_eq!(entries.get(&key).copied(), Some(text));
    }
}

#[test]
fn owned_dictionary_supports_dynamic_dispatch_and_borrowed_iteration() {
    let title = ["Gestion", "des stocks"].join(" ");
    let dictionary: Box<dyn Dictionary> = Box::new(HashMapDictionary::new([
        (TextKey::TuiName, "old title".to_owned()),
        (TextKey::TuiName, title.clone()),
    ]));
    assert_eq!(dictionary.get(TextKey::TuiName), Some(title.as_str()));
    assert_eq!(
        dictionary.items().collect::<Vec<_>>(),
        vec![(TextKey::TuiName, title.as_str())]
    );
    let pack: LanguagePack = LanguagePack::new(dictionary);
    assert_eq!(pack.get(TextKey::TuiName), Some(title.as_str()));
}

#[test]
fn missing_translation_falls_back_without_changing_raw_dictionary() {
    let pack = HashMapDictionary::default().into_language_pack();
    assert_eq!(pack.dict().get(TextKey::TuiName), None);
    assert_eq!(pack.dict().items().count(), 0);
    assert_eq!(pack.get(TextKey::TuiName), Some("Inventory Manager"));
}

#[test]
fn empty_translation_is_preserved() {
    let pack = HashMapDictionary::new([(TextKey::TuiName, String::new())]).into_language_pack();
    assert_eq!(pack.get(TextKey::TuiName), Some(""));
}

#[test]
fn typed_english_pack_and_runtime_default_use_english() {
    let english: LanguagePack<English> = EnglishDictionary::new().into();
    assert_eq!(english.get(TextKey::TuiName), Some("Inventory Manager"));
    assert_eq!(
        english.into_runtime().get(TextKey::TuiName),
        Some("Inventory Manager")
    );
    assert_eq!(
        LanguagePack::default().get(TextKey::TuiName),
        Some("Inventory Manager")
    );
}
