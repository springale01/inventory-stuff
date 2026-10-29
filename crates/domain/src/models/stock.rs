//! Item identity and stock balances. Stock owns the rules for changing its balance.

use crate::{
    models::{
        movement::{MovementEvent, MovementKind},
        sku::Sku,
    },
    tools::{
        id::Id,
        units::{Grams, Quantity, QuantityError},
    },
};
use chrono::Utc;

/// Catalog information shared by stock belonging to this item.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: Id<Item>,
    pub sku: Sku,
    pub name: String,
    pub description: Option<String>,
}

/// An individually identified unit, used by the existing persistence API.
/// The balance model below tracks totals; serial-number selection is a separate
/// operation that must be coordinated with totals when that feature is integrated.
#[derive(Debug, Clone)]
pub struct StockUnit {
    pub id: Id<StockUnit>,
    pub serial_number: Option<String>,
}

/// A nonnegative balance or adjustment in whole pieces or grams.
///
/// Stock keeps one measurement, even at zero. Estimated piece counts do not
/// change the balance: turning an estimate into a deduction needs an explicit
/// business rule. Convert
/// kilograms to grams before constructing mass. Mass arithmetic remains approximate
/// because `Quantity` uses `f64`; integer counts stay exact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StockAmount {
    Count(u64),
    Mass {
        quantity: Quantity<Grams>,
        /// Average piece weight, strictly positive when present. Movements
        /// preserve the balance's piece weight and change only total mass.
        mass_per_piece: Option<Quantity<Grams>>,
    },
}

impl StockAmount {
    /// Estimates pieces from total mass / average piece weight, without rounding.
    /// Returns `None` for counts (which are already exact) or unknown piece weight.
    /// An invalid piece weight or nonfinite estimate returns an error.
    pub fn estimated_piece_count(self) -> Result<Option<f64>, StockError> {
        self.validate()?;
        match self {
            Self::Mass {
                quantity,
                mass_per_piece: Some(piece_mass),
            } => {
                let estimate = quantity.value() / piece_mass.value();
                if !estimate.is_finite() {
                    return Err(QuantityError::Overflow.into());
                }
                if quantity.value() > 0.0 && estimate == 0.0 {
                    return Err(QuantityError::Underflow.into());
                }
                Ok(Some(estimate))
            }
            _ => Ok(None),
        }
    }

    pub(super) fn validate(self) -> Result<(), StockError> {
        if matches!(self, Self::Mass { mass_per_piece: Some(value), .. } if value.value() == 0.0) {
            return Err(StockError::ZeroMassPerPiece);
        }
        Ok(())
    }

    /// Zero is valid for balances and thresholds, but not for movement commands.
    pub fn is_zero(self) -> bool {
        match self {
            Self::Count(value) => value == 0,
            Self::Mass { quantity, .. } => quantity.value() == 0.0,
        }
    }

    fn same_measurement(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Count(_), Self::Count(_)) | (Self::Mass { .. }, Self::Mass { .. })
        )
    }

    fn checked_add(self, amount: Self) -> Result<Self, StockError> {
        match (self, amount) {
            (Self::Count(a), Self::Count(b)) => a
                .checked_add(b)
                .map(Self::Count)
                .ok_or(StockError::Overflow),
            (
                Self::Mass {
                    quantity: a,
                    mass_per_piece,
                },
                Self::Mass { quantity: b, .. },
            ) => Ok(Self::Mass {
                quantity: a.checked_add(b)?,
                mass_per_piece,
            }),
            _ => Err(StockError::MeasurementMismatch),
        }
    }

    fn checked_sub(self, amount: Self) -> Result<Self, StockError> {
        // Never convert counts to f64: large integers would lose precision.
        let insufficient = match (self, amount) {
            (Self::Count(a), Self::Count(b)) => b > a,
            (Self::Mass { quantity: a, .. }, Self::Mass { quantity: b, .. }) => {
                b.value() > a.value()
            }
            _ => return Err(StockError::MeasurementMismatch),
        };
        if insufficient {
            return Err(StockError::InsufficientStock {
                available: self,
                requested: amount,
            });
        }
        match (self, amount) {
            (Self::Count(a), Self::Count(b)) => Ok(Self::Count(a - b)),
            (
                Self::Mass {
                    quantity: a,
                    mass_per_piece,
                },
                Self::Mass { quantity: b, .. },
            ) => Ok(Self::Mass {
                quantity: a.checked_sub(b)?,
                mass_per_piece,
            }),
            _ => unreachable!("measurements were checked above"),
        }
    }
}

impl From<u64> for StockAmount {
    fn from(value: u64) -> Self {
        Self::Count(value)
    }
}

impl From<Quantity<Grams>> for StockAmount {
    fn from(value: Quantity<Grams>) -> Self {
        Self::Mass {
            quantity: value,
            mass_per_piece: None,
        }
    }
}

impl std::fmt::Display for StockAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Count(value) => write!(f, "{value} pieces"),
            Self::Mass { quantity, .. } => quantity.fmt(f),
        }
    }
}

/// The current balance for one item, with a threshold in the same measurement.
///
/// Private fields prevent bypassing validation. This is an in-memory balance;
/// coordinating multiple database writers belongs in the persistence layer.
#[derive(Debug, Clone)]
pub struct Stock {
    item_id: Id<Item>,
    quantity: StockAmount,
    low_stock_at: StockAmount,
}

/// A rejected domain operation leaves the original balance unchanged.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StockError {
    #[error("mass per piece must be positive")]
    ZeroMassPerPiece,
    #[error("stock movements must have a positive amount")]
    ZeroQuantityAdjustment,
    #[error("movement reason must not be blank")]
    EmptyReason,
    #[error("command targets a different item")]
    ItemMismatch,
    #[error("stock and amount must use the same measurement")]
    MeasurementMismatch,
    #[error("insufficient stock: available {available}, requested {requested}")]
    InsufficientStock {
        available: StockAmount,
        requested: StockAmount,
    },
    #[error("stock count overflow")]
    Overflow,
    #[error(transparent)]
    Quantity(#[from] QuantityError),
    #[error("mass adjustment is too small to change this balance at f64 precision")]
    PrecisionLoss,
}

impl Stock {
    /// Creates a balance. Zero is allowed; the threshold must use the same
    /// measurement. Initial state does not emit a movement event.
    pub fn new(
        item_id: Id<Item>,
        quantity: impl Into<StockAmount>,
        low_stock_at: impl Into<StockAmount>,
    ) -> Result<Self, StockError> {
        let quantity = quantity.into();
        let low_stock_at = low_stock_at.into();
        quantity.validate()?;
        low_stock_at.validate()?;
        if !quantity.same_measurement(low_stock_at) {
            return Err(StockError::MeasurementMismatch);
        }
        Ok(Self {
            item_id,
            quantity,
            low_stock_at,
        })
    }

    /// Identifies the catalog item whose balance this stock holds.
    pub fn item_id(&self) -> Id<Item> {
        self.item_id
    }

    /// Returns a copy without granting permission to mutate the balance.
    pub fn quantity(&self) -> StockAmount {
        self.quantity
    }

    /// Returns the configured low-stock threshold in the balance's measurement.
    pub fn low_stock_at(&self) -> StockAmount {
        self.low_stock_at
    }

    /// Low stock includes balances exactly equal to the threshold.
    pub fn is_low(&self) -> bool {
        match (self.quantity, self.low_stock_at) {
            (StockAmount::Count(a), StockAmount::Count(b)) => a <= b,
            (StockAmount::Mass { quantity: a, .. }, StockAmount::Mass { quantity: b, .. }) => {
                a.value() <= b.value()
            }
            _ => unreachable!("constructor guarantees matching measurements"),
        }
    }

    // Validated commands share this path so checks, mutation, and event creation
    // cannot drift apart between addition and removal.
    pub(super) fn apply(
        &mut self,
        item_id: Id<Item>,
        kind: MovementKind,
        amount: StockAmount,
        reason: Box<str>,
    ) -> Result<MovementEvent, StockError> {
        if item_id != self.item_id {
            return Err(StockError::ItemMismatch);
        }
        let before = self.quantity;
        let after = match kind {
            MovementKind::Added => before.checked_add(amount)?,
            MovementKind::Removed => before.checked_sub(amount)?,
        };
        // f64 may round away tiny adjustments to huge balances. Do not report a
        // positive movement as successful when the stored value did not change.
        if after == before {
            return Err(StockError::PrecisionLoss);
        }
        let event = MovementEvent {
            id: Id::new(),
            item_id,
            kind,
            amount,
            quantity_before: before,
            quantity_after: after,
            reason,
            occurred_at: Utc::now(),
        };
        // Compute everything fallible first. No error path after this assignment
        // can leave a partially applied movement.
        self.quantity = after;
        Ok(event)
    }
}
