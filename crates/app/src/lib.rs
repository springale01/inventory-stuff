#![doc = include_str!("../README.md")]

mod handlers;
mod state;
pub use handlers::inventory_handle::InventoryHandle;
pub use handlers::inventory_worker::InventoryError;
pub use handlers::settings_worker::{SettingsError, SettingsHandle, SettingsWorker};
pub use state::{AppError, AppState, InventorySnapshot};
pub mod statics;

pub use domain;
pub use persistance;
pub use settings;
// Keep the existing SKU convenience imports available.
pub use domain::{
    CounterExhausted, CounterSkuBuilder, CounterSkuSupplier, InvalidSku, Sku, SkuBuilder, SkuFamily,
};
pub use statics::{PineappleState, StaticsError, initialize, set_statics, shutdown_statics};
