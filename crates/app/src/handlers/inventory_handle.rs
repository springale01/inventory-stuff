use std::sync::Arc;

use domain::{Id, Item, Sku, Snapshot, StockAmount};
use tokio::sync::oneshot;

use super::{
    event_handler::{Envelope, WorkerHandle},
    inventory_worker::{InventoryError, InventoryMessage, InventoryResponse},
};
use crate::{AppError, InventorySnapshot};

/// Cheaply cloneable inventory access. Clones share the app's existing worker.
/// `AppState::shutdown` closes its mailbox even while cloned handles exist.
#[derive(Clone)]
pub struct InventoryHandle {
    worker: Arc<WorkerHandle<InventoryMessage, InventoryError>>,
}

impl InventoryHandle {
    pub(crate) fn new(worker: WorkerHandle<InventoryMessage, InventoryError>) -> Self {
        Self {
            worker: Arc::new(worker),
        }
    }

    /// Adds stock and returns the persisted balance after the movement.
    pub async fn add_stock(
        &self,
        item: Id<Item>,
        amount: impl Into<StockAmount>,
        reason: impl Into<Box<str>>,
    ) -> Result<Snapshot<StockAmount>, AppError> {
        match self
            .request(InventoryMessage::Add {
                item,
                amount: amount.into(),
                reason: reason.into(),
            })
            .await?
        {
            InventoryResponse::AmountChanged { after_amount } => Ok(after_amount),
            _ => Err(AppError::UnexpectedResponse),
        }
    }

    /// Removes stock and returns the persisted balance after the movement.
    pub async fn remove_stock(
        &self,
        item: Id<Item>,
        amount: impl Into<StockAmount>,
        reason: impl Into<Box<str>>,
    ) -> Result<Snapshot<StockAmount>, AppError> {
        match self
            .request(InventoryMessage::Remove {
                item,
                amount: amount.into(),
                reason: reason.into(),
            })
            .await?
        {
            InventoryResponse::AmountChanged { after_amount } => Ok(after_amount),
            _ => Err(AppError::UnexpectedResponse),
        }
    }

    /// Reads stock and its individual units from one consistent database view.
    pub async fn get_inventory(&self, item: Id<Item>) -> Result<InventorySnapshot, AppError> {
        match self.request(InventoryMessage::Get { item }).await? {
            InventoryResponse::GottenUnits { stock, units } => {
                Ok(InventorySnapshot { stock, units })
            }
            _ => Err(AppError::UnexpectedResponse),
        }
    }

    pub async fn get_item_by_sku(&self, sku: Sku) -> Result<Item, AppError> {
        match self.request(InventoryMessage::GetBySku { sku }).await? {
            InventoryResponse::SkuItem { item } => Ok(item),
            _ => Err(AppError::UnexpectedResponse),
        }
    }

    pub(crate) async fn close(&self) -> Result<(), AppError> {
        match self.request(InventoryMessage::Close).await? {
            InventoryResponse::Closing => Ok(()),
            _ => Err(AppError::UnexpectedResponse),
        }
    }

    async fn request(&self, req: InventoryMessage) -> Result<InventoryResponse, AppError> {
        let (reply_to, response) = oneshot::channel();
        self.worker
            .send(Envelope { req, reply_to })
            .await
            .map_err(|_| AppError::WorkerUnavailable {
                worker_name: "inventory",
            })?;
        response
            .await
            .map_err(|_| AppError::ResponseDropped)?
            .map_err(AppError::Inventory)
    }

    #[cfg(test)]
    pub(crate) async fn send(
        &self,
        envelope: Envelope<InventoryMessage, InventoryError>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Envelope<InventoryMessage, InventoryError>>>
    {
        self.worker.send(envelope).await
    }
}
