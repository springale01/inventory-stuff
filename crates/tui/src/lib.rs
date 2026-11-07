#![doc = include_str!("../README.md")]
pub mod clap_things;
pub(crate) mod tools;
pub mod widgets;
pub use widgets::{
    inventory_page::{InventoryAction, InventoryPage, ItemRow},
    inventory_ui::{InventoryUi, UiError},
    page::{NavigationState, Page, PageId, UiAction},
};

/// Initializes the terminal and restores it when the UI returns.
pub async fn run() -> Result<(), UiError> {
    let mut ui = InventoryUi::new().await?;
    // Created before initialization to also restore after a partial init failure.
    let _restore = RestoreTerminal;
    let mut terminal = ratatui::try_init()?;
    ui.run(&mut terminal).await
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
