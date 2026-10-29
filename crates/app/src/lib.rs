mod handlers;
mod state;
use std::cell::LazyCell;

pub use state::AppState;

use crate::handlers::event_handler::WorkerHandle;
