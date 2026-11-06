use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use settings::{
    io::{
        settings_reader::{SettingsIO, SettingsModel},
        text_file::TextFile,
    },
    langs::{LanguageSource, TextKey},
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "inventory-settings-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn missing_file_defaults_and_save_creates_parents_and_replaces_existing_file() {
    let dir = TestDirectory::new();
    let path = dir.0.join("nested/settings.toml");
    let io = SettingsIO::new(Some(path.clone()));
    let mut model = io.load().await.unwrap();
    assert_eq!(model, SettingsModel::default());
    assert!(!path.exists());
    model.database_settings.url = "sqlite://my-custom.db".into();
    io.save(&model).await.unwrap();
    assert_eq!(io.load().await.unwrap(), model);
    model.database_settings.max_connections = 3;
    io.save(&model).await.unwrap();
    assert_eq!(io.load().await.unwrap(), model);
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
}

#[tokio::test]
async fn relative_language_source_round_trips_independently_of_display_name() {
    let dir = TestDirectory::new();
    let path = dir.0.join("config/settings.toml");
    let dictionary = TextFile::new(dir.0.join("config/languages/pirate.toml"));
    dictionary
        .save("name = 'Pirate English'\n[entries]\ntui_name = 'Ye Inventory'\n")
        .await
        .unwrap();
    let io = SettingsIO::new(Some(path));
    let mut model = SettingsModel::default();
    model.database_settings.url = "sqlite://keep-me.db".into();
    model.language = LanguageSource::File {
        path: "languages/pirate.toml".into(),
    };
    io.save(&model).await.unwrap();
    let reloaded = io.load().await.unwrap();
    assert_eq!(reloaded, model);
    let settings = io.build_settings(&reloaded).await.unwrap();
    assert_eq!(settings.lang().name(), "Pirate English");
    assert_eq!(settings.lang().get(TextKey::TuiName), Some("Ye Inventory"));
    assert_eq!(settings.database_settings().url, "sqlite://keep-me.db");

    let old = settings.language_snapshot();
    settings.swap_lang(settings::langs::LanguagePack::default());
    assert_eq!(old.name(), "Pirate English");
    assert_eq!(settings.lang().name(), "english");
}

#[tokio::test]
async fn legacy_english_and_partial_tables_preserve_custom_configuration() {
    let dir = TestDirectory::new();
    let path = dir.0.join("settings.toml");
    TextFile::new(&path).save("language_pack = ' ENGLISH '\n[database_settings]\nurl = 'sqlite://custom.db'\n[runtime_settings]\n").await.unwrap();
    let io = SettingsIO::new(Some(path));
    let model = io.load().await.unwrap();
    assert_eq!(model.language, LanguageSource::BuiltInEnglish);
    assert_eq!(model.database_settings.max_connections, 25);
    let settings = io.build_settings(&model).await.unwrap();
    assert_eq!(settings.database_settings().url, "sqlite://custom.db");
    assert_eq!(settings.runtime_settings().mpsc_channel_capacity(), 100);
}

#[tokio::test]
async fn invalid_configuration_is_reported_without_overwriting_it() {
    let dir = TestDirectory::new();
    let path = dir.0.join("settings.toml");
    let io = SettingsIO::new(Some(path.clone()));
    let file = TextFile::new(path);
    for invalid in [
        "language_pack = 'Englsh'",
        "[database_settings]\nmax_connections = 0",
        "[runtime_settings]\nmpsc_channel_capacity = 0",
        "[database_settings]\nurl = ''",
        "[language]\nkind = 'file'\npath = ''",
        "language_pak = 'english'",
        "not valid TOML [",
    ] {
        file.save(invalid).await.unwrap();
        assert!(io.load().await.is_err(), "accepted: {invalid}");
        assert_eq!(file.load_file().await.unwrap(), invalid);
    }
    io.save(&SettingsModel::default()).await.unwrap();
    let before = file.load_file().await.unwrap();
    let mut invalid = SettingsModel::default();
    invalid.database_settings.max_connections = 0;
    assert!(io.save(&invalid).await.is_err());
    assert_eq!(file.load_file().await.unwrap(), before);
}

#[tokio::test]
async fn dictionaries_validate_keys_and_keep_english_fallback_and_empty_translations() {
    let dir = TestDirectory::new();
    let io = SettingsIO::new(Some(dir.0.join("settings.toml")));
    let source = LanguageSource::File {
        path: "language.toml".into(),
    };
    assert!(io.load_language(&source).await.is_err());
    let file = TextFile::new(dir.0.join("language.toml"));
    file.save("name = 'Partial'\n[entries]\n").await.unwrap();
    let pack = io.load_language(&source).await.unwrap();
    assert_eq!(pack.dict().get(TextKey::TuiName), None);
    assert_eq!(pack.get(TextKey::TuiName), Some("Inventory Manager"));
    file.save("name = 'Empty translation'\n[entries]\ntui_name = ''")
        .await
        .unwrap();
    assert_eq!(
        io.load_language(&source)
            .await
            .unwrap()
            .get(TextKey::TuiName),
        Some("")
    );
    for invalid in [
        "name = ' '\n[entries]",
        "name = 'Typo'\n[entries]\ntui_nam = 'Oops'",
    ] {
        file.save(invalid).await.unwrap();
        assert!(io.load_language(&source).await.is_err());
    }
}

#[tokio::test]
async fn failed_replacement_preserves_destination_and_cleans_temporary_file() {
    let dir = TestDirectory::new();
    let destination = dir.0.join("destination");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("sentinel"), "keep").unwrap();
    assert!(
        TextFile::new(&destination)
            .save("replacement")
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("sentinel")).unwrap(),
        "keep"
    );
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
}
