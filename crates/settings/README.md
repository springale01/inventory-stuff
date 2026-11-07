# Settings API

Import supported types directly from the crate root, for example:

```rust
use settings::{DatabaseSettings, LanguageSource, RuntimeSettings, Settings, SettingsModel};

let model = SettingsModel::default();
assert_eq!(model.language, LanguageSource::BuiltInEnglish);
let settings = Settings::new(None, DatabaseSettings::default(), RuntimeSettings::default());
assert_eq!(settings.lang().name(), "english");
```

File APIs (`SettingsIO`, `LanguageIO`, `TextFile`), serialized models, language
types, and their errors are exported from the root. Existing `io`, `langs`,
`database`, and `runtime` paths remain available. File operations return
`SettingsIoError`; runtime settings contain no file handles.

## Language dictionaries

UI code uses `TextKey` rather than English strings as lookup keys. English is
stored once in a lazily initialized hashmap. A `LanguagePack` owns a thread-safe,
dynamically dispatched `Dictionary`, so different dictionary implementations can
be selected at runtime.

```rust
use settings::{HashMapDictionary, Settings, TextKey};

let custom = HashMapDictionary::new([
    (TextKey::TuiName, "Gestion des stocks".to_owned()),
], "French");
let pack = custom.into_language_pack();
assert_eq!(pack.get(TextKey::TuiName), Some("Gestion des stocks"));
let settings = Settings::default();
settings.swap_lang(pack);
assert_eq!(settings.lang().get(TextKey::TuiName), Some("Gestion des stocks"));
```

`LanguagePack<English>` labels an English pack; `LanguagePack<CustomLanguage>`
labels a custom pack. These markers describe intent and do not validate the
dictionary contents. Both markers and the `Language` trait are public.
`HashMapDictionary::into_language_pack()` constructs a custom pack.

`LanguagePack` defaults its type parameter to `RuntimeLanguage`.
`LanguagePack::default()` provides English in a runtime pack. Use
`typed_pack.into_runtime()` to erase a marker when initializing
`Settings::new(Some(pack), database_settings, runtime_settings)`. `Settings::new(None, ...)` also
selects English. The previous `LanguageType` / `load_language` API is replaced
by typed pack construction.

`pack.get(key)` tries the selected dictionary, then English. It returns `None`
if neither contains the key. Empty strings are valid translations. Use
`pack.dict()` for raw lookups or iteration without fallback. Iteration borrows
from the dictionary and has no defined order. Runtime dictionaries own their
strings, and duplicate keys keep the last value.

When adding UI text, add a `TextKey` variant, include it in `TextKey::ALL`, and
provide its nonempty English string in `english.rs`. Keep text keys independent of translated
wording. Dictionary values currently represent plain text only.

`Settings` stores a runtime pack in `ArcSwap`, allowing `swap_lang` to accept
any language marker. `settings.lang()` returns a snapshot: keep it for a complete
render so all text uses the same language. Existing snapshots remain valid after
a swap. Share `Settings` through `Arc<Settings>` when multiple threads need it.

`settings.database_settings()` exposes connection configuration only. Defaults
are `sqlite://database/sqlite.db` and 25 maximum connections. Pool creation,
database file creation, migrations, and queries belong to persistence; constructing
settings performs no database I/O.

## Settings files and dictionary sources

`SettingsIO` owns a `TextFile` and handles TOML, validation, and missing-file
defaults. `TextFile` only handles UTF-8 file operations. A missing settings file
uses defaults without writing a file; invalid settings return an error. Missing
fields, including fields inside database/runtime tables, use their defaults.
Unknown fields are rejected to catch misspelled configuration keys.

A settings file can select built-in English:

```toml
[language]
kind = "built_in_english"

[database_settings]
url = "sqlite://database/sqlite.db"
max_connections = 25

[runtime_settings]
mpsc_channel_capacity = 100
```

Or replace the language table with a dictionary source:

```toml
[language]
kind = "file"
path = "languages/pirate.toml"
```

Relative dictionary paths are resolved against the settings file's directory,
not against the dictionary's display name. Absolute paths also work. The original
path is retained in `SettingsModel`, so saving never loses the selected source.
The old `language_pack = "English"` spelling is accepted on input; unknown
language names return errors rather than resetting unrelated settings.

Dictionary files have a display name and a table keyed by stable `TextKey` names:

```toml
name = "Pirate English"

[entries]
tui_name = "Ye Inventory"
```

Missing entries use English fallback; explicitly empty translations stay empty.
Unknown translation keys and blank display names are rejected. Dictionaries are
fully loaded before publication. `LanguagePack` serves translations and performs
no file I/O; `LanguageIO` owns its `TextFile` and parses the dictionary.

```rust,no_run
use settings::SettingsIO;

# async fn example() -> Result<(), settings::io::SettingsIoError> {
let io = SettingsIO::new(Some("config/settings.toml".into()));
let model = io.load().await?;
let active = io.build_settings(&model).await?;
assert_eq!(active.database_settings(), &model.database_settings);
io.save(&model).await?;
# Ok(())
# }
```

Keep the persisted `SettingsModel` alongside the runtime settings. There is no
conversion from a runtime dictionary's name back to a file source.

Saves write and sync a unique temporary file beside the destination, close it,
then rename it over the destination. Parent directories are created as needed.
An unsuccessful replacement leaves the old destination intact and removes the
temporary file. This avoids truncating the live file; directory metadata is not
synced, so full power-loss durability is not promised. A save already dispatched
to the blocking pool continues if its awaiting future is dropped.

## Application worker

`AppState` loads settings, opens the configured database, and owns both workers.
Its constructors are asynchronous and preserve settings/database errors.
`AppState::new(None)` uses `settings.toml`; `from_database(db, path)` accepts an
already initialized database. The standalone `SettingsWorker` and cheaply
cloneable `SettingsHandle` also remain available.

```rust,ignore
let app_state = app::AppState::new(Some("settings.toml".into())).await?;
let handle = app_state.settings_handle().clone();
let active = app_state.settings(); // Arc<Settings>, shared with the worker and UI

handle.change_language(settings::LanguageSource::File {
    path: "languages/pirate.toml".into(),
}).await?;

// Call active.lang() once per render to obtain a consistent current snapshot.
// Owned snapshots returned by change_language() keep their original pack.
handle.reload_language().await?; // Reread the current dictionary without restarting.

let mut edited = handle.model().await?;
edited.database_settings.max_connections = 10;
let restart_required = handle.update(edited).await?;
assert!(restart_required);

app_state.shutdown().await?;
// Both workers have drained. Cloned handles now reject new requests;
// the shared runtime settings and old language snapshots remain readable.
```

The worker validates and loads a candidate dictionary, persists the candidate,
then publishes it. Failed loads, validation, or saves preserve the active model
and language. A dropped reply does not cancel an accepted change. Language
changes apply live; database/pool and mailbox changes are persisted for restart.
`update()` reports whether those fields differ from startup. It does not reopen
databases or resize existing channels. Direct `swap_lang()` calls change runtime
state only; use the worker to persist a selection.
