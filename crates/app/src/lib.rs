mod handlers;
mod state;
pub use handlers::inventory_worker::InventoryError;
pub use handlers::settings_worker::{SettingsError, SettingsHandle, SettingsWorker};
pub use state::{AppError, AppState, InventorySnapshot};
pub mod statics;

pub use domain::models::sku::{
    Sku, SkuBuilder,
    sku_but_with_a_counter::{CounterExhausted, SkuFamily},
};
