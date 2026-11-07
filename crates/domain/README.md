# Domain API

`domain` contains synchronous inventory rules and value types. It performs no
database, filesystem, terminal, or Tokio operations.

Public types can be imported from the root; existing grouped paths such as
`domain::models::stock::Item` and `domain::tools::id::Id` remain supported.

```rust
use domain::{AddCmd, Id, Item, SkuBuilder, Stock, StockAmount};

let sku = SkuBuilder::new().add_field("BOLT").add_field("M8").finalize()?;
assert_eq!(sku.as_str(), "BOLT-M8");
let item = Id::<Item>::new();
let mut stock = Stock::new(item, 0_u64, 2_u64)?;
let movement = AddCmd::new(item, 5_u64, "Delivery")?.execute(&mut stock)?;
assert_eq!(movement.quantity_after, StockAmount::Count(5));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Id<T>` labels IDs by their entity type. `Sku` preserves exact text;
`SkuBuilder` validates and joins editable fields. `StockAmount` describes whole
counts or mass; `Quantity<U>` supports checked conversions within one dimension.
Commands validate changes and return movement events. Persistence must save those
events and the updated stock atomically. `Snapshot<T>` owns a point-in-time value.

Errors are specific to their operations: `InvalidSku`, `StockError`,
`QuantityError`, and `CounterExhausted`. All are available from the root.

## Sequential SKUs

```rust
use domain::{CounterSkuBuilder, SkuFamily};

struct Hardware;
impl SkuFamily for Hardware {}

let supplier = CounterSkuBuilder::new(None, Some(1))
    .add("BOLT")
    .build::<Hardware>(Some(4))?;
assert_eq!(supplier.supply()?.as_str(), "BOLT-0001");
assert_eq!(supplier.supply()?.as_str(), "BOLT-0002");
# Ok::<(), Box<dyn std::error::Error>>(())
```

The counter is thread-safe but in-memory. It does not save sequence state or
guarantee uniqueness against existing database rows after restart. A family is a
compile-time label, not text validation. Canonical grouped imports use
`models::sku::counter`; the earlier `sku_but_with_a_counter` path remains an alias.

`Request` associates request and response types. `Handle` describes an optional
async handler interface whose future can borrow the handler. Domain commands
remain synchronous and do not require a handler implementation to execute.
