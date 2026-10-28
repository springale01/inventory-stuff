

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

mod event_handler {
    use domain::Request;

    pub(crate) struct WorkerHandle<C>
    where C: Request {
        tx: tokio::sync::mpsc::Sender<Envelope<C>>,
    }

    impl<C> WorkerHandle<C>
    where C: Request {
        pub(crate) fn new(
            tx: tokio::sync::mpsc::Sender<C>,
        ) -> Self {
            Self { tx }
        }

        pub(crate) async fn send(
            &self,
            envelope: Envelope<C>
        ) -> Result<(), tokio::sync::mpsc::error::SendError<Envelope<C>>> {
            self.tx.send(envelope)
            .await
            
        }
    }

    pub struct Envelope<R: Request> {
        pub req: R,
        pub reply_to: tokio::sync::oneshot::Sender<R::Response>
    }
}

mod inventory_worker {
    use std::sync::Arc;

use persistance::PineappleDatabase;
use settings::Settings;

use crate::handlers::event_handler::WorkerHandle;

    pub(crate) struct InvetoryWorker {
        db: PineappleDatabase,
        rx: tokio::sync::mpsc::Receiver<InventoryCommand>,

        settings: Arc<Settings>
    }

    impl InvetoryWorker {
        pub async fn new(db: PineappleDatabase, settings: Arc<Settings>) -> WorkerHandle<InventoryCommand> {
            let (tx, rx) = tokio::sync::mpsc::channel(settings.runtime_settings().mpsc_channel_capacity());

            let handle = WorkerHandle
        }
    }

}