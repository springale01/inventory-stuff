use chrono::Utc;

use crate::{Request, models::stock::Item, tools::id::Id};

#[derive(Debug, Clone)]
pub enum MovementKind {
    Received,
    Removed,
    Corrected,
}

#[derive(Debug, Clone)]
pub struct Movement {
    pub id: Id<Movement>,
    pub item_id: Id<Item>,
    pub kind: MovementKind,
    pub quantity_before: u64,
    pub quantity_after: u64,
    pub reason: String,
    pub occurred_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Correction {
    pub target: Id<Item>,
    pub counted_quantity: u64,
    pub reason: String,
}

impl Request for Correction {
    type Response = Movement;
}
