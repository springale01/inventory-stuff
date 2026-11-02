use crate::handlers::{event_handler::WorkerHandle, inventory_worker};

pub struct AppState {
    pub(crate) handles: Handles,
}

impl AppState {}

pub(crate) struct Handles {
    // we can do a Vec later on if we need scaling
    pub(crate) inventory_handle:
        WorkerHandle<inventory_worker::InventoryMessage, inventory_worker::InventoryError>,
}
