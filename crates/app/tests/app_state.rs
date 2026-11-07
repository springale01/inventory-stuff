use app::domain::{Id, Item, Sku, StockAmount, StockError, StockUnit};
use app::settings::{
    DatabaseSettings, LanguageSource, SettingsIO, SettingsModel, TextFile, TextKey,
};
use app::{AppError, AppState, InventoryError};
use persistance::{PineappleDatabase, TrackingMode};
use std::{path::PathBuf, time::Duration};
use tokio::time::timeout;

fn database_settings() -> DatabaseSettings {
    DatabaseSettings {
        url: "sqlite::memory:".into(),
        max_connections: 1,
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "inventory-app-state-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    async fn settings_path(&self) -> PathBuf {
        let path = self.0.join("settings.toml");
        let model = SettingsModel {
            database_settings: database_settings(),
            ..Default::default()
        };
        SettingsIO::new(Some(path.clone()))
            .save(&model)
            .await
            .unwrap();
        path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn public_use_cases_persist_balances_and_preserve_inventory_errors() {
    let directory = TestDirectory::new();
    let db = PineappleDatabase::new(&database_settings()).await.unwrap();
    let item = Item {
        id: Id::new(),
        sku: Sku::new("BOLTS").unwrap(),
        name: "Bolts".into(),
        description: None,
    };
    db.create_item(&item, TrackingMode::Bulk, 2).await.unwrap();
    let state = AppState::from_database(db.clone(), Some(directory.settings_path().await))
        .await
        .unwrap();
    let inventory_handle = state.inventory_handle().clone();

    let found = inventory_handle
        .get_item_by_sku(item.sku.clone())
        .await
        .unwrap();
    assert_eq!(found.id, item.id);
    assert_eq!(
        *state
            .add_stock(found.id, 10, "Delivery")
            .await
            .unwrap()
            .value(),
        StockAmount::Count(10)
    );
    assert_eq!(
        *state
            .remove_stock(found.id, 3, "Sale")
            .await
            .unwrap()
            .value(),
        StockAmount::Count(7)
    );
    assert!(matches!(
        state.remove_stock(found.id, 8, "Too much").await,
        Err(AppError::Inventory(InventoryError::Stock(
            StockError::InsufficientStock { .. }
        )))
    ));
    let snapshot = state.get_inventory(found.id).await.unwrap();
    assert_eq!(snapshot.stock.value().quantity(), StockAmount::Count(7));
    assert!(snapshot.units.is_none());
    assert!(matches!(
        state.get_item_by_sku(Sku::new("MISSING").unwrap()).await,
        Err(AppError::Inventory(InventoryError::Persistence(
            persistance::InventoryError::NotFound("sku")
        )))
    ));
    assert!(matches!(
        state.get_inventory(Id::new()).await,
        Err(AppError::Inventory(InventoryError::Persistence(
            persistance::InventoryError::NotFound("stock")
        )))
    ));
    // Previously returned snapshots retain their balance after another movement.
    state.add_stock(found.id, 1, "Return").await.unwrap();
    assert_eq!(snapshot.stock.value().quantity(), StockAmount::Count(7));

    let individual = Item {
        id: Id::new(),
        sku: Sku::new("SERIALIZED").unwrap(),
        ..item
    };
    db.create_item(&individual, TrackingMode::Individual, 0)
        .await
        .unwrap();
    let unit = StockUnit {
        id: Id::new(),
        serial_number: Some("SN-1".into()),
    };
    db.receive_units(individual.id, std::slice::from_ref(&unit))
        .await
        .unwrap();
    let inventory = state.get_inventory(individual.id).await.unwrap();
    assert_eq!(inventory.stock.value().quantity(), StockAmount::Count(1));
    assert_eq!(inventory.units.unwrap()[0].id, unit.id);

    timeout(Duration::from_secs(5), state.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        db.get_inventory(found.id)
            .await
            .unwrap()
            .unwrap()
            .stock
            .quantity(),
        StockAmount::Count(8)
    );
    assert_eq!(db.list_movements(found.id, 10, 0).await.unwrap().len(), 3);
    assert!(matches!(
        inventory_handle.get_inventory(found.id).await,
        Err(AppError::WorkerUnavailable {
            worker_name: "inventory"
        })
    ));
}

#[tokio::test]
async fn configured_startup_migrates_database_and_reports_initialization_errors() {
    let directory = TestDirectory::new();
    let path = directory.settings_path().await;
    let state = AppState::new(Some(path.clone())).await.unwrap();
    assert!(matches!(
        state.get_inventory(Id::new()).await,
        Err(AppError::Inventory(InventoryError::Persistence(
            persistance::InventoryError::NotFound("stock")
        )))
    ));
    state.shutdown().await.unwrap();

    TextFile::new(&path)
        .save("[database_settings]\nmax_connections = 0")
        .await
        .unwrap();
    assert!(matches!(
        AppState::new(Some(path.clone())).await,
        Err(AppError::Settings(app::SettingsError::Configuration(_)))
    ));

    let mut model = SettingsModel::default();
    model.database_settings.url = "sqlite::memory:?unknown_option=true".into();
    SettingsIO::new(Some(path.clone()))
        .save(&model)
        .await
        .unwrap();
    assert!(matches!(
        AppState::new(Some(path)).await,
        Err(AppError::Database(_))
    ));
}

#[tokio::test]
async fn app_settings_are_shared_persisted_and_shutdown_closes_cloned_handles() {
    let directory = TestDirectory::new();
    let path = directory.settings_path().await;
    let io = SettingsIO::new(Some(path.clone()));
    let dictionary = TextFile::new(directory.0.join("languages/custom.toml"));
    dictionary
        .save("name = 'Custom'\n[entries]\ntui_name = 'Configured'")
        .await
        .unwrap();
    let mut model = io.load().await.unwrap();
    model.language = LanguageSource::File {
        path: "languages/custom.toml".into(),
    };
    model.runtime_settings = settings::runtime::RuntimeSettings::new(2.try_into().unwrap());
    io.save(&model).await.unwrap();

    let state = AppState::new(Some(path.clone())).await.unwrap();
    let shared = state.settings();
    let handle = state.settings_handle().clone();
    assert!(std::sync::Arc::ptr_eq(&shared, &handle.settings()));
    assert_eq!(shared.lang().get(TextKey::TuiName), Some("Configured"));
    assert_eq!(shared.database_settings(), &database_settings());
    assert_eq!(shared.runtime_settings().mpsc_channel_capacity(), 2);
    dictionary
        .save("name = 'Custom'\n[entries]\ntui_name = 'Reloaded'")
        .await
        .unwrap();
    handle.reload_language().await.unwrap();
    assert_eq!(shared.lang().get(TextKey::TuiName), Some("Reloaded"));
    let old = shared.language_snapshot();
    handle
        .change_language(LanguageSource::BuiltInEnglish)
        .await
        .unwrap();
    assert_eq!(old.name(), "Custom");
    assert_eq!(shared.lang().name(), "english");
    model = handle.model().await.unwrap();
    model.database_settings.max_connections = 3;
    assert!(handle.update(model.clone()).await.unwrap());
    assert_eq!(shared.database_settings().max_connections, 1);
    assert_eq!(io.load().await.unwrap(), model);

    // Keeping this handle alive must not keep either worker's shutdown open.
    timeout(Duration::from_secs(5), state.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        handle.model().await,
        Err(app::SettingsError::WorkerUnavailable)
    ));
    assert_eq!(shared.lang().name(), "english");
    let restarted = AppState::new(Some(path)).await.unwrap();
    assert_eq!(restarted.settings().database_settings().max_connections, 3);
    restarted.shutdown().await.unwrap();
}
