//! Nonnegative quantities with zero-sized units and dimension-safe conversions.
//! Values use `f64`, so calculations are approximate. Exact inventory accounting
//! requires a defined integer resolution and rounding policy at persistence.
//!
//! ```
//! use domain::tools::units::{Grams, Kilograms, Quantity};
//! let weight = Quantity::<Kilograms>::new(2.5)?;
//! assert_eq!(weight.convert::<Grams>()?.value(), 2_500.0);
//! # Ok::<(), domain::tools::units::QuantityError>(())
//! ```
//!
//! Mass-to-volume conversion requires density and cannot be done implicitly:
//! ```compile_fail
//! use domain::tools::units::{Kilograms, Liters, Quantity};
//! let mass = Quantity::<Kilograms>::new(1.0).unwrap();
//! let volume = mass.convert::<Liters>();
//! ```

use std::marker::PhantomData;

mod sealed {
    pub trait Sealed {}
}

/// Sealed so all supported conversion factors remain finite and positive.
pub trait Unit: sealed::Sealed + Copy + std::fmt::Debug + Send + Sync {
    type Dimension;
    const BASE_UNITS: f64;
    const SYMBOL: &'static str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume;

macro_rules! units {
    ($dimension:ty; $($name:ident, $factor:expr, $symbol:literal);+ $(;)?) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
            pub struct $name;

            impl sealed::Sealed for $name {}

            impl Unit for $name {
                type Dimension = $dimension;
                const BASE_UNITS: f64 = $factor;
                const SYMBOL: &'static str = $symbol;
            }
        )+
    };
}

units! { Mass;
    Grams, 1.0, "g";
    Kilograms, 1_000.0, "kg";
    Pounds, 453.59237, "lb";
    Ounces, 28.349523125, "oz";
}

units! { Volume;
    Milliliters, 1.0, "mL";
    Liters, 1_000.0, "L";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum QuantityError {
    #[error("quantity must be finite and nonnegative")]
    InvalidValue,
    #[error("quantity overflow")]
    Overflow,
    #[error("quantity underflow")]
    Underflow,
    #[error("subtraction would produce a negative quantity")]
    InsufficientQuantity,
}

/// A finite, nonnegative amount in `U`. Zero is valid; receipt handlers must
/// separately enforce positive amounts. Convert units before combining values.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Quantity<U: Unit> {
    value: f64,
    unit: PhantomData<U>,
}

impl<U: Unit> Quantity<U> {
    /// Validates a measurement. Zero is allowed for balances; commands separately
    /// enforce positive adjustments. Whole-item counts use integers instead.
    pub fn new(value: f64) -> Result<Self, QuantityError> {
        if !value.is_finite() || value < 0.0 {
            return Err(QuantityError::InvalidValue);
        }
        Ok(Self {
            value: if value == 0.0 { 0.0 } else { value },
            unit: PhantomData,
        })
    }

    /// Reads the value in `U`, without converting it to a different unit.
    pub fn value(self) -> f64 {
        self.value
    }

    /// Converts within the same dimension. Kilograms to liters is a compile error.
    pub fn convert<V: Unit<Dimension = U::Dimension>>(self) -> Result<Quantity<V>, QuantityError> {
        let value = self.value * (U::BASE_UNITS / V::BASE_UNITS);
        if self.value > 0.0 && value == 0.0 {
            return Err(QuantityError::Underflow);
        }
        if !value.is_finite() {
            return Err(QuantityError::Overflow);
        }
        Quantity::<V>::new(value)
    }

    /// Adds measurements in the same unit, checking overflow.
    pub fn checked_add(self, other: Self) -> Result<Self, QuantityError> {
        let value = self.value + other.value;
        if !value.is_finite() {
            return Err(QuantityError::Overflow);
        }
        Self::new(value)
    }

    /// Subtracts without allowing a negative measurement.
    pub fn checked_sub(self, other: Self) -> Result<Self, QuantityError> {
        if other.value > self.value {
            return Err(QuantityError::InsufficientQuantity);
        }
        Self::new(self.value - other.value)
    }
}

impl<U: Unit> std::fmt::Display for Quantity<U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.value, U::SYMBOL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_take_no_storage() {
        assert_eq!(size_of::<Kilograms>(), 0);
        assert_eq!(size_of::<Quantity<Kilograms>>(), size_of::<f64>());
    }

    #[test]
    fn validates_input() {
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                Quantity::<Grams>::new(value),
                Err(QuantityError::InvalidValue)
            );
        }
        assert_eq!(Quantity::<Grams>::new(0.0).unwrap().value(), 0.0);
    }

    #[test]
    fn converts_mass_and_volume() {
        let kg = Quantity::<Kilograms>::new(2.5).unwrap();
        assert_eq!(kg.convert::<Grams>().unwrap().value(), 2_500.0);
        let pounds = Quantity::<Pounds>::new(1.0).unwrap();
        assert!((pounds.convert::<Ounces>().unwrap().value() - 16.0).abs() < 1e-12);
        let roundtrip = kg
            .convert::<Pounds>()
            .unwrap()
            .convert::<Kilograms>()
            .unwrap();
        assert!((roundtrip.value() - 2.5).abs() < 1e-12);
        let liters = Quantity::<Liters>::new(1.5).unwrap();
        assert_eq!(liters.convert::<Milliliters>().unwrap().value(), 1_500.0);
        assert_eq!(kg.to_string(), "2.5 kg");
    }

    #[test]
    fn arithmetic_checks_boundaries() {
        let grams = Quantity::<Grams>::new(10.0).unwrap();
        assert_eq!(grams.checked_add(grams).unwrap().value(), 20.0);
        assert_eq!(grams.checked_sub(grams).unwrap().value(), 0.0);
        let larger = Quantity::<Grams>::new(11.0).unwrap();
        assert_eq!(
            grams.checked_sub(larger),
            Err(QuantityError::InsufficientQuantity)
        );
        let max = Quantity::<Grams>::new(f64::MAX).unwrap();
        assert_eq!(max.checked_add(max), Err(QuantityError::Overflow));
        let max_kg = Quantity::<Kilograms>::new(f64::MAX).unwrap();
        assert_eq!(max_kg.convert::<Grams>(), Err(QuantityError::Overflow));
        let tiny = Quantity::<Grams>::new(f64::from_bits(1)).unwrap();
        assert_eq!(tiny.convert::<Kilograms>(), Err(QuantityError::Underflow));
    }
}
