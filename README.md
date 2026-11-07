# Inventory Manager

A Rust workspace with inventory rules, SQLite persistence, shared settings, and a
Ratatui terminal UI.

| Crate | Responsibility | Public entry points |
| --- | --- | --- |
| `domain` | Values and inventory rules | `Id`, `Item`, `Sku`, `Stock`, `StockAmount`, commands and units |
| `settings` | Configuration and dictionaries | `Settings`, `SettingsModel`, `SettingsIO`, `LanguageSource`, `LanguagePack` |
| `persistance` | SQLite pools, migrations, transactions | `PineappleDatabase`, pagination and tracking types |
| `app` | Application operations and worker lifecycle | `AppState`, `InventoryHandle`, `SettingsHandle`, optional global access |
| `tui` | CLI arguments, input routing, and rendering | `run`, pages and display state |

Import supported types from each crate root. Existing grouped module paths remain
available. `app` also exposes the `domain`, `settings`, and `persistance` namespaces
for consumers that depend on the application facade. Private worker message types
do not need to be exported to perform application operations.

The startup flow is: parse CLI arguments, load settings and the selected dictionary,
initialize the database, start both workers, run the terminal UI, then drain both
workers. The TUI entry point handles shutdown after either a normal UI exit or a
returned UI error.

- [Application API and global lifecycle](crates/app/README.md)
- [Domain values, commands, and SKU generation](crates/domain/README.md)
- [Settings files, dictionaries, and language snapshots](crates/settings/README.md)

Language selection is saved as a source, separately from its display name. Share
`Arc<Settings>` for ongoing access and load one language snapshot per render.
Previously returned `Arc<LanguagePack>` snapshots keep their original pack.

The async page experiment remains separate from the running demo UI. Page labels
and worker results still need to be connected to that experiment's event loop.

Run `cargo test --workspace` to check the workspace and its public examples.
