#![doc = include_str!("../README.md")]

use std::pin::Pin;

pub mod models;
pub mod tools;

pub use models::{
    movement::{AddCmd, MovementEvent, MovementKind, RemoveCmd},
    sku::{
        CounterExhausted, CounterSkuBuilder, CounterSkuSupplier, InvalidSku, Sku, SkuBuilder,
        SkuFamily, SkuSupplier, SkuType,
    },
    stock::{Item, Stock, StockAmount, StockError, StockUnit},
};
pub use tools::{
    id::Id,
    snapshot::Snapshot,
    units::{
        Grams, Kilograms, Liters, Mass, Milliliters, Ounces, Pounds, Quantity, QuantityError, Unit,
        Volume,
    },
};

/// A request declares its successful response; execution errors are separate.
pub trait Request {
    type Response: Send;
}
/// An asynchronous request handler. The returned future may borrow the handler.
pub trait Handle<T>
where
    T: Request,
{
    type Error: Send;
    type Context;

    fn execute(
        &self,
        ctx: Self::Context,
        req: T,
    ) -> Pin<Box<dyn Future<Output = Result<T::Response, Self::Error>> + Send + '_>>;
}
