//! Commands own their inputs and describe intent, never a precomputed new balance.

use super::{MovementEvent, MovementKind};
use crate::{
    Request,
    models::stock::{Item, Stock, StockAmount, StockError},
    tools::id::Id,
};

/// A request to receive a positive amount of stock for one item.
#[derive(Debug, Clone)]
pub struct AddCmd {
    item_id: Id<Item>,
    amount: StockAmount,
    reason: Box<str>,
}

impl AddCmd {
    /// Rejects zero amounts and blank reasons. `Into` accepts `StockAmount`,
    /// `u64` counts, or `Quantity<Grams>` through their `From` implementations.
    pub fn new(
        item_id: Id<Item>,
        amount: impl Into<StockAmount>,
        reason: impl Into<Box<str>>,
    ) -> Result<Self, StockError> {
        let amount = amount.into();
        let reason = reason.into();
        validate_input(amount, &reason)?;
        Ok(Self {
            item_id,
            amount,
            reason,
        })
    }

    /// Consumes this command and applies it with exclusive access to stock.
    /// This is synchronous: an application handler can load stock asynchronously,
    /// call this domain operation, then persist the new balance and event together.
    pub fn execute(self, stock: &mut Stock) -> Result<MovementEvent, StockError> {
        stock.apply(self.item_id, MovementKind::Added, self.amount, self.reason)
    }
}

impl Request for AddCmd {
    // The execution Result carries errors; Response is only the successful value.
    type Response = MovementEvent;
}

/// A request to remove a positive amount without allowing a negative balance.
#[derive(Debug, Clone)]
pub struct RemoveCmd {
    item_id: Id<Item>,
    amount: StockAmount,
    reason: Box<str>,
}

impl RemoveCmd {
    /// Validates inputs. Availability is checked at execution time, because the
    /// balance may change between constructing and executing a command.
    pub fn new(
        item_id: Id<Item>,
        amount: impl Into<StockAmount>,
        reason: impl Into<Box<str>>,
    ) -> Result<Self, StockError> {
        let amount = amount.into();
        let reason = reason.into();
        validate_input(amount, &reason)?;
        Ok(Self {
            item_id,
            amount,
            reason,
        })
    }

    /// Returns a completed movement or leaves stock unchanged on failure.
    /// Consuming the command discourages accidental reuse; persistent retries
    /// still need an idempotency policy in the application/persistence layer.
    pub fn execute(self, stock: &mut Stock) -> Result<MovementEvent, StockError> {
        stock.apply(
            self.item_id,
            MovementKind::Removed,
            self.amount,
            self.reason,
        )
    }
}

impl Request for RemoveCmd {
    type Response = MovementEvent;
}

// Share validation without a macro so both public command types stay easy to read.
fn validate_input(amount: StockAmount, reason: &str) -> Result<(), StockError> {
    amount.validate()?;
    if amount.is_zero() {
        return Err(StockError::ZeroQuantityAdjustment);
    }
    if reason.trim().is_empty() {
        return Err(StockError::EmptyReason);
    }
    Ok(())
}
