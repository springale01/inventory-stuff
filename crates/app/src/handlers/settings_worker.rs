use std::{path::PathBuf, sync::Arc};

use domain::Request;
use settings::{
    Settings,
    io::{
        SettingsIoError,
        settings_reader::{SettingsIO, SettingsModel},
    },
    langs::{LanguagePack, LanguageSource},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use super::event_handler::{Envelope, WorkerHandle};

/// Serializes settings changes. Call `summon` before sending requests through the
/// returned handle. Drop all handles and await the task to drain accepted work.
pub struct SettingsWorker {
    settings: Arc<Settings>,
    model: SettingsModel,
    io: SettingsIO,
    mailbox: mpsc::Receiver<Envelope<SettingsMessage, SettingsError>>,
}

impl SettingsWorker {
    pub async fn new(path: Option<PathBuf>) -> Result<(Self, SettingsHandle), SettingsError> {
        let io = SettingsIO::new(path);
        let model = io.load().await?;
        let settings = Arc::new(io.build_settings(&model).await?);
        let (tx, mailbox) = mpsc::channel(settings.runtime_settings().mpsc_channel_capacity());
        let handle = SettingsHandle {
            worker: Arc::new(WorkerHandle::new(tx)),
            settings: Arc::clone(&settings),
        };
        Ok((
            Self {
                settings,
                model,
                io,
                mailbox,
            },
            handle,
        ))
    }

    pub fn summon(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(Envelope { req, reply_to }) = self.mailbox.recv().await {
                let result = self.handle(req).await;
                // Submitted changes complete even if their caller stops waiting.
                let _ = reply_to.send(result);
            }
        })
    }

    async fn handle(
        &mut self,
        request: SettingsMessage,
    ) -> Result<SettingsResponse, SettingsError> {
        match request {
            SettingsMessage::Close => {
                self.mailbox.close();
                Ok(SettingsResponse::Closing)
            }
            SettingsMessage::Get => Ok(SettingsResponse::Model(self.model.clone())),
            SettingsMessage::ChangeLanguage(source) => {
                let mut candidate = self.model.clone();
                candidate.language = source;
                self.commit(candidate).await?;
                Ok(SettingsResponse::Language(
                    self.settings.language_snapshot(),
                ))
            }
            SettingsMessage::ReloadLanguage => {
                let pack = self.io.load_language(&self.model.language).await?;
                self.settings.swap_lang(pack);
                Ok(SettingsResponse::Language(
                    self.settings.language_snapshot(),
                ))
            }
            SettingsMessage::Update(candidate) => {
                let restart_required = self.commit(candidate).await?;
                Ok(SettingsResponse::Updated { restart_required })
            }
        }
    }

    async fn commit(&mut self, candidate: SettingsModel) -> Result<bool, SettingsError> {
        candidate.validate()?;
        let pack = self.io.load_language(&candidate.language).await?;
        let restart_required = candidate.database_settings != *self.settings.database_settings()
            || candidate.runtime_settings != *self.settings.runtime_settings();
        // Nothing is published until both loading and saving have succeeded.
        self.io.save(&candidate).await?;
        self.model = candidate;
        self.settings.swap_lang(pack);
        Ok(restart_required)
    }
}

/// Cheaply cloneable access to the worker and its shared active runtime settings.
#[derive(Clone)]
pub struct SettingsHandle {
    worker: Arc<WorkerHandle<SettingsMessage, SettingsError>>,
    settings: Arc<Settings>,
}

impl SettingsHandle {
    /// Stops new submissions while allowing already accepted messages to drain.
    pub(crate) async fn close(&self) -> Result<(), SettingsError> {
        match self.request(SettingsMessage::Close).await? {
            SettingsResponse::Closing => Ok(()),
            _ => Err(SettingsError::UnexpectedResponse),
        }
    }

    /// Languages change live. Database/pool and mailbox settings remain those
    /// from startup; persisted changes take effect when resources are recreated.
    pub fn settings(&self) -> Arc<Settings> {
        Arc::clone(&self.settings)
    }

    pub async fn model(&self) -> Result<SettingsModel, SettingsError> {
        match self.request(SettingsMessage::Get).await? {
            SettingsResponse::Model(model) => Ok(model),
            _ => Err(SettingsError::UnexpectedResponse),
        }
    }

    /// Validates and saves the selection, then publishes the language.
    /// The returned pack is a snapshot, not a subscription to future changes.
    pub async fn change_language(
        &self,
        source: LanguageSource,
    ) -> Result<Arc<LanguagePack>, SettingsError> {
        match self
            .request(SettingsMessage::ChangeLanguage(source))
            .await?
        {
            SettingsResponse::Language(pack) => Ok(pack),
            _ => Err(SettingsError::UnexpectedResponse),
        }
    }

    /// Rereads the selected dictionary. A failed reload leaves the active pack intact.
    pub async fn reload_language(&self) -> Result<Arc<LanguagePack>, SettingsError> {
        match self.request(SettingsMessage::ReloadLanguage).await? {
            SettingsResponse::Language(pack) => Ok(pack),
            _ => Err(SettingsError::UnexpectedResponse),
        }
    }

    /// Saves a complete model and applies its language. Returns whether database
    /// or mailbox configuration differs from startup and requires a restart.
    pub async fn update(&self, model: SettingsModel) -> Result<bool, SettingsError> {
        match self.request(SettingsMessage::Update(model)).await? {
            SettingsResponse::Updated { restart_required } => Ok(restart_required),
            _ => Err(SettingsError::UnexpectedResponse),
        }
    }

    async fn request(&self, req: SettingsMessage) -> Result<SettingsResponse, SettingsError> {
        let (reply_to, response) = oneshot::channel();
        self.worker
            .send(Envelope { req, reply_to })
            .await
            .map_err(|_| SettingsError::WorkerUnavailable)?;
        response.await.map_err(|_| SettingsError::ResponseDropped)?
    }
}

pub(crate) enum SettingsMessage {
    Close,
    Get,
    ChangeLanguage(LanguageSource),
    ReloadLanguage,
    Update(SettingsModel),
}

pub(crate) enum SettingsResponse {
    Closing,
    Model(SettingsModel),
    Language(Arc<LanguagePack>),
    Updated { restart_required: bool },
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Configuration(#[from] SettingsIoError),
    #[error("Settings worker is no longer accepting requests")]
    WorkerUnavailable,
    #[error("Settings worker stopped before replying; the change may have completed")]
    ResponseDropped,
    #[error("Settings worker returned an unexpected response")]
    UnexpectedResponse,
}

impl Request for SettingsMessage {
    type Response = SettingsResponse;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropped_reply_does_not_cancel_change_and_shutdown_drains_mailbox() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "inventory-settings-drain-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("settings.toml");
        let (worker, handle) = SettingsWorker::new(Some(path.clone())).await.unwrap();
        let (reply_to, response) = oneshot::channel();
        drop(response);
        let mut candidate = SettingsModel::default();
        candidate.database_settings.max_connections = 4;
        handle
            .worker
            .send(Envelope {
                req: SettingsMessage::Update(candidate.clone()),
                reply_to,
            })
            .await
            .unwrap_or_else(|_| panic!("mailbox closed"));
        // Queue the change before starting, then close the mailbox. The worker
        // must process accepted messages rather than exiting on sender closure.
        drop(handle);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker.summon())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(SettingsIO::new(Some(path)).load().await.unwrap(), candidate);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
