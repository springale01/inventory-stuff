# Application API

`app` owns application operations, worker handles, and startup/shutdown. Import its
public types from the crate root. Worker envelopes and request/response enums are
implementation details. `domain`, `settings`, and `persistance` are re-exported
as namespaces so consumers can name the values in application APIs.

```rust,no_run
use app::AppState;
use app::domain::{Id, Item};

# async fn example() -> Result<(), app::AppError> {
let app = AppState::new(Some("config/settings.toml".into())).await?;
let inventory = app.inventory_handle().clone();
let settings = app.settings_handle().clone();
let active = app.settings();

// Reads do not require holding an AppState borrow or a global lock guard.
let item = Id::<Item>::new();
let result = inventory.get_inventory(item).await;
let model = settings.model().await?;
let language = active.language_snapshot();

app.shutdown().await?;
// Handle clones now reject requests; language/settings snapshots remain readable.
# Ok(())
# }
```

`AppState::new(path)` loads one settings model and dictionary, initializes the
configured database, then starts both workers. `None` uses `settings.toml`.
`AppState::from_database(db, path)` uses an existing database instead, while still
loading settings. That injected database overrides the file's connection
configuration for the run. Database clones retain their shared pool.

`InventoryHandle` supports the same inventory operations as `AppState`.
`SettingsHandle` reads/updates the saved model, changes language, and reloads
dictionary files. It publishes a new language only after validation and saving
succeed. Database/pool and mailbox configuration changes require resource
recreation; `SettingsHandle::update` reports whether a restart is required.

Application operations return `AppError`. Domain failures remain available as
`InventoryError::Stock(domain::StockError)`; settings operations return
`SettingsError`, wrapping `settings::SettingsIoError` for validation or file
failures. Errors are propagated rather than silently replacing configuration.

## Optional global access

The `statics` module supports a process-wide app for UI code that prefers globals.
Its entry points and `PineappleState` are also exported at the crate root.

```rust,no_run
use app::{PineappleState, set_statics, shutdown_statics};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
set_statics(Some("settings.toml".into()))?;
let inventory = PineappleState::runtime()
    .block_on(PineappleState::inventory_handle())?;
let settings = PineappleState::runtime()
    .block_on(PineappleState::settings())?;

// Run the synchronous terminal loop while the shared runtime serves workers.
// Restore the terminal before reporting errors, and shut down even on UI errors.
shutdown_statics()?;
# Ok(())
# }
```

Async entry points use `app::initialize(path).await` and
`PineappleState::shutdown_global().await` on the caller's runtime. Keep that runtime
alive until shutdown. Calling the synchronous startup/shutdown wrappers inside a
Tokio runtime returns `StaticsError::AsyncContext` instead of nesting `block_on`.
The TUI executable uses these async entry points and awaits `tui::run()` between them.

Initialization is once per process. Failed startup leaves the slot available for
retry. Repeated/concurrent initialization returns `AlreadyInitialized` without
starting duplicate workers; reinitialization after shutdown is not supported.
Uninitialized and shut-down access return explicit errors.

`PineappleState::state().await?` returns a guard directly to a live `AppState`.
Release it before shutdown. Prefer `inventory_handle()`, `settings_handle()`, or
`settings()` when retaining access beyond a single operation: they return owned
clones and do not keep the global read lock alive.

For standalone settings work, `SettingsWorker::new` returns a worker and handle.
Call `summon` before sending requests. Close the handle and await the task, or drop
all handle clones and await the task. In an app, `AppState` manages this lifecycle.
