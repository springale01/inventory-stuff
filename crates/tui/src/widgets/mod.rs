use std::any::Any;
pub mod add_page;
pub(crate) mod inventory_ui;
pub mod main_page;
pub trait Widgets: Any + Send {
    type Error: Send;

    fn layer(&self) -> u8;
    fn is_active(&self) -> bool;
}
