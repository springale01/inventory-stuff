use app::settings::{DatabaseSettings, SettingsIO, SettingsModel};
use app::{PineappleState, StaticsError, initialize};

#[tokio::test]
async fn concurrent_async_initialization_has_one_owner_and_shutdown_uses_the_callers_runtime() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "inventory-global-async-{}-{stamp}",
        std::process::id()
    ));
    let path = directory.join("settings.toml");
    let model = SettingsModel {
        database_settings: DatabaseSettings {
            url: "sqlite::memory:".into(),
            max_connections: 1,
        },
        ..Default::default()
    };
    SettingsIO::new(Some(path.clone()))
        .save(&model)
        .await
        .unwrap();
    let (first, second) = tokio::join!(initialize(Some(path.clone())), initialize(Some(path)));
    assert!(matches!(
        (&first, &second),
        (Ok(()), Err(StaticsError::AlreadyInitialized))
            | (Err(StaticsError::AlreadyInitialized), Ok(()))
    ));
    let handle = PineappleState::settings_handle().await.unwrap();
    assert_eq!(handle.model().await.unwrap(), model);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        PineappleState::shutdown_global(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(
        handle.model().await,
        Err(app::SettingsError::WorkerUnavailable)
    ));
    std::fs::remove_dir_all(directory).unwrap();
}
