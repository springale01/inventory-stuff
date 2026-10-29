use domain::{
    models::{
        sku::{Sku, SkuBuilder},
        stock::{Item, StockAmount, StockError, StockUnit},
    },
    tools::{
        id::Id,
        units::{Grams, Quantity},
    },
};
use persistance::{InventoryError, PageRequest, PineappleDatabase, TrackingMode};
use settings::database::DatabaseSettings;

async fn database() -> PineappleDatabase {
    PineappleDatabase::new(&DatabaseSettings {
        url: "sqlite::memory:".into(),
        max_connections: 1,
    })
    .await
    .unwrap()
}

fn item(sku: &str) -> Item {
    Item {
        id: Id::new(),
        sku: Sku::new(sku).unwrap(),
        name: "Example".into(),
        description: None,
    }
}

fn unit(serial: Option<&str>) -> StockUnit {
    StockUnit {
        id: Id::new(),
        serial_number: serial.map(str::to_owned),
    }
}

#[tokio::test]
async fn built_sku_round_trips_through_storage_and_lookup() {
    let db = database().await;
    let mut value = item("unused");
    value.sku = SkuBuilder::new()
        .add_field("GPU")
        .add_field("5090")
        .add_field("001")
        .finalize()
        .unwrap();
    db.create_item(&value, TrackingMode::Individual, 0)
        .await
        .unwrap();
    let loaded = db.get_item(value.id).await.unwrap().unwrap();
    assert_eq!(loaded.sku, value.sku);
    assert_eq!(
        db.get_item_by_sku(&loaded.sku).await.unwrap().unwrap().id,
        value.id
    );
}

#[tokio::test]
async fn numbered_pages_cover_all_items_in_sku_order() {
    let db = database().await;
    for sku in ["C", "A", "E", "B", "D"] {
        db.create_item(&item(sku), TrackingMode::Bulk, 0)
            .await
            .unwrap();
    }
    let mut page = db
        .list_items_page(PageRequest::new(1, 2).unwrap())
        .await
        .unwrap();
    assert_eq!(page.total_items, 5);
    assert_eq!(page.total_pages(), 3);
    assert!(page.previous_page().is_none());
    let mut skus = Vec::new();
    loop {
        skus.extend(page.items.iter().map(|item| item.sku.as_str().to_owned()));
        let Some(next) = page.next_page() else {
            break;
        };
        page = db.list_items_page(next).await.unwrap();
    }
    assert_eq!(skus, ["A", "B", "C", "D", "E"]);
    assert_eq!(page.request().number(), 3);
    assert_eq!(page.request().size(), 2);
    assert_eq!(page.items.len(), 1);
    let previous = db
        .list_items_page(page.previous_page().unwrap())
        .await
        .unwrap();
    assert_eq!(previous.items[0].sku.as_str(), "C");
    let beyond = db
        .list_items_page(PageRequest::new(4, 2).unwrap())
        .await
        .unwrap();
    assert!(beyond.items.is_empty());
    assert_eq!(beyond.total_items, 5);
    assert!(beyond.next_page().is_none());
    assert_eq!(beyond.previous_page().unwrap().number(), 3);
}

#[tokio::test]
async fn search_pages_count_only_matching_items_and_handle_empty_results() {
    let db = database().await;
    let empty = db.list_items_page(PageRequest::default()).await.unwrap();
    assert_eq!(empty.total_pages(), 0);
    assert_eq!(empty.total_items, 0);
    assert!(empty.next_page().is_none());
    assert!(empty.previous_page().is_none());
    for sku in ["A%", "B%", "C"] {
        db.create_item(&item(sku), TrackingMode::Bulk, 0)
            .await
            .unwrap();
    }
    let first = db
        .search_items_page("%", PageRequest::new(1, 1).unwrap())
        .await
        .unwrap();
    assert_eq!(first.total_items, 2);
    assert_eq!(first.total_pages(), 2);
    assert_eq!(first.items[0].sku.as_str(), "A%");
    let last = db
        .search_items_page("%", first.next_page().unwrap())
        .await
        .unwrap();
    assert_eq!(last.items[0].sku.as_str(), "B%");
    assert!(last.next_page().is_none());
    let none = db
        .search_items_page("missing", PageRequest::default())
        .await
        .unwrap();
    assert_eq!(none.total_items, 0);
    assert!(none.items.is_empty());
    assert_eq!(
        db.search_items_page("", PageRequest::default())
            .await
            .unwrap()
            .total_items,
        3
    );
}

#[test]
fn page_requests_validate_number_size_and_sqlite_offset_range() {
    assert!(PageRequest::new(0, 25).is_err());
    assert!(PageRequest::new(1, 0).is_err());
    assert!(PageRequest::new(1, PageRequest::MAX_SIZE + 1).is_err());
    assert!(PageRequest::new(u64::MAX, 200).is_err());
    assert!(PageRequest::new(i64::MAX as u64 + 1, 1).is_ok());
    assert!(PageRequest::new(i64::MAX as u64 + 2, 1).is_err());
    assert_eq!(PageRequest::default(), PageRequest::new(1, 25).unwrap());
}

#[tokio::test]
async fn item_crud_preserves_identity_and_handles_literal_input() {
    let db = database().await;
    let mut first = item("A' ; DROP TABLE items; --");
    let second = item("B");
    db.create_item(&first, TrackingMode::Bulk, 3).await.unwrap();
    db.create_item(&second, TrackingMode::Individual, 0)
        .await
        .unwrap();
    assert_eq!(
        db.get_item_by_sku(&first.sku).await.unwrap().unwrap().id,
        first.id
    );
    assert_eq!(db.list_items(1, 1).await.unwrap()[0].id, second.id);
    assert!(db.list_items(0, 0).await.unwrap().is_empty());
    assert!(db.list_items(10, 2).await.unwrap().is_empty());
    assert_eq!(
        db.search_items("DROP TABLE", 10, 0).await.unwrap()[0].id,
        first.id
    );
    assert!(db.search_items("%", 10, 0).await.unwrap().is_empty());

    first.name = "名前 ' updated".into();
    first.description = Some("a description".into());
    db.update_item(&first).await.unwrap();
    let loaded = db.get_item(first.id).await.unwrap().unwrap();
    assert_eq!(loaded.name, first.name);
    assert_eq!(loaded.description, first.description);
    let mut duplicate = item(first.sku.as_str());
    assert!(
        db.create_item(&duplicate, TrackingMode::Bulk, 0)
            .await
            .is_err()
    );
    assert!(db.get_item(duplicate.id).await.unwrap().is_none());
    duplicate = second.clone();
    duplicate.sku = first.sku.clone();
    assert!(db.update_item(&duplicate).await.is_err());
    assert_eq!(
        db.get_item(second.id).await.unwrap().unwrap().sku,
        second.sku
    );

    db.delete_item(first.id).await.unwrap();
    assert!(db.get_item(first.id).await.unwrap().is_none());
    assert!(db.get_stock(first.id).await.unwrap().is_none());
    assert!(matches!(
        db.delete_item(first.id).await,
        Err(InventoryError::NotFound(_))
    ));
    assert!(matches!(
        db.update_item(&first).await,
        Err(InventoryError::NotFound(_))
    ));
}

#[tokio::test]
async fn bulk_adjustments_reject_underflow_overflow_and_keep_previous_quantity() {
    let db = database().await;
    let value = item("BULK");
    db.create_item(&value, TrackingMode::Bulk, 2).await.unwrap();
    db.receive_bulk(value.id, 10).await.unwrap();
    db.remove_bulk(value.id, 3).await.unwrap();
    assert!(matches!(
        db.remove_bulk(value.id, 8).await,
        Err(InventoryError::Stock(StockError::InsufficientStock {
            available: StockAmount::Count(7),
            requested: StockAmount::Count(8)
        }))
    ));
    assert_eq!(
        db.get_stock(value.id).await.unwrap().unwrap().quantity,
        StockAmount::Count(7)
    );
    assert!(matches!(
        db.receive_bulk(value.id, 0).await,
        Err(InventoryError::Stock(StockError::ZeroQuantityAdjustment))
    ));
    assert!(matches!(
        db.delete_item(value.id).await,
        Err(InventoryError::StockNotEmpty)
    ));
    db.set_bulk_quantity(value.id, i64::MAX as u64)
        .await
        .unwrap();
    assert!(matches!(
        db.receive_bulk(value.id, 1).await,
        Err(InventoryError::QuantityOverflow)
    ));
    assert!(matches!(
        db.set_bulk_quantity(value.id, u64::MAX).await,
        Err(InventoryError::QuantityOverflow)
    ));
    assert_eq!(
        db.get_stock(value.id).await.unwrap().unwrap().quantity,
        StockAmount::Count(i64::MAX as u64)
    );
    db.set_bulk_quantity(value.id, 0).await.unwrap();
    assert!(db.get_stock(value.id).await.unwrap().unwrap().is_low());
    db.delete_item(value.id).await.unwrap();
}

#[tokio::test]
async fn units_are_atomic_and_removal_checks_item_ownership() {
    let db = database().await;
    let first = item("FIRST");
    let second = item("SECOND");
    for value in [&first, &second] {
        db.create_item(value, TrackingMode::Individual, 1)
            .await
            .unwrap();
    }
    let existing = unit(Some("SN-1"));
    db.receive_units(first.id, std::slice::from_ref(&existing))
        .await
        .unwrap();
    let new = unit(None);
    let duplicate_serial = unit(Some("SN-1"));
    assert!(
        db.receive_units(first.id, &[new.clone(), duplicate_serial])
            .await
            .is_err()
    );
    assert!(db.get_unit(first.id, new.id).await.unwrap().is_none());
    assert_eq!(db.list_units(first.id, 100, 0).await.unwrap().len(), 1);
    assert!(matches!(
        db.remove_unit(second.id, existing.id).await,
        Err(InventoryError::NotFound(_))
    ));
    assert!(db.get_unit(first.id, existing.id).await.unwrap().is_some());
    db.receive_units(first.id, &[new, unit(None)])
        .await
        .unwrap();
    assert_eq!(
        db.get_stock(first.id).await.unwrap().unwrap().quantity,
        StockAmount::Count(3)
    );
    assert_eq!(db.list_units(first.id, 1, 1).await.unwrap().len(), 1);
    assert!(matches!(
        db.delete_item(first.id).await,
        Err(InventoryError::StockNotEmpty)
    ));
    assert_eq!(
        db.remove_unit(first.id, existing.id)
            .await
            .unwrap()
            .serial_number
            .as_deref(),
        Some("SN-1")
    );
    assert!(db.get_unit(first.id, existing.id).await.unwrap().is_none());
    assert_eq!(
        db.get_stock(first.id).await.unwrap().unwrap().quantity,
        StockAmount::Count(2)
    );
}

#[tokio::test]
async fn low_stock_filters_both_tracking_modes_and_threshold_updates() {
    let db = database().await;
    let bulk = item("A");
    let individual = item("B");
    db.create_item(&bulk, TrackingMode::Bulk, 5).await.unwrap();
    db.create_item(&individual, TrackingMode::Individual, 1)
        .await
        .unwrap();
    db.receive_bulk(bulk.id, 6).await.unwrap();
    db.receive_units(individual.id, &[unit(None)])
        .await
        .unwrap();
    assert_eq!(
        db.list_stock(true, 100, 0).await.unwrap()[0].item.id,
        individual.id
    );
    assert_eq!(db.list_stock(true, 100, 0).await.unwrap().len(), 1);
    db.set_low_stock_at(bulk.id, 6).await.unwrap();
    assert_eq!(db.list_stock(true, 100, 0).await.unwrap().len(), 2);
    assert_eq!(
        db.list_stock(false, 1, 1).await.unwrap()[0].tracking_mode,
        TrackingMode::Individual
    );
    assert!(matches!(
        db.receive_bulk(individual.id, 1).await,
        Err(InventoryError::WrongTrackingMode)
    ));
    assert!(matches!(
        db.receive_units(bulk.id, &[unit(None)]).await,
        Err(InventoryError::WrongTrackingMode)
    ));
    assert!(matches!(
        db.remove_unit(bulk.id, Id::new()).await,
        Err(InventoryError::WrongTrackingMode)
    ));
    assert!(matches!(
        db.set_low_stock_at(Id::new(), 1).await,
        Err(InventoryError::NotFound(_))
    ));
    assert!(matches!(
        db.receive_bulk(Id::new(), 1).await,
        Err(InventoryError::NotFound(_))
    ));
}

#[tokio::test]
async fn invalid_inputs_and_bad_stored_ids_return_errors() {
    let db = database().await;
    assert!(Sku::new(" \t").is_err());
    let mut value = item("VALID");
    value.name = " ".into();
    assert!(matches!(
        db.create_item(&value, TrackingMode::Bulk, 0).await,
        Err(InventoryError::InvalidInput(_))
    ));
    value.name = "valid".into();
    assert!(matches!(
        db.create_item(&value, TrackingMode::Bulk, u64::MAX).await,
        Err(InventoryError::QuantityOverflow)
    ));
    assert!(db.get_item(value.id).await.unwrap().is_none());
    db.create_item(&value, TrackingMode::Individual, 0)
        .await
        .unwrap();
    assert!(matches!(
        db.receive_units(value.id, &[]).await,
        Err(InventoryError::InvalidInput(_))
    ));
    assert!(matches!(
        db.receive_units(value.id, &[unit(Some(" "))]).await,
        Err(InventoryError::InvalidInput(_))
    ));
    sqlx::query("INSERT INTO items VALUES ('not-a-uuid', 'BAD', 'Bad', NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    assert!(matches!(
        db.get_item_by_sku(&Sku::new("BAD").unwrap()).await,
        Err(InventoryError::InvalidData(_))
    ));
}

#[tokio::test]
async fn concurrent_receipts_do_not_lose_updates() {
    let root = std::env::temp_dir().join(format!("pineapple-queries-{}", uuid::Uuid::now_v7()));
    let path = root.join("inventory.db");
    let db = PineappleDatabase::new(&DatabaseSettings {
        url: format!("sqlite://{}", path.display()),
        max_connections: 4,
    })
    .await
    .unwrap();
    let value = item("CONCURRENT");
    db.create_item(&value, TrackingMode::Bulk, 0).await.unwrap();
    tokio::join!(
        async {
            for _ in 0..20 {
                db.receive_bulk(value.id, 1).await.unwrap();
            }
        },
        async {
            for _ in 0..20 {
                db.receive_bulk(value.id, 1).await.unwrap();
            }
        },
    );
    assert_eq!(
        db.get_stock(value.id).await.unwrap().unwrap().quantity,
        StockAmount::Count(40)
    );
    let events = db.list_movements(value.id, 100, 0).await.unwrap();
    assert_eq!(events.len(), 40);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.quantity_before, StockAmount::Count(index as u64));
        assert_eq!(event.quantity_after, StockAmount::Count(index as u64 + 1));
    }
    db.close().await;
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}

fn mass(value: f64, per_piece: Option<f64>) -> StockAmount {
    StockAmount::Mass {
        quantity: Quantity::<Grams>::new(value).unwrap(),
        mass_per_piece: per_piece.map(|value| Quantity::new(value).unwrap()),
    }
}

#[tokio::test]
async fn mass_stock_and_movements_round_trip_with_piece_weights_and_reasons() {
    let db = database().await;
    let value = item("BOLTS");
    db.create_item(&value, TrackingMode::Bulk, mass(100.0, Some(2.5)))
        .await
        .unwrap();
    let before = db.get_inventory(value.id).await.unwrap().unwrap();
    assert_eq!(before.stock.quantity(), mass(0.0, Some(2.5)));
    assert!(before.units.is_none());
    assert_eq!(db.list_stock(true, 10, 0).await.unwrap().len(), 1);
    let added = db
        .add_stock(value.id, mass(1_000.0, None), "Delivery 'quoted'")
        .await
        .unwrap();
    assert_eq!(added.quantity_after, mass(1_000.0, Some(2.5)));
    assert!(db.list_stock(true, 10, 0).await.unwrap().is_empty());
    let removed = db
        .remove_stock(value.id, mass(250.5, None), "Weighed sale")
        .await
        .unwrap();
    let loaded = db.get_inventory(value.id).await.unwrap().unwrap();
    assert_eq!(loaded.stock.quantity(), mass(749.5, Some(2.5)));
    assert_eq!(
        loaded.stock.quantity().estimated_piece_count().unwrap(),
        Some(299.8)
    );
    let events = db.list_movements(value.id, 10, 0).await.unwrap();
    assert_eq!(events.len(), 2);
    for (saved, expected) in events.iter().zip([&added, &removed]) {
        assert_eq!(saved.id, expected.id);
        assert_eq!(saved.kind, expected.kind);
        assert_eq!(saved.amount, expected.amount);
        assert_eq!(saved.quantity_before, expected.quantity_before);
        assert_eq!(saved.quantity_after, expected.quantity_after);
        assert_eq!(saved.reason, expected.reason);
        assert_eq!(saved.occurred_at, expected.occurred_at);
    }
    assert_eq!(
        db.list_movements(value.id, 1, 1).await.unwrap()[0].id,
        removed.id
    );
    db.set_low_stock_at(value.id, mass(749.5, None))
        .await
        .unwrap();
    assert!(db.get_stock(value.id).await.unwrap().unwrap().is_low());
    assert_eq!(db.list_stock(true, 10, 0).await.unwrap().len(), 1);
    assert!(matches!(
        db.delete_item(value.id).await,
        Err(InventoryError::StockNotEmpty)
    ));
    db.remove_stock(value.id, mass(749.5, None), "Remaining sale")
        .await
        .unwrap();
    db.delete_item(value.id).await.unwrap();
    assert!(db.get_inventory(value.id).await.unwrap().is_none());
    assert_eq!(db.list_movements(value.id, 10, 0).await.unwrap().len(), 3);
}

#[tokio::test]
async fn rejected_mass_operations_preserve_balance_threshold_and_history() {
    let db = database().await;
    let value = item("MASS");
    assert!(matches!(
        db.create_item(&value, TrackingMode::Individual, mass(0.0, None))
            .await,
        Err(InventoryError::WrongTrackingMode)
    ));
    assert!(matches!(
        db.create_item(&value, TrackingMode::Bulk, mass(0.0, Some(0.0)))
            .await,
        Err(InventoryError::Stock(StockError::ZeroMassPerPiece))
    ));
    assert!(db.get_item(value.id).await.unwrap().is_none());
    db.create_item(&value, TrackingMode::Bulk, mass(0.5, None))
        .await
        .unwrap();
    db.add_stock(value.id, mass(1.0, None), "Receipt")
        .await
        .unwrap();
    assert!(matches!(
        db.add_stock(value.id, 1_u64, "Wrong unit").await,
        Err(InventoryError::Stock(StockError::MeasurementMismatch))
    ));
    assert!(matches!(
        db.set_low_stock_at(value.id, 0_u64).await,
        Err(InventoryError::Stock(StockError::MeasurementMismatch))
    ));
    assert!(matches!(
        db.remove_stock(value.id, mass(2.0, None), "Too much").await,
        Err(InventoryError::Stock(StockError::InsufficientStock { .. }))
    ));
    assert!(matches!(
        db.add_stock(value.id, mass(1.0, None), " \t").await,
        Err(InventoryError::Stock(StockError::EmptyReason))
    ));
    assert!(matches!(
        db.remove_stock(value.id, mass(0.0, None), "Zero").await,
        Err(InventoryError::Stock(StockError::ZeroQuantityAdjustment))
    ));
    let loaded = db.get_stock(value.id).await.unwrap().unwrap();
    assert_eq!(loaded.quantity, mass(1.0, None));
    assert_eq!(loaded.low_stock_at, mass(0.5, None));
    assert_eq!(db.list_movements(value.id, 10, 0).await.unwrap().len(), 1);
    let huge = item("HUGE");
    db.create_item(&huge, TrackingMode::Bulk, mass(0.0, None))
        .await
        .unwrap();
    db.add_stock(huge.id, mass(f64::MAX, None), "Receipt")
        .await
        .unwrap();
    assert!(matches!(
        db.add_stock(huge.id, mass(f64::MAX, None), "Overflow")
            .await,
        Err(InventoryError::Stock(StockError::Quantity(_)))
    ));
    assert!(matches!(
        db.remove_stock(huge.id, mass(1.0, None), "Rounded away")
            .await,
        Err(InventoryError::Stock(StockError::PrecisionLoss))
    ));
    assert_eq!(
        db.get_stock(huge.id).await.unwrap().unwrap().quantity,
        mass(f64::MAX, None)
    );
    assert_eq!(db.list_movements(huge.id, 10, 0).await.unwrap().len(), 1);
}

#[tokio::test]
async fn audit_failure_rolls_back_balance_and_individual_units() {
    let db = database().await;
    let bulk = item("BULK-ROLLBACK");
    let individual = item("UNITS-ROLLBACK");
    db.create_item(&bulk, TrackingMode::Bulk, 0).await.unwrap();
    db.create_item(&individual, TrackingMode::Individual, 0)
        .await
        .unwrap();
    let existing = unit(None);
    db.receive_units(individual.id, std::slice::from_ref(&existing))
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER fail_audit BEFORE INSERT ON stock_movements BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END")
        .execute(db.pool()).await.unwrap();
    assert!(matches!(
        db.add_stock(bulk.id, 5_u64, "Receipt").await,
        Err(InventoryError::Sql(_))
    ));
    assert!(matches!(
        db.receive_units(individual.id, &[unit(None)]).await,
        Err(InventoryError::Sql(_))
    ));
    assert!(matches!(
        db.remove_unit(individual.id, existing.id).await,
        Err(InventoryError::Sql(_))
    ));
    assert_eq!(
        db.get_stock(bulk.id).await.unwrap().unwrap().quantity,
        StockAmount::Count(0)
    );
    assert!(db.list_movements(bulk.id, 10, 0).await.unwrap().is_empty());
    let state = db.get_inventory(individual.id).await.unwrap().unwrap();
    assert_eq!(state.stock.quantity(), StockAmount::Count(1));
    assert_eq!(state.units.unwrap()[0].id, existing.id);
    assert_eq!(
        db.list_movements(individual.id, 10, 0).await.unwrap().len(),
        1
    );
    assert!(matches!(
        db.add_stock(individual.id, 1_u64, "No unit identity").await,
        Err(InventoryError::WrongTrackingMode)
    ));
}
