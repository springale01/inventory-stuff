use chrono::{DateTime, Utc};

use crate::{
    models::stock::{Item, StockAmount},
    tools::id::Id,
};

/// The direction of a completed change. Amounts themselves stay nonnegative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementKind {
    Added,
    Removed,
}

/// A record of a successful in-memory change, with measured balances.
///
/// Returning this event does not save it. A persistence handler should commit
/// the updated balance and this record in the same database transaction.
#[derive(Debug, Clone)]
pub struct MovementEvent {
    pub id: Id<MovementEvent>,
    pub item_id: Id<Item>,
    pub kind: MovementKind,
    /// The requested change, in the same measurement as both balances.
    pub amount: StockAmount,
    pub quantity_before: StockAmount,
    pub quantity_after: StockAmount,
    pub reason: Box<str>,
    /// Time of application, rather than time of command construction.
    pub occurred_at: DateTime<Utc>,
}
