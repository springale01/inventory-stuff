//! Optional process-wide access to the application.
//!
//! Synchronous entry points use the shared runtime. Async callers should use
//! `initialize` and `PineappleState::shutdown_global` on their existing runtime.
//! Initialization is once per process; failed startup can be retried, but shutdown
//! is terminal. Release state read guards before requesting shutdown.

use std::{
    path::PathBuf,
    sync::{Arc, LazyLock, OnceLock},
};

use tokio::{
    runtime::Runtime,
    sync::{Mutex, RwLock, RwLockReadGuard},
};

use crate::{AppError, AppState, InventoryHandle, SettingsHandle};
use settings::Settings;

static APPSTATE: OnceLock<PineappleState> = OnceLock::new();
static INITIALIZATION: Mutex<()> = Mutex::const_new(());
static RUNTIME: LazyLock<Runtime> =
    LazyLock::new(|| Runtime::new().expect("Tokio runtime failed to start"));

/// A global application's lifetime, including the empty state after shutdown.
pub struct PineappleState(RwLock<Option<AppState>>);

/// Initializes global state on the caller's runtime. Concurrent initializers are
/// serialized; repeated initialization is an error and never creates extra workers.
pub async fn initialize(settings_path: Option<PathBuf>) -> Result<(), StaticsError> {
    let _initialization = INITIALIZATION.lock().await;
    if APPSTATE.get().is_some() {
        return Err(StaticsError::AlreadyInitialized);
    }
    let app = AppState::new(settings_path).await?;
    if let Err(extra) = APPSTATE.set(PineappleState(RwLock::new(Some(app)))) {
        // Keep cleanup explicit even if the initialization invariant is changed.
        if let Some(app) = extra.0.into_inner() {
            app.shutdown().await?;
        }
        return Err(StaticsError::AlreadyInitialized);
    }
    Ok(())
}

/// Synchronous startup for a terminal entry point. Async callers use `initialize`.
pub fn set_statics(path: Option<PathBuf>) -> Result<(), StaticsError> {
    ensure_sync_context()?;
    RUNTIME.block_on(initialize(path))
}

/// Synchronous shutdown, including draining both workers. Call after restoring
/// the terminal, even when the UI returned an error.
pub fn shutdown_statics() -> Result<(), StaticsError> {
    ensure_sync_context()?;
    RUNTIME.block_on(PineappleState::shutdown_global())
}

fn ensure_sync_context() -> Result<(), StaticsError> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(StaticsError::AsyncContext);
    }
    Ok(())
}

impl PineappleState {
    /// Compatibility entry point. Prefer the explicitly named `initialize`.
    #[deprecated(
        note = "use app::initialize(path).await; global initialization does not construct a value"
    )]
    pub async fn new(path: Option<PathBuf>) -> Result<(), StaticsError> {
        initialize(path).await
    }

    pub fn runtime() -> &'static Runtime {
        &RUNTIME
    }

    /// A guard to a live app; absence is reported as an error rather than exposed
    /// as an Option or panic. Drop this guard before awaiting shutdown.
    pub async fn state() -> Result<RwLockReadGuard<'static, AppState>, StaticsError> {
        let state = APPSTATE.get().ok_or(StaticsError::NotInitialized)?;
        let guard = state.0.read().await;
        RwLockReadGuard::try_map(guard, Option::as_ref).map_err(|_| StaticsError::NoneAppState)
    }

    /// Owned shared settings, without keeping a global read guard alive.
    pub async fn settings() -> Result<Arc<Settings>, StaticsError> {
        Ok(Self::state().await?.settings())
    }

    /// An owned worker handle for background/UI requests without a global guard.
    pub async fn settings_handle() -> Result<SettingsHandle, StaticsError> {
        Ok(Self::state().await?.settings_handle().clone())
    }

    pub async fn inventory_handle() -> Result<InventoryHandle, StaticsError> {
        Ok(Self::state().await?.inventory_handle().clone())
    }

    /// Takes the app out of global storage, releases the lock, then drains workers.
    pub async fn shutdown_global() -> Result<(), StaticsError> {
        APPSTATE
            .get()
            .ok_or(StaticsError::NotInitialized)?
            .shutdown()
            .await
    }

    pub async fn shutdown(&self) -> Result<(), StaticsError> {
        let app = {
            let mut guard = self.0.write().await;
            guard.take().ok_or(StaticsError::NoneAppState)?
        };
        app.shutdown().await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StaticsError {
    #[error("Application has not been initialized")]
    NotInitialized,
    #[error("Application has already been initialized; global initialization is once per process")]
    AlreadyInitialized,
    #[error("Application has shut down")]
    NoneAppState,
    #[error("Use the async initialization/shutdown API inside a Tokio runtime")]
    AsyncContext,
    #[error(transparent)]
    AppStateError(#[from] AppError),
}
