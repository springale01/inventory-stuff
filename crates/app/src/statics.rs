use std::{
    path::PathBuf,
    sync::{LazyLock, OnceLock},
};

use crate::{AppError, AppState};
use tokio::{
    runtime::Runtime,
    sync::{RwLock, RwLockReadGuard},
};
static APPSTATE: OnceLock<PineappleState> = OnceLock::new();
static RUNTIME: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| Runtime::new().expect("Tokio Runtime failed to start"));

pub struct PineappleState(RwLock<Option<AppState>>);
/// Ignores the value or the error if Appstate has already been initialized
/// On Error it panics but prints out the error info before it panics
fn initialize(settings_path: Option<PathBuf>) {
    let appstate = RUNTIME.block_on(AppState::new(settings_path));

    match appstate {
        Ok(appstate) => {
            let _ = match APPSTATE.set(PineappleState(RwLock::new(Some(appstate)))) {
                Err(_) => {
                    eprintln!("Appstate set twice, the lastest one was ignored");
                }
                _ => (),
            };
        }
        Err(e) => {
            let info = e.to_string();
            panic!("{}", info)
        }
    }
}
pub fn set_statics(path: Option<PathBuf>) -> () {
    PineappleState::runtime().block_on(PineappleState::new(path))
}
impl PineappleState {
    pub async fn new(path: Option<impl Into<PathBuf>>) {
        initialize(path.map(Into::into));
    }

    pub fn runtime() -> &'static Runtime {
        &RUNTIME
    }

    pub async fn state() -> RwLockReadGuard<'static, Option<AppState>> {
        APPSTATE
            .get()
            .expect("Should have been initialized already ; - ;")
            .0
            .read()
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
    #[error("Appstate in the lock is None")]
    NoneAppState,
    #[error(transparent)]
    AppStateError(#[from] AppError),
}
