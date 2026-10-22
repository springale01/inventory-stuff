# SQLite startup

```rust,no_run
use persistance::PineappleDatabase;
use settings::database::DatabaseSettings;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let db = PineappleDatabase::new(&DatabaseSettings::default()).await?;
let count: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
    .fetch_one(db.pool())
    .await?;
println!("{count} items");
db.close().await;
# Ok(())
# }
```

Startup creates missing directories and the SQLite file, enables foreign keys,
and runs pending migrations before returning a usable pool. Migrations are
embedded from `database/migrations` at build time, so the running app does not
need a migrations directory. Migration errors are returned without dropping or
resetting existing data.

The default URL `sqlite://database/sqlite.db` resolves relative to the process
working directory. Start the app from the workspace root to use the project's
database directory, or configure an absolute URL for a stable location.
`sqlite::memory:` is also supported for temporary databases.

Clone `PineappleDatabase` to share its pool. Use `db.pool().begin().await?` for
transactions. Call `close().await` during shutdown after operations finish;
closing any clone closes the shared pool for every clone.

For manual migrations, run from the workspace root:

```powershell
sqlx database create --database-url "sqlite://database/sqlite.db"
sqlx migrate run --source database/migrations --database-url "sqlite://database/sqlite.db"
```

Add a new migration for future schema changes; do not edit migrations already
applied to a database. SQLite quantities are signed 64-bit integers, so persistence
must reject domain `u64` values above `i64::MAX` before binding them.

## Inventory methods

Queries use domain values from `domain::models::stock`, `domain::models::sku`, and `domain::tools::id`.
Use `Sku::new(...)` to validate a SKU and `Id::new()` to generate an identity.

```rust,no_run
use domain::{models::{sku::Sku, stock::Item}, tools::id::Id};
use persistance::{PineappleDatabase, TrackingMode};

# async fn example(db: &PineappleDatabase) -> Result<(), Box<dyn std::error::Error>> {
let screws = Item {
    id: Id::new(),
    sku: Sku::new("SCREW-M4")?,
    name: "M4 screws".into(),
    description: None,
};
db.create_item(&screws, TrackingMode::Bulk, 20).await?;
db.receive_bulk(screws.id, 100).await?;
db.remove_bulk(screws.id, 5).await?;
let stock = db.get_stock(screws.id).await?.expect("item was created");
assert_eq!(stock.quantity, 95);

let low_stock = db.list_stock(true, 50, 0).await?;
let matching_items = db.search_items("screws", 50, 0).await?;
# Ok(())
# }
```

| Task | Method |
| --- | --- |
| Create a product and its empty stock together | `create_item(&item, mode, low_stock_at)` |
| Find a product | `get_item(id)`, `get_item_by_sku(&sku)` |
| Browse or search products | `list_items(limit, offset)`, `search_items(text, limit, offset)` |
| Edit SKU, name, description | `update_item(&item)` |
| Quantity, tracking mode, and threshold | `get_stock(id)` |
| Browse stock or only low stock | `list_stock(low_only, limit, offset)` |
| Change the alert threshold | `set_low_stock_at(id, threshold)` |
| Receive/remove bulk stock | `receive_bulk(id, amount)`, `remove_bulk(id, amount)` |
| Correct the absolute bulk count | `set_bulk_quantity(id, quantity)` |
| Receive individual pieces atomically | `receive_units(item_id, &[StockUnit])` |
| Browse/find individual pieces | `list_units(item_id, limit, offset)`, `get_unit(item_id, unit_id)` |
| Remove one specific physical piece | `remove_unit(item_id, unit_id)` |
| Delete an empty product and stock record | `delete_item(id)` |

Single-record lookups return `Option`; mutations return `InventoryError::NotFound`
when the target is missing. Unit lists are empty for missing items or bulk items.
Lists use stable ordering and offset pagination. Search is a literal, case-sensitive
substring match; SKUs and serial numbers are also case-sensitive. An empty search
matches all products. List limits of zero return no rows.

Bulk changes reject zero adjustments, underflow, and values above `i64::MAX`.
An absolute correction may set quantity to zero. Individual quantities are derived
from unit rows. Wrong-mode mutations return `WrongTrackingMode`. Batches and
multi-record mutations run in transactions, and bulk adjustments serialize writes
to prevent lost updates. Failed batches leave no partially inserted units.
Deletion refuses to discard remaining inventory. Tracking-mode conversion and
movement audit history are not implemented.

Duplicate SKUs, unit IDs, or per-item serial numbers return `InventoryError::Sql`;
the underlying SQLx database error exposes `is_unique_violation()` for UI messages.
Invalid IDs or values loaded from externally modified databases return
`InvalidData` instead of panicking. All user values are bound as parameters.

## Numbered item pages

```rust,no_run
use persistance::{PageRequest, PineappleDatabase};

# async fn example(db: &PineappleDatabase) -> Result<(), Box<dyn std::error::Error>> {
let page = db.list_items_page(PageRequest::new(1, 20)?).await?;
println!("Page {} of {} ({} items)",
    page.request().number(), page.total_pages(), page.total_items);
for item in &page.items {
    println!("{}: {}", item.sku.as_str(), item.name);
}
if let Some(next) = page.next_page() {
    let next_page = db.list_items_page(next).await?;
}
let matches = db.search_items_page("screw", PageRequest::new(1, 20)?).await?;
# Ok(())
# }
```

`PageRequest::new(page_number, page_size)` uses page numbers starting at 1.
The size must be 1 through 200; the default is page 1 with 25 items.
The database computes `offset = (page_number - 1) * page_size` and returns
at most `page_size` records in SKU order, using the ID to break ties.
For 45 items with size 20, pages contain 20, 20, and 5 items respectively.

`Page<Item>` includes `items`, `total_items`, `total_pages()`, and optional
`next_page()` / `previous_page()` requests for UI buttons. Empty results have
zero total pages. Requests beyond the last page return an empty items vector
with accurate totals, rather than silently changing the requested page.
The previous-page request still lets the UI navigate back from such a page.

The total and rows are read in one transaction so each response uses one database
snapshot. Search totals count only matches. When changing the search text, reset
the UI to page 1 and use that same text for next/previous requests. Each request
reads fresh data: inserts, deletes, or SKU edits between requests can move rows
between pages. Offset pagination can slow down on very deep pages; cursor
pagination can be added if that becomes a practical problem. The existing
limit/offset methods remain available.
