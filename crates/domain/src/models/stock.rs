use std::fmt::Debug;

use crate::{models::sku::Sku, tools::id::Id};

#[derive(Debug, Clone)]
pub struct Item {
    pub id: Id<Item>,
    pub sku: Sku,
    pub name: String,
    pub description: Option<String>,
}
#[derive(Debug, Clone)]
pub struct StockUnit {
    pub id: Id<StockUnit>,
    pub serial_number: Option<String>,
}
#[derive(Debug)]
pub struct Stock {
    item_id: Id<Item>,
    units: Box<dyn StockList>,
    low_stock_at: u64,
}

pub trait StockList: Sync + Debug {
    fn quantity(&self) -> usize;

    fn add(&mut self, things: StockContents) -> Result<(), StockError>;
    fn remove(&mut self) -> Result<StockUnit, StockError>;
    fn find(&self, id: Id<Item>) -> Result<StockUnit, StockError>;
}

pub enum StockContents {
    Bulk { quantity: u64 },
    Individual { units: Vec<StockUnit> },
}

#[derive(Debug, thiserror::Error)]
pub enum StockError {
    #[error("Stock has zero quantity")]
    ZeroQuantityAdjustment,

    #[error("Insufficient stock: available {available}, requested {requested}")]
    InsufficientStock { available: u64, requested: u64 },

    #[error("Stock quantity overflow")]
    Overflow,
}

impl Stock {
    pub fn new(id: Id<Item>, units: Box<dyn StockList>, low_stock_at: u64) -> Self {
        Self {
            item_id: id,
            units,
            low_stock_at,
        }
    }
}
