//! Inventory queries. Counts are limited to SQLite's signed 64-bit range.
use domain::{
    models::sku::Sku,
    models::stock::{Item, StockUnit},
    tools::id::Id,
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
    pub quantity: u64,
    pub low_stock_at: u64,
}

impl StockSummary {
    pub fn is_low(&self) -> bool {
        self.quantity <= self.low_stock_at
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InventoryError {
    #[error("Invalid input: {0}")]
    InvalidInput(&'static str),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("Operation does not match the item's stock tracking mode")]
    WrongTrackingMode,
    #[error("Insufficient stock: available {available}, requested {requested}")]
    InsufficientStock { available: u64, requested: u64 },
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
    let quantity: i64 = row.try_get("quantity")?;
    let threshold: i64 = row.try_get("low_stock_at")?;
    Ok(StockSummary {
        item: item(row)?,
        tracking_mode,
        quantity: u64::try_from(quantity)
            .map_err(|_| InventoryError::InvalidData("negative quantity".into()))?,
        low_stock_at: u64::try_from(threshold)
            .map_err(|_| InventoryError::InvalidData("negative threshold".into()))?,
    })
}

// Individual quantities are always counted, never copied into stocks.quantity.
macro_rules! stock_select {
    ($suffix:literal) => {
        concat!(
            "SELECT i.id, i.sku, i.name, i.description, s.tracking_mode,
    s.low_stock_at, CASE WHEN s.tracking_mode = 'bulk' THEN s.quantity
    ELSE (SELECT count(*) FROM stock_units u WHERE u.item_id = i.id) END AS quantity
    FROM items i JOIN stocks s ON s.item_id = i.id ",
            $suffix
        )
    };
}

impl PineappleDatabase {
    /// Creates the item and empty stock record atomically. IDs come from the caller.
    pub async fn create_item(
        &self,
        value: &Item,
        mode: TrackingMode,
        low_stock_at: u64,
    ) -> Result<(), InventoryError> {
        if value.name.trim().is_empty() {
            return Err(InventoryError::InvalidInput("item name must not be blank"));
        }
        let threshold = integer(low_stock_at)?;
        let mut tx = self.pool().begin().await?;
        sqlx::query("INSERT INTO items (id, sku, name, description) VALUES (?, ?, ?, ?)")
            .bind(value.id.id().to_string())
            .bind(value.sku.as_str())
            .bind(&value.name)
            .bind(&value.description)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO stocks (item_id, tracking_mode, quantity, low_stock_at) VALUES (?, ?, ?, ?)")
            .bind(value.id.id().to_string()).bind(mode.as_str())
            .bind(if mode == TrackingMode::Bulk { Some(0_i64) } else { None }).bind(threshold)
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
        sqlx::query(stock_select!("WHERE (? = 0 OR (CASE WHEN s.tracking_mode = 'bulk' THEN s.quantity ELSE (SELECT count(*) FROM stock_units u WHERE u.item_id = i.id) END) <= s.low_stock_at) ORDER BY i.sku, i.id LIMIT ? OFFSET ?"))
            .bind(low_only).bind(i64::from(limit)).bind(integer(offset)?)
            .fetch_all(self.pool()).await?.iter().map(summary).collect()
    }

    pub async fn set_low_stock_at(
        &self,
        item_id: Id<Item>,
        threshold: u64,
    ) -> Result<(), InventoryError> {
        let result = sqlx::query("UPDATE stocks SET low_stock_at = ? WHERE item_id = ?")
            .bind(integer(threshold)?)
            .bind(item_id.id().to_string())
            .execute(self.pool())
            .await?;
        if result.rows_affected() == 0 {
            return Err(InventoryError::NotFound("stock"));
        }
        Ok(())
    }

    pub async fn receive_bulk(&self, item_id: Id<Item>, amount: u64) -> Result<(), InventoryError> {
        self.change_bulk(item_id, amount, BulkChange::Receive).await
    }

    pub async fn remove_bulk(&self, item_id: Id<Item>, amount: u64) -> Result<(), InventoryError> {
        self.change_bulk(item_id, amount, BulkChange::Remove).await
    }

    /// Absolute correction; zero is allowed. No movement audit is recorded yet.
    pub async fn set_bulk_quantity(
        &self,
        item_id: Id<Item>,
        quantity: u64,
    ) -> Result<(), InventoryError> {
        self.change_bulk(item_id, quantity, BulkChange::Set).await
    }

    async fn change_bulk(
        &self,
        item_id: Id<Item>,
        value: u64,
        change: BulkChange,
    ) -> Result<(), InventoryError> {
        if value == 0 && !matches!(change, BulkChange::Set) {
            return Err(InventoryError::InvalidInput(
                "adjustment must be greater than zero",
            ));
        }
        let value_i64 = integer(value)?;
        let mut tx = self.pool().begin().await?;
        // Write first to acquire SQLite's writer lock before reading stock.
        // All arithmetic happens under that lock, preventing lost updates.
        let (mode, available) = lock_stock(&mut tx, item_id).await?;
        if mode != "bulk" {
            return Err(InventoryError::WrongTrackingMode);
        }
        let available = available
            .ok_or_else(|| InventoryError::InvalidData("bulk stock has no quantity".into()))?;
        let next = match change {
            BulkChange::Receive => available
                .checked_add(value_i64)
                .ok_or(InventoryError::QuantityOverflow)?,
            BulkChange::Remove => available.checked_sub(value_i64).filter(|v| *v >= 0).ok_or(
                InventoryError::InsufficientStock {
                    available: available as u64,
                    requested: value,
                },
            )?,
            BulkChange::Set => value_i64,
        };
        sqlx::query("UPDATE stocks SET quantity = ? WHERE item_id = ?")
            .bind(next)
            .bind(item_id.id().to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
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
        let (mode, _) = lock_stock(&mut tx, item_id).await?;
        if mode != "individual" {
            return Err(InventoryError::WrongTrackingMode);
        }
        for unit in units {
            sqlx::query("INSERT INTO stock_units (id, item_id, serial_number) VALUES (?, ?, ?)")
                .bind(unit.id.id().to_string())
                .bind(item_id.id().to_string())
                .bind(&unit.serial_number)
                .execute(&mut *tx)
                .await?;
        }
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
        let (mode, _) = lock_stock(&mut tx, item_id).await?;
        if mode != "individual" {
            return Err(InventoryError::WrongTrackingMode);
        }
        let row = sqlx::query(
            "DELETE FROM stock_units WHERE item_id = ? AND id = ? RETURNING id, serial_number",
        )
        .bind(item_id.id().to_string())
        .bind(unit_id.id().to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(InventoryError::NotFound("stock unit"))?;
        let removed = unit(&row)?;
        tx.commit().await?;
        Ok(removed)
    }

    /// Refuses deletion while bulk quantity or individual units remain.
    pub async fn delete_item(&self, item_id: Id<Item>) -> Result<(), InventoryError> {
        let mut tx = self.pool().begin().await?;
        let (_, quantity) = lock_stock(&mut tx, item_id).await?;
        let units: i64 = sqlx::query_scalar("SELECT count(*) FROM stock_units WHERE item_id = ?")
            .bind(item_id.id().to_string())
            .fetch_one(&mut *tx)
            .await?;
        if quantity.unwrap_or(0) != 0 || units != 0 {
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

enum BulkChange {
    Receive,
    Remove,
    Set,
}

async fn lock_stock(
    connection: &mut SqliteConnection,
    item_id: Id<Item>,
) -> Result<(String, Option<i64>), InventoryError> {
    let row = sqlx::query("UPDATE stocks SET low_stock_at = low_stock_at WHERE item_id = ? RETURNING tracking_mode, quantity")
        .bind(item_id.id().to_string()).fetch_optional(connection).await?
        .ok_or(InventoryError::NotFound("stock"))?;
    Ok((row.try_get("tracking_mode")?, row.try_get("quantity")?))
}
