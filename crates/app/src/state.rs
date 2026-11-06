use std::{path::PathBuf, sync::Arc};

use domain::{
    models::{
        sku::Sku,
        stock::{Item, Stock, StockAmount, StockUnit},
    },
    tools::{id::Id, snapshot::Snapshot},
};
use persistance::PineappleDatabase;
use settings::Settings;
use tokio::{sync::oneshot, task::JoinHandle};

use crate::{
    SettingsError, SettingsHandle, SettingsWorker,
    handlers::{
        event_handler::{Envelope, WorkerHandle},
        inventory_worker::{self, InventoryMessage, InventoryResponse, InventoryWorker},
    },
};

/// Application use cases backed by inventory and settings workers.
///
/// Construction requires a Tokio runtime. Use [`Self::shutdown`] to close both
/// mailboxes and wait for accepted work, including when settings handles are cloned.

pub struct AppState {
    pub(crate) handles: Handles,
    tasks: WorkerTasks,
}

struct WorkerTasks {
    inventory: JoinHandle<()>,
    settings: JoinHandle<()>,
}

impl AppState {
    /// Loads settings and its selected dictionary, opens the configured database,
    /// applies migrations, then starts both workers. `None` uses `settings.toml`.
    pub async fn new(settings_path: Option<PathBuf>) -> Result<Self, AppError> {
        let (worker, handle) = SettingsWorker::new(settings_path).await?;
        let db = PineappleDatabase::new(handle.settings().database_settings()).await?;
        Ok(Self::start(db, worker, handle))
    }

    /// Loads settings and starts both workers using an initialized database.
    /// The supplied database overrides the file's database configuration for this
    /// run; existing database clones continue to share the same connection pool.
    pub async fn from_database(
        db: PineappleDatabase,
        settings_path: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        let (worker, handle) = SettingsWorker::new(settings_path).await?;
        Ok(Self::start(db, worker, handle))
    }

    /// Starts from the default settings path, preserving initialization errors.
    pub async fn from_database_default() -> Result<Self, AppError> {
        Self::new(None).await
    }

    fn start(
        db: PineappleDatabase,
        worker_settings: SettingsWorker,
        settings_handle: SettingsHandle,
    ) -> Self {
        let capacity = settings_handle
            .settings()
            .runtime_settings()
            .mpsc_channel_capacity();
        let (worker_inventory, inventory_handle) = InventoryWorker::new(db, Some(capacity));
        Self {
            handles: Handles {
                inventory_handle,
                settings_handle,
            },
            tasks: WorkerTasks {
                inventory: worker_inventory.summon(),
                settings: worker_settings.summon(),
            },
        }
    }

    /// Shared active settings. Load `lang()` once per render for a consistent snapshot.
    pub fn settings(&self) -> Arc<Settings> {
        self.handles.settings_handle.settings()
    }

    /// Access settings edits and reloads. Clones stop accepting requests at shutdown.
    pub fn settings_handle(&self) -> &SettingsHandle {
        &self.handles.settings_handle
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

    /// Resolves an SKU to its catalog item; a missing SKU is an inventory error.
    pub async fn get_item_by_sku(&self, sku: Sku) -> Result<Item, AppError> {
        match self.request(InventoryMessage::GetBySku { sku }).await? {
            InventoryResponse::SkuItem { item } => Ok(item),
            _ => Err(AppError::UnexpectedResponse),
        }
    }

    async fn request(&self, req: InventoryMessage) -> Result<InventoryResponse, AppError> {
        let (reply_to, response) = oneshot::channel();
        self.handles
            .inventory_handle
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
    /// Closes both mailboxes and waits for all accepted work, even if one task
    /// fails. Cloned settings handles cannot keep shutdown open. Shared database
    /// pools stay open; once submitted, work completes if its caller stops waiting.
    pub async fn shutdown(self) -> Result<(), AppError> {
        let Self { handles, tasks } = self;
        let closing = handles.settings_handle.close().await;
        drop(handles);
        let (inventory, settings) = tokio::join!(tasks.inventory, tasks.settings);
        inventory.map_err(|source| AppError::WorkerTask {
            worker_name: "inventory",
            source,
        })?;
        settings.map_err(|source| AppError::WorkerTask {
            worker_name: "settings",
            source,
        })?;
        closing.map_err(AppError::Settings)
    }
}

#[derive(Debug, Clone)]
pub struct InventorySnapshot {
    pub stock: Snapshot<Stock>,
    /// `None` for bulk stock; `Some([])` for empty individually tracked stock.
    pub units: Option<Box<[StockUnit]>>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Database(#[from] persistance::DatabaseError),
    #[error(transparent)]
    Inventory(#[from] inventory_worker::InventoryError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error("{worker_name} worker is no longer accepting requests")]
    WorkerUnavailable { worker_name: &'static str },
    #[error("Inventory worker stopped before replying; the operation may have completed")]
    ResponseDropped,
    #[error("Inventory worker returned an unexpected response")]
    UnexpectedResponse,
    #[error("{worker_name} worker task failed: {source}")]
    WorkerTask {
        worker_name: &'static str,
        #[source]
        source: tokio::task::JoinError,
    },
}

pub(crate) struct Handles {
    // we can do a Vec later on if we need scaling
    pub(crate) inventory_handle:
        WorkerHandle<inventory_worker::InventoryMessage, inventory_worker::InventoryError>,
    pub(crate) settings_handle: SettingsHandle,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

    async fn test_state() -> (AppState, PineappleDatabase) {
        let db = PineappleDatabase::new(&settings::database::DatabaseSettings {
            url: "sqlite::memory:".into(),
            max_connections: 1,
        })
        .await
        .unwrap();
        // This path is intentionally absent. Loading/closing default settings
        // performs no writes, so these tests never touch a real settings file.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "inventory-state-{}-{stamp}.toml",
            std::process::id()
        ));
        (
            AppState::from_database(db.clone(), Some(path))
                .await
                .unwrap(),
            db,
        )
    }

    #[tokio::test]
    async fn shutdown_drains_accepted_inventory_work() {
        let (state, db) = test_state().await;
        let item = Item {
            id: Id::new(),
            sku: Sku::new("QUEUED").unwrap(),
            name: "Queued".into(),
            description: None,
        };
        db.create_item(&item, persistance::TrackingMode::Bulk, 0)
            .await
            .unwrap();
        let (reply_to, response) = oneshot::channel();
        state
            .handles
            .inventory_handle
            .send(Envelope {
                req: InventoryMessage::Add {
                    item: item.id,
                    amount: 5.into(),
                    reason: "Delivery".into(),
                },
                reply_to,
            })
            .await
            .unwrap_or_else(|_| panic!("mailbox closed"));
        tokio::time::timeout(Duration::from_secs(5), state.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            response.await.unwrap().unwrap(),
            InventoryResponse::AmountChanged { .. }
        ));
        assert_eq!(
            db.get_inventory(item.id)
                .await
                .unwrap()
                .unwrap()
                .stock
                .quantity(),
            StockAmount::Count(5)
        );
    }

    #[tokio::test]
    async fn shutdown_identifies_failed_task_and_still_joins_the_other_worker() {
        for failed_worker in ["inventory", "settings"] {
            let (mut state, _) = test_state().await;
            let settings_handle = state.settings_handle().clone();
            let finished = Arc::new(AtomicBool::new(false));
            let signal = Arc::clone(&finished);
            let WorkerTasks {
                inventory,
                settings,
            } = state.tasks;
            state.tasks = if failed_worker == "inventory" {
                inventory.abort();
                WorkerTasks {
                    inventory,
                    settings: tokio::spawn(async move {
                        settings.await.unwrap();
                        tokio::task::yield_now().await;
                        signal.store(true, Ordering::SeqCst);
                    }),
                }
            } else {
                settings.abort();
                WorkerTasks {
                    settings,
                    inventory: tokio::spawn(async move {
                        inventory.await.unwrap();
                        tokio::task::yield_now().await;
                        signal.store(true, Ordering::SeqCst);
                    }),
                }
            };
            let result = tokio::time::timeout(Duration::from_secs(5), state.shutdown())
                .await
                .unwrap();
            assert!(
                matches!(result, Err(AppError::WorkerTask { worker_name, source })
                if worker_name == failed_worker && source.is_cancelled())
            );
            assert!(finished.load(Ordering::SeqCst));
            assert!(matches!(
                settings_handle.model().await,
                Err(SettingsError::WorkerUnavailable)
            ));
        }
    }
}
