//! Inventory queries. Counts are limited to SQLite's signed 64-bit range.
use domain::{
    models::{
        movement::{AddCmd, MovementEvent, MovementKind, RemoveCmd},
        sku::Sku,
        stock::{Item, Stock, StockAmount, StockError, StockUnit},
    },
    tools::{
        id::Id,
        units::{Grams, Quantity},
    },
};
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use uuid::Uuid;

use crate::{Page, PageRequest, PineappleDatabase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackingMode {
    Bulk,
    Individual,
}

impl TrackingMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bulk => "bulk",
            Self::Individual => "individual",
        }
    }
}

#[derive(Debug)]
pub struct StockSummary {
    pub item: Item,
    pub tracking_mode: TrackingMode,
    pub quantity: StockAmount,
    pub low_stock_at: StockAmount,
}

impl StockSummary {
    pub fn is_low(&self) -> bool {
        match (self.quantity, self.low_stock_at) {
            (StockAmount::Count(a), StockAmount::Count(b)) => a <= b,
            (StockAmount::Mass { quantity: a, .. }, StockAmount::Mass { quantity: b, .. }) => {
                a.value() <= b.value()
            }
            _ => false,
        }
    }
}

/// Stock and individual units read from one database transaction.
#[derive(Debug)]
pub struct InventoryState {
    pub stock: Stock,
    /// `None` for bulk tracking; `Some([])` for empty individually tracked stock.
    pub units: Option<Box<[StockUnit]>>,
}

#[derive(Debug, thiserror::Error)]
pub enum InventoryError {
    #[error("Invalid input: {0}")]
    InvalidInput(&'static str),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("Operation does not match the item's stock tracking mode")]
    WrongTrackingMode,
    #[error(transparent)]
    Stock(#[from] StockError),
    #[error("Quantity exceeds SQLite's supported range")]
    QuantityOverflow,
    #[error("Remove all stock before deleting the item")]
    StockNotEmpty,
    #[error("Invalid stored data: {0}")]
    InvalidData(String),
    #[error("Inventory query failed: {0}")]
    Sql(#[from] sqlx::Error),
}

fn integer(value: u64) -> Result<i64, InventoryError> {
    i64::try_from(value).map_err(|_| InventoryError::QuantityOverflow)
}

fn id<T>(row: &SqliteRow, column: &str) -> Result<Id<T>, InventoryError> {
    let text: String = row.try_get(column)?;
    Uuid::parse_str(&text)
        .map(Id::from_uuid)
        .map_err(|error| InventoryError::InvalidData(format!("{column}: {error}")))
}

fn item(row: &SqliteRow) -> Result<Item, InventoryError> {
    let sku: String = row.try_get("sku")?;
    Ok(Item {
        id: id(row, "id")?,
        sku: Sku::new(sku).map_err(|error| InventoryError::InvalidData(error.to_string()))?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
    })
}

fn unit(row: &SqliteRow) -> Result<StockUnit, InventoryError> {
    Ok(StockUnit {
        id: id(row, "id")?,
        serial_number: row.try_get("serial_number")?,
    })
}

fn summary(row: &SqliteRow) -> Result<StockSummary, InventoryError> {
    let mode: String = row.try_get("tracking_mode")?;
    let tracking_mode = match mode.as_str() {
        "bulk" => TrackingMode::Bulk,
        "individual" => TrackingMode::Individual,
        _ => {
            return Err(InventoryError::InvalidData(format!(
                "unknown tracking mode: {mode}"
            )));
        }
    };
    let measurement: String = row.try_get("measurement")?;
    let quantity = decode_amount(
        row,
        &measurement,
        "quantity",
        "quantity_grams",
        "mass_per_piece",
    )?;
    let low_stock_at = decode_amount(
        row,
        &measurement,
        "low_stock_at",
        "low_stock_at_grams",
        "mass_per_piece",
    )?;
    Stock::new(id(row, "id")?, quantity, low_stock_at)
        .map_err(|e| InventoryError::InvalidData(e.to_string()))?;
    if tracking_mode == TrackingMode::Individual && measurement != "count" {
        return Err(InventoryError::InvalidData(
            "individual stock must use counts".into(),
        ));
    }
    Ok(StockSummary {
        item: item(row)?,
        tracking_mode,
        quantity,
        low_stock_at,
    })
}

// Individual quantities are always counted, never copied into stocks.quantity.
macro_rules! stock_select {
    ($suffix:literal) => {
        concat!(
            "SELECT i.id, i.sku, i.name, i.description, s.tracking_mode,
    s.low_stock_at, s.measurement, s.quantity_grams, s.low_stock_at_grams, s.mass_per_piece,
    CASE WHEN s.tracking_mode = 'bulk' THEN s.quantity
    ELSE (SELECT count(*) FROM stock_units u WHERE u.item_id = i.id) END AS quantity
    FROM items i JOIN stocks s ON s.item_id = i.id ",
            $suffix
        )
    };
}

fn measurement(amount: StockAmount) -> &'static str {
    match amount {
        StockAmount::Count(_) => "count",
        StockAmount::Mass { .. } => "mass",
    }
}

// Separate INTEGER and REAL columns keep large piece counts exact.
type AmountColumns = (Option<i64>, Option<f64>, Option<f64>);

fn encode_amount(amount: StockAmount) -> Result<AmountColumns, InventoryError> {
    match amount {
        StockAmount::Count(count) => Ok((Some(integer(count)?), None, None)),
        StockAmount::Mass {
            quantity,
            mass_per_piece,
        } => {
            if mass_per_piece.is_some_and(|value| value.value() == 0.0) {
                return Err(StockError::ZeroMassPerPiece.into());
            }
            Ok((
                None,
                Some(quantity.value()),
                mass_per_piece.map(Quantity::value),
            ))
        }
    }
}

fn decode_amount(
    row: &SqliteRow,
    measurement: &str,
    count: &str,
    grams: &str,
    piece: &str,
) -> Result<StockAmount, InventoryError> {
    let invalid = InventoryError::InvalidData;
    match measurement {
        "count" => {
            let value: Option<i64> = row.try_get(count)?;
            let value = value.ok_or_else(|| invalid(format!("missing {count}")))?;
            Ok(StockAmount::Count(
                u64::try_from(value).map_err(|_| invalid(format!("negative {count}")))?,
            ))
        }
        "mass" => {
            let value: Option<f64> = row.try_get(grams)?;
            let value = value.ok_or_else(|| invalid(format!("missing {grams}")))?;
            let piece: Option<f64> = row.try_get(piece)?;
            let quantity = Quantity::<Grams>::new(value).map_err(|e| invalid(e.to_string()))?;
            let mass_per_piece = piece
                .map(Quantity::<Grams>::new)
                .transpose()
                .map_err(|e| invalid(e.to_string()))?;
            if mass_per_piece.is_some_and(|value| value.value() == 0.0) {
                return Err(invalid("zero mass per piece".into()));
            }
            Ok(StockAmount::Mass {
                quantity,
                mass_per_piece,
            })
        }
        _ => Err(invalid(format!("unknown measurement: {measurement}"))),
    }
}

async fn load_summary(
    connection: &mut SqliteConnection,
    item_id: Id<Item>,
) -> Result<StockSummary, InventoryError> {
    let row = sqlx::query(stock_select!("WHERE i.id = ?"))
        .bind(item_id.id().to_string())
        .fetch_optional(connection)
        .await?
        .ok_or(InventoryError::NotFound("stock"))?;
    summary(&row)
}

async fn write_balance(
    connection: &mut SqliteConnection,
    stock: &Stock,
) -> Result<(), InventoryError> {
    let (count, grams, _) = encode_amount(stock.quantity())?;
    sqlx::query("UPDATE stocks SET quantity = ?, quantity_grams = ? WHERE item_id = ?")
        .bind(count)
        .bind(grams)
        .bind(stock.item_id().id().to_string())
        .execute(connection)
        .await?;
    Ok(())
}

async fn save_movement(
    connection: &mut SqliteConnection,
    event: &MovementEvent,
) -> Result<(), InventoryError> {
    let (amount_count, amount_grams, amount_piece) = encode_amount(event.amount)?;
    let (before_count, before_grams, before_piece) = encode_amount(event.quantity_before)?;
    let (after_count, after_grams, after_piece) = encode_amount(event.quantity_after)?;
    let kind = match event.kind {
        MovementKind::Added => "added",
        MovementKind::Removed => "removed",
    };
    sqlx::query("INSERT INTO stock_movements (id, item_id, kind, measurement, amount_count, before_count, after_count, amount_grams, before_grams, after_grams, amount_mass_per_piece, before_mass_per_piece, after_mass_per_piece, reason, occurred_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(event.id.id().to_string()).bind(event.item_id.id().to_string())
        .bind(kind).bind(measurement(event.amount))
        .bind(amount_count).bind(before_count).bind(after_count)
        .bind(amount_grams).bind(before_grams).bind(after_grams)
        .bind(amount_piece).bind(before_piece).bind(after_piece)
        .bind(&*event.reason).bind(event.occurred_at.to_rfc3339())
        .execute(connection).await?;
    Ok(())
}

fn movement(row: &SqliteRow) -> Result<MovementEvent, InventoryError> {
    let kind: String = row.try_get("kind")?;
    let kind = match kind.as_str() {
        "added" => MovementKind::Added,
        "removed" => MovementKind::Removed,
        _ => {
            return Err(InventoryError::InvalidData(format!(
                "unknown movement kind: {kind}"
            )));
        }
    };
    let measurement: String = row.try_get("measurement")?;
    let occurred_at: String = row.try_get("occurred_at")?;
    let reason: String = row.try_get("reason")?;
    Ok(MovementEvent {
        id: id(row, "id")?,
        item_id: id(row, "item_id")?,
        kind,
        amount: decode_amount(
            row,
            &measurement,
            "amount_count",
            "amount_grams",
            "amount_mass_per_piece",
        )?,
        quantity_before: decode_amount(
            row,
            &measurement,
            "before_count",
            "before_grams",
            "before_mass_per_piece",
        )?,
        quantity_after: decode_amount(
            row,
            &measurement,
            "after_count",
            "after_grams",
            "after_mass_per_piece",
        )?,
        reason: reason.into_boxed_str(),
        occurred_at: chrono::DateTime::parse_from_rfc3339(&occurred_at)
            .map_err(|e| InventoryError::InvalidData(e.to_string()))?
            .with_timezone(&chrono::Utc),
    })
}

impl PineappleDatabase {
    /// Creates the item and empty stock record atomically. IDs come from the caller.
    pub async fn create_item(
        &self,
        value: &Item,
        mode: TrackingMode,
        low_stock_at: impl Into<StockAmount>,
    ) -> Result<(), InventoryError> {
        if value.name.trim().is_empty() {
            return Err(InventoryError::InvalidInput("item name must not be blank"));
        }
        let threshold = low_stock_at.into();
        let zero = match threshold {
            StockAmount::Count(_) => StockAmount::Count(0),
            StockAmount::Mass { mass_per_piece, .. } => StockAmount::Mass {
                quantity: Quantity::new(0.0).expect("zero is valid"),
                mass_per_piece,
            },
        };
        Stock::new(value.id, zero, threshold)?;
        if mode == TrackingMode::Individual && matches!(threshold, StockAmount::Mass { .. }) {
            return Err(InventoryError::WrongTrackingMode);
        }
        let (count, grams, piece) = encode_amount(zero)?;
        let (threshold_count, threshold_grams, _) = encode_amount(threshold)?;
        let mut tx = self.pool().begin().await?;
        sqlx::query("INSERT INTO items (id, sku, name, description) VALUES (?, ?, ?, ?)")
            .bind(value.id.id().to_string())
            .bind(value.sku.as_str())
            .bind(&value.name)
            .bind(&value.description)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at, measurement, quantity_grams, low_stock_at_grams, mass_per_piece) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(value.id.id().to_string()).bind(mode.as_str())
            .bind(if mode == TrackingMode::Bulk { count } else { None }).bind(threshold_count)
            .bind(measurement(zero)).bind(grams).bind(threshold_grams).bind(piece)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_item(&self, item_id: Id<Item>) -> Result<Option<Item>, InventoryError> {
        sqlx::query("SELECT id, sku, name, description FROM items WHERE id = ?")
            .bind(item_id.id().to_string())
            .fetch_optional(self.pool())
            .await?
            .as_ref()
            .map(item)
            .transpose()
    }

    pub async fn get_item_by_sku(&self, sku: &Sku) -> Result<Option<Item>, InventoryError> {
        sqlx::query("SELECT id, sku, name, description FROM items WHERE sku = ?")
            .bind(sku.as_str())
            .fetch_optional(self.pool())
            .await?
            .as_ref()
            .map(item)
            .transpose()
    }

    /// Lists in deterministic SKU order. SKU matching is case-sensitive.
    pub async fn list_items(&self, limit: u32, offset: u64) -> Result<Vec<Item>, InventoryError> {
        sqlx::query(
            "SELECT id, sku, name, description FROM items ORDER BY sku, id LIMIT ? OFFSET ?",
        )
        .bind(i64::from(limit))
        .bind(integer(offset)?)
        .fetch_all(self.pool())
        .await?
        .iter()
        .map(item)
        .collect()
    }

    /// Loads a page and total count from the same read-transaction snapshot.
    pub async fn list_items_page(
        &self,
        request: PageRequest,
    ) -> Result<Page<Item>, InventoryError> {
        self.items_page(None, request).await
    }

    /// Same literal, case-sensitive matching as search_items, including its total count.
    pub async fn search_items_page(
        &self,
        text: &str,
        request: PageRequest,
    ) -> Result<Page<Item>, InventoryError> {
        self.items_page(Some(text), request).await
    }

    async fn items_page(
        &self,
        text: Option<&str>,
        request: PageRequest,
    ) -> Result<Page<Item>, InventoryError> {
        let text = text.map(str::to_owned);
        let mut tx = self.pool().begin().await?;
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE (? IS NULL OR instr(sku, ?) > 0 OR instr(name, ?) > 0)")
            .bind(text.clone()).bind(text.clone()).bind(text.clone())
            .fetch_one(&mut *tx).await?;
        let rows = sqlx::query("SELECT id, sku, name, description FROM items WHERE (? IS NULL OR instr(sku, ?) > 0 OR instr(name, ?) > 0) ORDER BY sku, id LIMIT ? OFFSET ?")
            .bind(text.clone()).bind(text.clone()).bind(text)
            .bind(i64::from(request.size())).bind(request.offset())
            .fetch_all(&mut *tx).await?;
        let items = rows.iter().map(item).collect::<Result<Vec<_>, _>>()?;
        tx.commit().await?;
        let total = u64::try_from(total)
            .map_err(|_| InventoryError::InvalidData("negative item count".into()))?;
        Ok(Page::new(items, total, request))
    }

    /// Literal, case-sensitive substring search of SKU or name. No SQL wildcard syntax.
    pub async fn search_items(
        &self,
        text: &str,
        limit: u32,
        offset: u64,
    ) -> Result<Vec<Item>, InventoryError> {
        sqlx::query("SELECT id, sku, name, description FROM items WHERE instr(sku, ?) > 0 OR instr(name, ?) > 0 ORDER BY sku, id LIMIT ? OFFSET ?")
            .bind(text.to_owned()).bind(text.to_owned()).bind(i64::from(limit)).bind(integer(offset)?)
            .fetch_all(self.pool()).await?.iter().map(item).collect()
    }

    /// Updates product metadata without changing stock, tracking mode, or identity.
    pub async fn update_item(&self, value: &Item) -> Result<(), InventoryError> {
        if value.name.trim().is_empty() {
            return Err(InventoryError::InvalidInput("item name must not be blank"));
        }
        let result =
            sqlx::query("UPDATE items SET sku = ?, name = ?, description = ? WHERE id = ?")
                .bind(value.sku.as_str())
                .bind(&value.name)
                .bind(&value.description)
                .bind(value.id.id().to_string())
                .execute(self.pool())
                .await?;
        if result.rows_affected() == 0 {
            return Err(InventoryError::NotFound("item"));
        }
        Ok(())
    }

    pub async fn get_stock(
        &self,
        item_id: Id<Item>,
    ) -> Result<Option<StockSummary>, InventoryError> {
        sqlx::query(stock_select!("WHERE i.id = ?"))
            .bind(item_id.id().to_string())
            .fetch_optional(self.pool())
            .await?
            .as_ref()
            .map(summary)
            .transpose()
    }

    /// Set low_only to true for the low-stock screen. Includes zero stock.
    pub async fn list_stock(
        &self,
        low_only: bool,
        limit: u32,
        offset: u64,
    ) -> Result<Vec<StockSummary>, InventoryError> {
        sqlx::query(stock_select!("WHERE (? = 0 OR (CASE WHEN s.measurement = 'mass' THEN s.quantity_grams <= s.low_stock_at_grams ELSE (CASE WHEN s.tracking_mode = 'bulk' THEN s.quantity ELSE (SELECT count(*) FROM stock_units u WHERE u.item_id = i.id) END) <= s.low_stock_at END)) ORDER BY i.sku, i.id LIMIT ? OFFSET ?"))
            .bind(low_only).bind(i64::from(limit)).bind(integer(offset)?)
            .fetch_all(self.pool()).await?.iter().map(summary).collect()
    }

    pub async fn set_low_stock_at(
        &self,
        item_id: Id<Item>,
        threshold: impl Into<StockAmount>,
    ) -> Result<(), InventoryError> {
        let threshold = threshold.into();
        let mut tx = self.pool().begin().await?;
        lock_stock(&mut tx, item_id).await?;
        let current = load_summary(&mut tx, item_id).await?;
        Stock::new(item_id, current.quantity, threshold)?;
        let (count, grams, _) = encode_amount(threshold)?;
        sqlx::query("UPDATE stocks SET low_stock_at = ?, low_stock_at_grams = ? WHERE item_id = ?")
            .bind(count)
            .bind(grams)
            .bind(item_id.id().to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// A consistent view for the inventory worker's Get response.
    pub async fn get_inventory(
        &self,
        item_id: Id<Item>,
    ) -> Result<Option<InventoryState>, InventoryError> {
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query(stock_select!("WHERE i.id = ?"))
            .bind(item_id.id().to_string())
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let value = summary(&row)?;
        let stock = Stock::new(item_id, value.quantity, value.low_stock_at)?;
        let units = if value.tracking_mode == TrackingMode::Individual {
            Some(
                sqlx::query(
                    "SELECT id, serial_number FROM stock_units WHERE item_id = ? ORDER BY id",
                )
                .bind(item_id.id().to_string())
                .fetch_all(&mut *tx)
                .await?
                .iter()
                .map(unit)
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
            )
        } else {
            None
        };
        tx.commit().await?;
        Ok(Some(InventoryState { stock, units }))
    }

    /// Applies domain validation and commits the balance and event together.
    /// Individually tracked stock requires explicit unit selection instead.
    pub async fn add_stock(
        &self,
        item_id: Id<Item>,
        amount: impl Into<StockAmount>,
        reason: impl Into<Box<str>>,
    ) -> Result<MovementEvent, InventoryError> {
        let command = AddCmd::new(item_id, amount, reason)?;
        self.change_stock(item_id, |stock| command.execute(stock))
            .await
    }

    pub async fn remove_stock(
        &self,
        item_id: Id<Item>,
        amount: impl Into<StockAmount>,
        reason: impl Into<Box<str>>,
    ) -> Result<MovementEvent, InventoryError> {
        let command = RemoveCmd::new(item_id, amount, reason)?;
        self.change_stock(item_id, |stock| command.execute(stock))
            .await
    }

    async fn change_stock(
        &self,
        item_id: Id<Item>,
        apply: impl FnOnce(&mut Stock) -> Result<MovementEvent, StockError>,
    ) -> Result<MovementEvent, InventoryError> {
        let mut tx = self.pool().begin().await?;
        // Acquire the writer lock before reading: separate workers cannot lose updates.
        lock_stock(&mut tx, item_id).await?;
        let value = load_summary(&mut tx, item_id).await?;
        if value.tracking_mode != TrackingMode::Bulk {
            return Err(InventoryError::WrongTrackingMode);
        }
        let mut stock = Stock::new(item_id, value.quantity, value.low_stock_at)?;
        let event = apply(&mut stock)?;
        write_balance(&mut tx, &stock).await?;
        save_movement(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(event)
    }

    /// Compatibility helpers for count-only callers; new callers should supply a reason.
    pub async fn receive_bulk(&self, item_id: Id<Item>, amount: u64) -> Result<(), InventoryError> {
        self.add_stock(item_id, amount, "Bulk receipt")
            .await
            .map(|_| ())
    }

    pub async fn remove_bulk(&self, item_id: Id<Item>, amount: u64) -> Result<(), InventoryError> {
        self.remove_stock(item_id, amount, "Bulk removal")
            .await
            .map(|_| ())
    }

    /// Absolute count correction. Any actual change is recorded as a movement.
    pub async fn set_bulk_quantity(
        &self,
        item_id: Id<Item>,
        quantity: u64,
    ) -> Result<(), InventoryError> {
        integer(quantity)?;
        let mut tx = self.pool().begin().await?;
        lock_stock(&mut tx, item_id).await?;
        let current = load_summary(&mut tx, item_id).await?;
        if current.tracking_mode != TrackingMode::Bulk {
            return Err(InventoryError::WrongTrackingMode);
        }
        let StockAmount::Count(before) = current.quantity else {
            return Err(StockError::MeasurementMismatch.into());
        };
        let mut stock = Stock::new(item_id, current.quantity, current.low_stock_at)?;
        let event = if quantity > before {
            Some(AddCmd::new(item_id, quantity - before, "Count correction")?.execute(&mut stock)?)
        } else if quantity < before {
            Some(
                RemoveCmd::new(item_id, before - quantity, "Count correction")?
                    .execute(&mut stock)?,
            )
        } else {
            None
        };
        if let Some(event) = event {
            write_balance(&mut tx, &stock).await?;
            save_movement(&mut tx, &event).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// History is ordered by event ID, with stable offset pagination.
    pub async fn list_movements(
        &self,
        item_id: Id<Item>,
        limit: u32,
        offset: u64,
    ) -> Result<Vec<MovementEvent>, InventoryError> {
        sqlx::query("SELECT * FROM stock_movements WHERE item_id = ? ORDER BY id LIMIT ? OFFSET ?")
            .bind(item_id.id().to_string())
            .bind(i64::from(limit))
            .bind(integer(offset)?)
            .fetch_all(self.pool())
            .await?
            .iter()
            .map(movement)
            .collect()
    }
    /// Inserts all units or none. Serial numbers are unique per item when present.
    pub async fn receive_units(
        &self,
        item_id: Id<Item>,
        units: &[StockUnit],
    ) -> Result<(), InventoryError> {
        if units.is_empty() {
            return Err(InventoryError::InvalidInput(
                "at least one unit is required",
            ));
        }
        for unit in units {
            if unit
                .serial_number
                .as_ref()
                .is_some_and(|s| s.trim().is_empty())
            {
                return Err(InventoryError::InvalidInput(
                    "serial number must not be blank",
                ));
            }
        }
        let mut tx = self.pool().begin().await?;
        let mode = lock_stock(&mut tx, item_id).await?;
        if mode != "individual" {
            return Err(InventoryError::WrongTrackingMode);
        }
        let current = load_summary(&mut tx, item_id).await?;
        let mut stock = Stock::new(item_id, current.quantity, current.low_stock_at)?;
        let amount = u64::try_from(units.len()).map_err(|_| InventoryError::QuantityOverflow)?;
        let event = AddCmd::new(item_id, amount, "Individual unit receipt")?.execute(&mut stock)?;
        for unit in units {
            sqlx::query("INSERT INTO stock_units (id, item_id, serial_number) VALUES (?, ?, ?)")
                .bind(unit.id.id().to_string())
                .bind(item_id.id().to_string())
                .bind(&unit.serial_number)
                .execute(&mut *tx)
                .await?;
        }
        save_movement(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_units(
        &self,
        item_id: Id<Item>,
        limit: u32,
        offset: u64,
    ) -> Result<Vec<StockUnit>, InventoryError> {
        sqlx::query("SELECT id, serial_number FROM stock_units WHERE item_id = ? ORDER BY id LIMIT ? OFFSET ?")
            .bind(item_id.id().to_string()).bind(i64::from(limit)).bind(integer(offset)?)
            .fetch_all(self.pool()).await?.iter().map(unit).collect()
    }

    pub async fn get_unit(
        &self,
        item_id: Id<Item>,
        unit_id: Id<StockUnit>,
    ) -> Result<Option<StockUnit>, InventoryError> {
        sqlx::query("SELECT id, serial_number FROM stock_units WHERE item_id = ? AND id = ?")
            .bind(item_id.id().to_string())
            .bind(unit_id.id().to_string())
            .fetch_optional(self.pool())
            .await?
            .as_ref()
            .map(unit)
            .transpose()
    }

    /// Removes exactly the requested unit belonging to this item.
    pub async fn remove_unit(
        &self,
        item_id: Id<Item>,
        unit_id: Id<StockUnit>,
    ) -> Result<StockUnit, InventoryError> {
        let mut tx = self.pool().begin().await?;
        let mode = lock_stock(&mut tx, item_id).await?;
        if mode != "individual" {
            return Err(InventoryError::WrongTrackingMode);
        }
        let current = load_summary(&mut tx, item_id).await?;
        let mut stock = Stock::new(item_id, current.quantity, current.low_stock_at)?;
        let row = sqlx::query(
            "DELETE FROM stock_units WHERE item_id = ? AND id = ? RETURNING id, serial_number",
        )
        .bind(item_id.id().to_string())
        .bind(unit_id.id().to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(InventoryError::NotFound("stock unit"))?;
        let removed = unit(&row)?;
        let event =
            RemoveCmd::new(item_id, 1_u64, "Individual unit removal")?.execute(&mut stock)?;
        save_movement(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(removed)
    }

    /// Refuses deletion while bulk quantity or individual units remain.
    pub async fn delete_item(&self, item_id: Id<Item>) -> Result<(), InventoryError> {
        let mut tx = self.pool().begin().await?;
        lock_stock(&mut tx, item_id).await?;
        let current = load_summary(&mut tx, item_id).await?;
        let units: i64 = sqlx::query_scalar("SELECT count(*) FROM stock_units WHERE item_id = ?")
            .bind(item_id.id().to_string())
            .fetch_one(&mut *tx)
            .await?;
        if !current.quantity.is_zero() || units != 0 {
            return Err(InventoryError::StockNotEmpty);
        }
        sqlx::query("DELETE FROM stocks WHERE item_id = ?")
            .bind(item_id.id().to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM items WHERE id = ?")
            .bind(item_id.id().to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn lock_stock(
    connection: &mut SqliteConnection,
    item_id: Id<Item>,
) -> Result<String, InventoryError> {
    let mode = sqlx::query_scalar(
        "UPDATE stocks SET low_stock_at = low_stock_at WHERE item_id = ? RETURNING tracking_mode",
    )
    .bind(item_id.id().to_string())
    .fetch_optional(connection)
    .await?
    .ok_or(InventoryError::NotFound("stock"))?;
    Ok(mode)
}
