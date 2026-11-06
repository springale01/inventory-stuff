use std::{path::PathBuf, time::Duration};

use app::SettingsWorker;
use settings::{
    io::{settings_reader::SettingsIO, text_file::TextFile},
    langs::{LanguageSource, TextKey},
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "inventory-settings-worker-{}-{stamp}",
            std::process::id()
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
async fn changes_persist_reload_and_share_runtime_state_while_old_snapshots_survive() {
    let dir = TestDirectory::new();
    let path = dir.0.join("settings.toml");
    let dictionary = TextFile::new(dir.0.join("custom.toml"));
    dictionary
        .save("name = 'Custom'\n[entries]\ntui_name = 'First'")
        .await
        .unwrap();
    let (worker, handle) = SettingsWorker::new(Some(path.clone())).await.unwrap();
    let task = worker.summon();
    let shared = handle.settings();
    let other = handle.clone();
    assert!(std::sync::Arc::ptr_eq(&shared, &other.settings()));
    let old = shared.language_snapshot();
    let source = LanguageSource::File {
        path: "custom.toml".into(),
    };
    let first = handle.change_language(source.clone()).await.unwrap();
    assert_eq!(old.name(), "english");
    assert_eq!(shared.lang().get(TextKey::TuiName), Some("First"));
    assert_eq!(other.model().await.unwrap().language, source);
    assert_eq!(
        SettingsIO::new(Some(path.clone()))
            .load()
            .await
            .unwrap()
            .language,
        source
    );
    dictionary
        .save("name = 'Renamed'\n[entries]\ntui_name = 'Second'")
        .await
        .unwrap();
    other.reload_language().await.unwrap();
    assert_eq!(first.get(TextKey::TuiName), Some("First"));
    assert_eq!(shared.lang().get(TextKey::TuiName), Some("Second"));
    let mut candidate = handle.model().await.unwrap();
    candidate.database_settings.max_connections = 7;
    candidate.runtime_settings = settings::runtime::RuntimeSettings::new(3.try_into().unwrap());
    assert!(handle.update(candidate.clone()).await.unwrap());
    assert_eq!(shared.database_settings().max_connections, 25);
    assert_eq!(shared.runtime_settings().mpsc_channel_capacity(), 100);
    assert_eq!(handle.model().await.unwrap(), candidate);
    drop(handle);
    drop(other);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();

    let (worker, restarted) = SettingsWorker::new(Some(path)).await.unwrap();
    assert_eq!(restarted.settings().database_settings().max_connections, 7);
    assert_eq!(
        restarted
            .settings()
            .runtime_settings()
            .mpsc_channel_capacity(),
        3
    );
    assert_eq!(restarted.settings().lang().name(), "Renamed");
    drop(restarted);
    worker.summon().await.unwrap();
}

#[tokio::test]
async fn failed_load_reload_validation_and_save_leave_model_and_language_intact() {
    let dir = TestDirectory::new();
    let path = dir.0.join("settings.toml");
    let dictionary = TextFile::new(dir.0.join("custom.toml"));
    dictionary
        .save("name = 'Custom'\n[entries]\ntui_name = 'Keep'")
        .await
        .unwrap();
    let (worker, handle) = SettingsWorker::new(Some(path.clone())).await.unwrap();
    let task = worker.summon();
    handle
        .change_language(LanguageSource::File {
            path: "custom.toml".into(),
        })
        .await
        .unwrap();
    let before = handle.model().await.unwrap();
    let disk_before = std::fs::read_to_string(&path).unwrap();
    assert!(
        handle
            .change_language(LanguageSource::File {
                path: "missing.toml".into()
            })
            .await
            .is_err()
    );
    let mut invalid = before.clone();
    invalid.database_settings.max_connections = 0;
    assert!(handle.update(invalid).await.is_err());
    dictionary.save("broken [ TOML").await.unwrap();
    assert!(handle.reload_language().await.is_err());
    assert_eq!(handle.model().await.unwrap(), before);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), disk_before);
    assert_eq!(handle.settings().lang().get(TextKey::TuiName), Some("Keep"));

    // A directory at the destination forces replacement to fail on every platform.
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(
        handle
            .change_language(LanguageSource::BuiltInEnglish)
            .await
            .is_err()
    );
    assert_eq!(handle.model().await.unwrap(), before);
    assert_eq!(handle.settings().lang().get(TextKey::TuiName), Some("Keep"));
    drop(handle);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
}
