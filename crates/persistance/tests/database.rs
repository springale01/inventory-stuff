use std::path::PathBuf;

use persistance::{DatabaseError, PineappleDatabase};
use settings::database::DatabaseSettings;
use uuid::Uuid;

fn temporary_root() -> PathBuf {
    std::env::temp_dir().join(format!("pineapple-database-{}", Uuid::now_v7()))
}

fn settings(path: &std::path::Path) -> DatabaseSettings {
    DatabaseSettings {
        url: format!("sqlite://{}", path.display()),
        max_connections: 2,
    }
}

#[tokio::test]
async fn startup_creates_migrates_and_reopens_a_database() {
    let root = temporary_root();
    let path = root.join("nested").join("inventory.db");
    let config = settings(&path);
    let db = PineappleDatabase::new(&config).await.unwrap();
    assert!(path.is_file());
    sqlx::query("INSERT INTO items VALUES ('item', 'SKU', 'Example', NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('item', 'bulk', 10, 2)")
        .execute(db.pool())
        .await
        .unwrap();

    // All statements in a failed/abandoned transaction must roll back together.
    let mut transaction = db.pool().begin().await.unwrap();
    sqlx::query("UPDATE stocks SET quantity = 20 WHERE item_id = 'item'")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    db.close().await;

    let reopened = PineappleDatabase::new(&config).await.unwrap();
    let quantity: i64 = sqlx::query_scalar("SELECT quantity FROM stocks WHERE item_id = 'item'")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert_eq!(quantity, 10);
    let migrations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(reopened.pool())
            .await
            .unwrap();
    assert_eq!(migrations, 2);
    let shared = reopened.clone();
    reopened.close().await;
    assert!(shared.pool().is_closed());

    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(path.parent().unwrap()).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[tokio::test]
async fn memory_database_shares_schema_and_enforces_foreign_keys_on_each_connection() {
    let db = PineappleDatabase::new(&DatabaseSettings {
        url: "sqlite::memory:".into(),
        max_connections: 2,
    })
    .await
    .unwrap();
    let mut first = db.pool().acquire().await.unwrap();
    let mut second = db.pool().acquire().await.unwrap();
    for connection in [&mut first, &mut second] {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let error = sqlx::query("INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('missing', 'bulk', 0, 0)")
            .execute(&mut **connection)
            .await
            .unwrap_err();
        assert!(
            error
                .as_database_error()
                .unwrap()
                .is_foreign_key_violation()
        );
    }
    drop(first);
    drop(second);
    db.close().await;
}

#[tokio::test]
async fn invalid_configuration_returns_errors() {
    let error = PineappleDatabase::new(&DatabaseSettings {
        url: "sqlite::memory:".into(),
        max_connections: 0,
    })
    .await
    .unwrap_err();
    assert!(matches!(error, DatabaseError::InvalidMaxConnections));
    let error = PineappleDatabase::new(&DatabaseSettings {
        url: "sqlite://ignored.db?mode=invalid".into(),
        max_connections: 1,
    })
    .await
    .unwrap_err();
    assert!(matches!(error, DatabaseError::Connection(_)));
}

#[tokio::test]
async fn directory_creation_failure_is_reported() {
    let path = temporary_root();
    std::fs::write(&path, "a file cannot be a directory").unwrap();
    let error = PineappleDatabase::new(&settings(&path.join("inventory.db")))
        .await
        .unwrap_err();
    assert!(matches!(error, DatabaseError::CreateDirectory { .. }));
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn migration_failure_is_reported_without_destroying_existing_data() {
    let root = temporary_root();
    std::fs::create_dir(&root).unwrap();
    let path = root.join("inventory.db");
    let config = settings(&path);
    let options = config
        .url
        .parse::<sqlx::sqlite::SqliteConnectOptions>()
        .unwrap()
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("CREATE TABLE items (original TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO items VALUES ('keep me')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let error = PineappleDatabase::new(&config).await.unwrap_err();
    assert!(matches!(error, DatabaseError::Migration(_)));
    let pool = sqlx::SqlitePool::connect(&config.url).await.unwrap();
    let original: String = sqlx::query_scalar("SELECT original FROM items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(original, "keep me");
    pool.close().await;
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}
