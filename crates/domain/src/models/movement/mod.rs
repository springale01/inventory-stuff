//! Start with a command, apply it to stock, then inspect the resulting event.
//!
//! ```
//! use domain::{
//!     models::{movement::{AddCmd, RemoveCmd, MovementKind}, stock::{Item, Stock, StockAmount}},
//!     tools::{id::Id, units::{Grams, Quantity}},
//! };
//!
//! let item_id = Id::<Item>::new();
//! let grams = |value| Quantity::<Grams>::new(value);
//! let mut stock = Stock::new(item_id, grams(1_000.0)?, grams(200.0)?)?;
//! let event = RemoveCmd::new(item_id, grams(250.0)?, "Sold by weight")?
//!     .execute(&mut stock)?;
//! assert_eq!(event.kind, MovementKind::Removed);
//! assert_eq!(event.quantity_before, StockAmount::Mass { quantity: grams(1_000.0)?, mass_per_piece: None });
//! assert_eq!(stock.quantity(), StockAmount::Mass { quantity: grams(750.0)?, mass_per_piece: None });
//!
//! // Counts use integers, so a fractional keyboard cannot be constructed.
//! let keyboard_id = Id::<Item>::new();
//! let mut keyboards = Stock::new(keyboard_id, 0_u64, 2_u64)?;
//! AddCmd::new(keyboard_id, 12_u64, "Delivery")?.execute(&mut keyboards)?;
//! assert_eq!(keyboards.quantity(), StockAmount::Count(12));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod command;
mod event;

pub use command::{AddCmd, RemoveCmd};
pub use event::{MovementEvent, MovementKind};

#[cfg(test)]
mod tests;
