//! Terminal entry point. Pages own display state; the app crate owns use cases.
pub mod clap_things;
pub(crate) mod tools;
pub mod widgets;
pub mod workers;
use settings::langs::LanguagePack;
use std::{
    io,
    sync::{Arc, LazyLock},
};
use widgets::inventory_ui::InventoryUi;

/// Initializes the terminal and restores it when the UI returns.
pub fn run() -> io::Result<()> {
    ratatui::run(|terminal| InventoryUi::new().run(terminal))
}
