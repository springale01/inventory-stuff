//! Terminal entry point. Pages own display state; the app crate owns use cases.
pub mod widgets;

use std::io;
use widgets::inventory_ui::InventoryUi;

/// Initializes the terminal and restores it when the UI returns.
pub fn run() -> io::Result<()> {
    ratatui::run(|terminal| InventoryUi::new().run(terminal))
}
