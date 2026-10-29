use sqlx::{SqlitePool, migrate::Migrator, sqlite::SqlitePoolOptions};

static MIGRATOR: Migrator = sqlx::migrate!("../../database/migrations");

async fn database() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    // Running the migrator again must leave an initialized database usable.
    MIGRATOR.run(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool
}

async fn execute(pool: &SqlitePool, sql: &'static str) {
    sqlx::query(sql).execute(pool).await.unwrap();
}

async fn rejects(pool: &SqlitePool, sql: &'static str) {
    let error = sqlx::query(sql).execute(pool).await.unwrap_err();
    assert!(
        error.as_database_error().is_some_and(|error| {
            error.is_check_violation()
                || error.is_unique_violation()
                || error.is_foreign_key_violation()
                // SQLite reports ON DELETE RESTRICT as SQLITE_CONSTRAINT_TRIGGER.
                || error.code().as_deref() == Some("1811")
        }),
        "expected a constraint violation: {error}"
    );
}

#[tokio::test]
async fn bulk_and_individual_stock_round_trip() {
    let pool = database().await;
    execute(&pool, "INSERT INTO items VALUES ('01900000-0000-7000-8000-000000000001', 'SCREW', 'Screws', NULL), ('01900000-0000-7000-8000-000000000002', 'LAPTOP', 'Laptop', 'Tracked individually')").await;
    execute(&pool, "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('01900000-0000-7000-8000-000000000001', 'bulk', 500, 100), ('01900000-0000-7000-8000-000000000002', 'individual', NULL, 2)").await;
    execute(&pool, "INSERT INTO stock_units (id, item_id, serial_number) VALUES ('01900000-0000-7000-8000-000000000003', '01900000-0000-7000-8000-000000000002', 'SN-1'), ('01900000-0000-7000-8000-000000000004', '01900000-0000-7000-8000-000000000002', NULL), ('01900000-0000-7000-8000-000000000005', '01900000-0000-7000-8000-000000000002', NULL)").await;
    let bulk: i64 = sqlx::query_scalar("SELECT quantity FROM stocks WHERE tracking_mode = 'bulk'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(bulk, 500);
    let units: i64 = sqlx::query_scalar("SELECT count(*) FROM stock_units")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(units, 3);
    rejects(&pool, "INSERT INTO stock_units (id, item_id) VALUES ('bulk-unit', '01900000-0000-7000-8000-000000000001')").await;
    rejects(&pool, "INSERT INTO stock_units (id, item_id, serial_number) VALUES ('duplicate-serial', '01900000-0000-7000-8000-000000000002', 'SN-1')").await;
    rejects(
        &pool,
        "UPDATE stocks SET tracking_mode = 'bulk', quantity = 3 WHERE tracking_mode = 'individual'",
    )
    .await;
    rejects(
        &pool,
        "DELETE FROM stocks WHERE tracking_mode = 'individual'",
    )
    .await;
    rejects(&pool, "DELETE FROM items WHERE sku = 'LAPTOP'").await;
    execute(&pool, "DELETE FROM stock_units").await;
    execute(&pool, "DELETE FROM stocks").await;
    execute(&pool, "DELETE FROM items").await;
}

#[tokio::test]
async fn invalid_stock_states_are_rejected() {
    let pool = database().await;
    execute(
        &pool,
        "INSERT INTO items VALUES ('item', 'SKU', 'Example', NULL)",
    )
    .await;
    for sql in [
        "INSERT INTO items VALUES ('duplicate', 'SKU', 'Duplicate', NULL)",
        "INSERT INTO items VALUES ('empty', ' ', 'Example', NULL)",
        "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('missing', 'bulk', 0, 0)",
        "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('item', 'bulk', -1, 0)",
        "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('item', 'bulk', NULL, 0)",
        "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('item', 'individual', 1, 0)",
        "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('item', 'bulk', 1, -1)",
        "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('item', 'unknown', 1, 0)",
    ] {
        rejects(&pool, sql).await;
    }
    execute(&pool, "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('item', 'bulk', 0, 0)").await;
    rejects(
        &pool,
        "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES ('item', 'individual', NULL, 0)",
    )
    .await;
}

#[tokio::test]
async fn measured_migration_preserves_populated_legacy_database_and_foreign_keys() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let legacy = Migrator::with_migrations(MIGRATOR.iter().take(1).cloned().collect());
    legacy.run(&pool).await.unwrap();
    execute(&pool, "INSERT INTO items VALUES ('bulk', 'BULK', 'Bulk', NULL), ('individual', 'INDIVIDUAL', 'Individual', NULL)").await;
    execute(&pool, "INSERT INTO stocks VALUES ('bulk', 'bulk', 9007199254740993, 12), ('individual', 'individual', NULL, 2)").await;
    execute(&pool, "INSERT INTO stock_units (id, item_id, serial_number) VALUES ('unit', 'individual', 'SERIAL')").await;
    MIGRATOR.run(&pool).await.unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT quantity FROM stocks WHERE item_id = 'bulk'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        9_007_199_254_740_993
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT serial_number FROM stock_units WHERE id = 'unit'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "SERIAL"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    rejects(&pool, "DELETE FROM stocks WHERE item_id = 'individual'").await;
    rejects(
        &pool,
        "INSERT INTO stock_units (id, item_id) VALUES ('bad-unit', 'bulk')",
    )
    .await;
    rejects(&pool, "INSERT INTO stock_units (id, item_id, serial_number) VALUES ('duplicate', 'individual', 'SERIAL')").await;
}

#[tokio::test]
async fn mass_storage_rejects_mixed_measurements_zero_piece_weight_and_infinity() {
    let pool = database().await;
    execute(
        &pool,
        "INSERT INTO items VALUES ('mass', 'MASS', 'Mass stock', NULL)",
    )
    .await;
    execute(&pool, "INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at, measurement, quantity_grams, low_stock_at_grams, mass_per_piece) VALUES ('mass', 'bulk', NULL, NULL, 'mass', 1000.5, 100.0, 2.5)").await;
    for sql in [
        "UPDATE stocks SET quantity = 1 WHERE item_id = 'mass'",
        "UPDATE stocks SET low_stock_at = 1 WHERE item_id = 'mass'",
        "UPDATE stocks SET quantity_grams = NULL WHERE item_id = 'mass'",
        "UPDATE stocks SET low_stock_at_grams = NULL WHERE item_id = 'mass'",
        "UPDATE stocks SET mass_per_piece = 0 WHERE item_id = 'mass'",
        "UPDATE stocks SET quantity_grams = -1 WHERE item_id = 'mass'",
        "UPDATE stocks SET quantity_grams = 1e999 WHERE item_id = 'mass'",
        "UPDATE stocks SET mass_per_piece = 1e999 WHERE item_id = 'mass'",
        "UPDATE stocks SET tracking_mode = 'individual' WHERE item_id = 'mass'",
    ] {
        rejects(&pool, sql).await;
    }
}
