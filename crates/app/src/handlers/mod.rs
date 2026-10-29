/// Makes some structs
macro_rules! dataclass {
    ($struct_name:ident; $($name:ident: $ty:ty);* $(;)?) => {
        struct $struct_name {
            $(
                pub $name: $ty,
            )+
        }
    };
}

pub(crate) mod event_handler {
    use domain::Request;

    pub(crate) struct WorkerHandle<C, E>
    where
        C: Request,
    {
        tx: tokio::sync::mpsc::Sender<Envelope<C, E>>,
    }

    impl<C, E> WorkerHandle<C, E>
    where
        C: Request,
    {
        pub(crate) fn new(tx: tokio::sync::mpsc::Sender<Envelope<C, E>>) -> Self {
            Self { tx }
        }

        pub(crate) async fn send(
            &self,
            envelope: Envelope<C, E>,
        ) -> Result<(), tokio::sync::mpsc::error::SendError<Envelope<C, E>>> {
            self.tx.send(envelope).await
        }
    }
    pub struct Envelope<R: Request, E> {
        pub req: R,
        pub reply_to: tokio::sync::oneshot::Sender<Result<R::Response, E>>,
    }
}

pub mod inventory_worker {

    use domain::{
        Request,
        models::stock::{Item, Stock, StockAmount, StockError, StockUnit},
        tools::{id::Id, snapshot::Snapshot},
    };
    use persistance::PineappleDatabase;
    use tokio::{sync::OnceCell, task::JoinHandle};

    use crate::handlers::event_handler::{Envelope, WorkerHandle};

    pub(crate) struct InventoryWorker {
        db: PineappleDatabase,
        rx: tokio::sync::mpsc::Receiver<Envelope<InventoryMessage, InventoryError>>,
    }

    impl InventoryWorker {
        pub fn new(
            db: PineappleDatabase,
            channel_capacity: Option<usize>,
        ) -> (Self, WorkerHandle<InventoryMessage, InventoryError>) {
            let (tx, rx) = tokio::sync::mpsc::channel(channel_capacity.unwrap_or(100));

            let handle = WorkerHandle::new(tx);

            (Self { db, rx }, handle)
        }

        pub fn summon(self) -> JoinHandle<()> {
            tokio::spawn(async move {
                let InventoryWorker { db, mut rx } = self;

                while let Some(thing) = rx.recv().await {
                    let Envelope { req, reply_to } = thing;

                    let result = Self::handle(&db, req).await;
                    // A caller may stop waiting after submission. The operation
                    // still completes, and the worker keeps serving its mailbox.
                    let _ = reply_to.send(result);
                }
            })
        }

        async fn handle(
            db: &PineappleDatabase,
            req: InventoryMessage,
        ) -> Result<InventoryResponse, InventoryError> {
            match req {
                InventoryMessage::Add {
                    item,
                    amount,
                    reason,
                } => {
                    let event = db.add_stock(item, amount, reason).await?;
                    Ok(InventoryResponse::AmountChanged {
                        after_amount: Snapshot::new(event.quantity_after),
                    })
                }
                InventoryMessage::Remove {
                    item,
                    amount,
                    reason,
                } => {
                    let event = db.remove_stock(item, amount, reason).await?;
                    Ok(InventoryResponse::AmountChanged {
                        after_amount: Snapshot::new(event.quantity_after),
                    })
                }
                InventoryMessage::Get { item } => {
                    let state = db
                        .get_inventory(item)
                        .await?
                        .ok_or(persistance::InventoryError::NotFound("stock"))?;
                    Ok(InventoryResponse::GottenUnits {
                        stock: Snapshot::new(state.stock),
                        units: state.units,
                    })
                }
            }
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum InventoryError {
        #[error(transparent)]
        Stock(#[from] StockError),
        #[error(transparent)]
        Persistence(persistance::InventoryError),
    }

    impl From<persistance::InventoryError> for InventoryError {
        fn from(error: persistance::InventoryError) -> Self {
            match error {
                persistance::InventoryError::Stock(error) => Self::Stock(error),
                error => Self::Persistence(error),
            }
        }
    }

    pub enum InventoryMessage {
        Add {
            item: Id<Item>,
            amount: StockAmount,
            reason: Box<str>,
        },
        Remove {
            item: Id<Item>,
            amount: StockAmount,
            reason: Box<str>,
        },
        Get {
            item: Id<Item>,
        },
    }

    pub enum InventoryResponse {
        GottenUnits {
            stock: Snapshot<Stock>,
            units: Option<Box<[StockUnit]>>,
        },
        /// Add / Remove returns this one
        AmountChanged { after_amount: Snapshot<StockAmount> },
    }

    impl Request for InventoryMessage {
        type Response = InventoryResponse; // Placeholder
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use domain::{
            models::sku::Sku,
            tools::units::{Grams, Quantity},
        };
        use persistance::TrackingMode;
        use settings::database::DatabaseSettings;
        use std::time::Duration;
        use tokio::time::timeout;

        async fn request(
            handle: &WorkerHandle<InventoryMessage, InventoryError>,
            req: InventoryMessage,
        ) -> Result<InventoryResponse, InventoryError> {
            let (reply_to, response) = tokio::sync::oneshot::channel();
            handle
                .send(Envelope { req, reply_to })
                .await
                .unwrap_or_else(|_| panic!("mailbox closed"));
            timeout(Duration::from_secs(5), response)
                .await
                .expect("worker did not reply")
                .expect("worker dropped reply")
        }

        fn mass(value: f64, piece: Option<f64>) -> StockAmount {
            StockAmount::Mass {
                quantity: Quantity::<Grams>::new(value).unwrap(),
                mass_per_piece: piece.map(|value| Quantity::new(value).unwrap()),
            }
        }

        #[tokio::test]
        async fn worker_persists_movements_replies_to_errors_and_survives_dropped_replies() {
            let db = PineappleDatabase::new(&DatabaseSettings {
                url: "sqlite::memory:".into(),
                max_connections: 1,
            })
            .await
            .unwrap();
            let item = Item {
                id: Id::new(),
                sku: Sku::new("BOLTS").unwrap(),
                name: "Bolts".into(),
                description: None,
            };
            db.create_item(&item, TrackingMode::Bulk, mass(100.0, Some(2.5)))
                .await
                .unwrap();
            let (worker, handle) = InventoryWorker::new(db.clone(), None);
            let task = worker.summon();
            let added = request(
                &handle,
                InventoryMessage::Add {
                    item: item.id,
                    amount: mass(1_000.0, None),
                    reason: "Delivery".into(),
                },
            )
            .await
            .unwrap();
            match added {
                InventoryResponse::AmountChanged { after_amount } => {
                    assert_eq!(*after_amount.value(), mass(1_000.0, Some(2.5)))
                }
                _ => panic!("expected amount response"),
            }
            assert!(matches!(
                request(
                    &handle,
                    InventoryMessage::Remove {
                        item: item.id,
                        amount: mass(2_000.0, None),
                        reason: "Too much".into()
                    }
                )
                .await,
                Err(InventoryError::Stock(StockError::InsufficientStock { .. }))
            ));
            let removed = request(
                &handle,
                InventoryMessage::Remove {
                    item: item.id,
                    amount: mass(250.0, None),
                    reason: "Sale".into(),
                },
            )
            .await
            .unwrap();
            assert!(matches!(removed, InventoryResponse::AmountChanged { .. }));

            let (reply_to, response) = tokio::sync::oneshot::channel();
            drop(response);
            handle
                .send(Envelope {
                    req: InventoryMessage::Add {
                        item: item.id,
                        amount: mass(50.0, None),
                        reason: "Caller stopped waiting".into(),
                    },
                    reply_to,
                })
                .await
                .unwrap_or_else(|_| panic!("mailbox closed"));
            match request(&handle, InventoryMessage::Get { item: item.id })
                .await
                .unwrap()
            {
                InventoryResponse::GottenUnits { stock, units } => {
                    assert_eq!(stock.value().quantity(), mass(800.0, Some(2.5)));
                    assert!(units.is_none());
                }
                _ => panic!("expected stock response"),
            }
            assert!(matches!(
                request(&handle, InventoryMessage::Get { item: Id::new() }).await,
                Err(InventoryError::Persistence(
                    persistance::InventoryError::NotFound(_)
                ))
            ));
            assert_eq!(db.list_movements(item.id, 10, 0).await.unwrap().len(), 3);

            let individual = Item {
                id: Id::new(),
                sku: Sku::new("SERIALIZED").unwrap(),
                name: "Serialized".into(),
                description: None,
            };
            db.create_item(&individual, TrackingMode::Individual, 0)
                .await
                .unwrap();
            let unit = StockUnit {
                id: Id::new(),
                serial_number: Some("SN-1".into()),
            };
            db.receive_units(individual.id, std::slice::from_ref(&unit))
                .await
                .unwrap();
            match request(
                &handle,
                InventoryMessage::Get {
                    item: individual.id,
                },
            )
            .await
            .unwrap()
            {
                InventoryResponse::GottenUnits { stock, units } => {
                    assert_eq!(stock.value().quantity(), StockAmount::Count(1));
                    assert_eq!(units.unwrap()[0].id, unit.id);
                }
                _ => panic!("expected stock response"),
            }
            drop(handle);
            timeout(Duration::from_secs(5), task)
                .await
                .expect("worker did not stop")
                .unwrap();
        }
    }
}
