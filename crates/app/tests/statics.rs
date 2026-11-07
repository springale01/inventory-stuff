use app::settings::{DatabaseSettings, SettingsIO, SettingsModel};
use app::{PineappleState, StaticsError, set_statics, shutdown_statics};

#[test]
fn sync_globals_report_errors_share_handles_and_drain_on_shutdown() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "inventory-global-sync-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("settings.toml");
    let runtime = PineappleState::runtime();
    assert!(matches!(
        runtime.block_on(PineappleState::state()),
        Err(StaticsError::NotInitialized)
    ));
    assert!(matches!(
        shutdown_statics(),
        Err(StaticsError::NotInitialized)
    ));
    std::fs::write(&path, "[database_settings]\nmax_connections = 0").unwrap();
    assert!(matches!(
        set_statics(Some(path.clone())),
        Err(StaticsError::AppStateError(_))
    ));
    let model = SettingsModel {
        database_settings: DatabaseSettings {
            url: "sqlite::memory:".into(),
            max_connections: 1,
        },
        ..Default::default()
    };
    runtime
        .block_on(SettingsIO::new(Some(path.clone())).save(&model))
        .unwrap();
    set_statics(Some(path.clone())).unwrap();
    assert!(matches!(
        set_statics(None),
        Err(StaticsError::AlreadyInitialized)
    ));

    let (inventory, settings_handle, settings) = runtime.block_on(async {
        assert!(matches!(set_statics(None), Err(StaticsError::AsyncContext)));
        assert!(matches!(
            shutdown_statics(),
            Err(StaticsError::AsyncContext)
        ));
        let state = PineappleState::state().await.unwrap();
        assert_eq!(state.settings().database_settings().max_connections, 1);
        let expected = state.settings();
        drop(state);
        let settings = PineappleState::settings().await.unwrap();
        assert!(std::sync::Arc::ptr_eq(&expected, &settings));
        (
            PineappleState::inventory_handle().await.unwrap(),
            PineappleState::settings_handle().await.unwrap(),
            settings,
        )
    });
    shutdown_statics().unwrap();
    runtime.block_on(async {
        assert!(matches!(
            PineappleState::state().await,
            Err(StaticsError::NoneAppState)
        ));
        assert!(matches!(
            inventory.get_inventory(app::domain::Id::new()).await,
            Err(app::AppError::WorkerUnavailable {
                worker_name: "inventory"
            })
        ));
        assert!(matches!(
            settings_handle.model().await,
            Err(app::SettingsError::WorkerUnavailable)
        ));
    });
    assert_eq!(settings.lang().name(), "english");
    assert!(matches!(
        shutdown_statics(),
        Err(StaticsError::NoneAppState)
    ));
    assert!(matches!(
        set_statics(Some(path)),
        Err(StaticsError::AlreadyInitialized)
    ));
    std::fs::remove_dir_all(directory).unwrap();
}
