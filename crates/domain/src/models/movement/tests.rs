//! Behavior tests: follow the public commands rather than private helper methods.

use super::{AddCmd, MovementKind, RemoveCmd};
use crate::{
    models::stock::{Item, Stock, StockAmount, StockError},
    tools::{
        id::Id,
        units::{Grams, Quantity, QuantityError},
    },
};

fn grams(value: f64) -> Quantity<Grams> {
    Quantity::new(value).unwrap()
}

#[test]
fn count_movements_record_balances_and_cross_the_low_stock_threshold() {
    let id = Id::<Item>::new();
    let mut stock = Stock::new(id, 0_u64, 2_u64).unwrap();
    assert!(stock.is_low());
    let added = AddCmd::new(id, 12_u64, "Delivery")
        .unwrap()
        .execute(&mut stock)
        .unwrap();
    assert_eq!(added.item_id, id);
    assert_eq!(added.kind, MovementKind::Added);
    assert_eq!(added.amount, StockAmount::Count(12));
    assert_eq!(added.quantity_before, StockAmount::Count(0));
    assert_eq!(added.quantity_after, stock.quantity());
    assert!(!stock.is_low());

    let removed = RemoveCmd::new(id, 10_u64, "Sale")
        .unwrap()
        .execute(&mut stock)
        .unwrap();
    assert_eq!(removed.kind, MovementKind::Removed);
    assert_eq!(removed.quantity_before, StockAmount::Count(12));
    assert_eq!(removed.quantity_after, StockAmount::Count(2));
    assert_eq!(&*removed.reason, "Sale");
    assert_ne!(added.id, removed.id);
    assert!(stock.is_low()); // Equality with the threshold is deliberately low.
}

#[test]
fn fractional_mass_can_be_received_removed_to_zero_and_received_again() {
    let id = Id::<Item>::new();
    let mut stock = Stock::new(id, grams(1_000.0), grams(100.0)).unwrap();
    let event = RemoveCmd::new(id, grams(250.5), "Weighed sale")
        .unwrap()
        .execute(&mut stock)
        .unwrap();
    assert_eq!(event.quantity_after, StockAmount::Mass(grams(749.5)));
    RemoveCmd::new(id, grams(749.5), "Remaining sale")
        .unwrap()
        .execute(&mut stock)
        .unwrap();
    assert_eq!(stock.quantity(), StockAmount::Mass(grams(0.0)));
    // A zero mass balance is still mass, never an untyped zero or a count.
    assert_eq!(
        AddCmd::new(id, 1_u64, "Wrong unit")
            .unwrap()
            .execute(&mut stock)
            .unwrap_err(),
        StockError::MeasurementMismatch
    );
    AddCmd::new(id, grams(0.5), "Delivery")
        .unwrap()
        .execute(&mut stock)
        .unwrap();
    assert_eq!(stock.quantity(), StockAmount::Mass(grams(0.5)));
}

#[test]
fn invalid_commands_are_rejected_before_execution() {
    let id = Id::<Item>::new();
    assert_eq!(
        AddCmd::new(id, 0_u64, "Delivery").unwrap_err(),
        StockError::ZeroQuantityAdjustment
    );
    assert_eq!(
        RemoveCmd::new(id, grams(0.0), "Sale").unwrap_err(),
        StockError::ZeroQuantityAdjustment
    );
    assert_eq!(
        AddCmd::new(id, 1_u64, " \t\n").unwrap_err(),
        StockError::EmptyReason
    );
    assert_eq!(
        RemoveCmd::new(id, 1_u64, "").unwrap_err(),
        StockError::EmptyReason
    );
}

#[test]
fn failed_execution_never_changes_the_balance() {
    let id = Id::<Item>::new();
    let mut stock = Stock::new(id, 5_u64, 0_u64).unwrap();
    let cases = [
        (
            RemoveCmd::new(Id::new(), 1_u64, "Wrong item").unwrap(),
            StockError::ItemMismatch,
        ),
        (
            RemoveCmd::new(id, grams(1.0), "Wrong unit").unwrap(),
            StockError::MeasurementMismatch,
        ),
        (
            RemoveCmd::new(id, 6_u64, "Too much").unwrap(),
            StockError::InsufficientStock {
                available: StockAmount::Count(5),
                requested: StockAmount::Count(6),
            },
        ),
    ];
    for (cmd, expected) in cases {
        assert_eq!(cmd.execute(&mut stock).unwrap_err(), expected);
        assert_eq!(stock.quantity(), StockAmount::Count(5));
    }
}

#[test]
fn large_counts_stay_exact_and_overflow_is_rejected() {
    let id = Id::<Item>::new();
    let mut stock = Stock::new(id, u64::MAX, 0_u64).unwrap();
    assert_eq!(
        AddCmd::new(id, 1_u64, "Overflow")
            .unwrap()
            .execute(&mut stock)
            .unwrap_err(),
        StockError::Overflow
    );
    assert_eq!(stock.quantity(), StockAmount::Count(u64::MAX));
    RemoveCmd::new(id, 1_u64, "Sale")
        .unwrap()
        .execute(&mut stock)
        .unwrap();
    assert_eq!(stock.quantity(), StockAmount::Count(u64::MAX - 1));
}

#[test]
fn mass_failures_preserve_the_balance() {
    let id = Id::<Item>::new();
    let mut stock = Stock::new(id, grams(f64::MAX), grams(0.0)).unwrap();
    assert_eq!(
        AddCmd::new(id, grams(f64::MAX), "Overflow")
            .unwrap()
            .execute(&mut stock)
            .unwrap_err(),
        StockError::Quantity(QuantityError::Overflow)
    );
    assert_eq!(
        RemoveCmd::new(id, grams(1.0), "Too small")
            .unwrap()
            .execute(&mut stock)
            .unwrap_err(),
        StockError::PrecisionLoss
    );
    assert_eq!(
        AddCmd::new(id, grams(1.0), "Too small")
            .unwrap()
            .execute(&mut stock)
            .unwrap_err(),
        StockError::PrecisionLoss
    );
    assert_eq!(stock.quantity(), StockAmount::Mass(grams(f64::MAX)));

    let mut stock = Stock::new(id, grams(0.5), grams(0.0)).unwrap();
    assert!(matches!(
        RemoveCmd::new(id, grams(1.0), "Too much")
            .unwrap()
            .execute(&mut stock),
        Err(StockError::InsufficientStock { .. })
    ));
    assert_eq!(stock.quantity(), StockAmount::Mass(grams(0.5)));
}

#[test]
fn threshold_must_match_the_balance_measurement() {
    assert_eq!(
        Stock::new(Id::new(), grams(0.0), 0_u64).unwrap_err(),
        StockError::MeasurementMismatch
    );
}
