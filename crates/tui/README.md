# Inventory TUI

The terminal UI uses one async coordinator and one inventory page. Run from the
workspace root with `cargo run -p tui`, optionally passing
`--settings-location config/settings.toml`.

The executable parses arguments, awaits application initialization, runs the UI,
restores the terminal, then awaits application shutdown. Rendering is synchronous;
waiting for input and shell requests is asynchronous.

| Module | Responsibility |
| --- | --- |
| `widgets/inventory_ui.rs` | Async event loop, page registry, requests, navigation watch, supplier ownership |
| `widgets/page.rs` | `Page`, `PageId`, `NavigationState`, and `UiAction` |
| `widgets/inventory_page.rs` | Rows, table selection, layout, details, help, and key bindings |
| `widgets/sku.rs` | Domain-typed supplier IDs, strong registry references, and weak row origins |
| `tools/key.rs` | Maps key chords to page actions |

Public UI types are also exported from the crate root. The older `MainPage`, V2,
and nested experimental UI implementations have been removed.

## Ownership and navigation

`InventoryUi` owns its pages, `Arc<Settings>`, and `SkuSupplierRegistry`. Pages do
not borrow the shell's fields. Each registered page receives a navigation watch
subscription. Only the current page gets key events and rendering calls.

`NavigationState` publishes current and previous page together. Same-page
navigation leaves history unchanged; a missing destination returns `UiError`
without changing the state. `Back` visits the single previous page, not a full
history stack. Popups belong to the inventory page and do not change navigation.

The page map uses stable `PageId` keys. `PageId::as_str()` is independent of the
selected language; `label(pack)` looks up a translated heading. The shell loads
one language snapshot per render, and refreshes periodically so language changes
appear while input is idle.

```rust,no_run
use std::sync::Arc;
use settings::Settings;
use tui::{InventoryPage, InventoryUi, PageId, UiAction};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let mut ui = InventoryUi::with_settings(Arc::new(Settings::default()));
// Replace the sample page with your own display rows.
ui.register_page(PageId::InventoryPage, InventoryPage::new(Vec::new()));
let navigation = ui.subscribe_navigation();
let requests = ui.requests();
let page = ui.page_mut::<InventoryPage>(PageId::InventoryPage).unwrap();
assert!(page.items().is_empty());
// From a task running alongside ui.run(...):
requests.send(UiAction::Redraw).await?;
# Ok(())
# }
```

Only the inventory page is registered initially. Register other pages before
requesting navigation to them. `page::<T>` and `page_mut::<T>` perform checked
downcasts for shell operations needing a page's concrete methods; a mismatched
type returns `None`.

## Events and rendering

The event loop selects between Crossterm's `EventStream`, a bounded Tokio request
channel, and a refresh timer. Channel actions can wake it without keyboard input.
There are no blocking `event::read()` calls or nested `block_on` calls in the UI.
`run_with_events` accepts an injected event stream and terminal backend for tests.
End of input exits normally; event and drawing errors propagate through `UiError`.

Pages return shell actions from `handle_key`; their synchronous `draw` receives a
frame, rectangle, and borrowed language pack. Keep filesystem/database operations
out of drawing. Extend `UiAction` and the shell dispatcher when adding typed
background operation results; the existing inbox carries navigation/redraw/quit
actions rather than inventory data.

The default screen still shows labelled sample count-based rows. It does not
query the database or change stock. Replace it with `InventoryPage::new(rows)`
when connecting real application results. SKU values and optional weak supplier
origins are part of each `ItemRow`; the shell owns the strong supplier registry.
See the `widgets::sku` documentation for a complete generation example.

## Controls and layout

- Up/Down or j/k: select; Home/End: first/last item.
- ?: open help; Esc, q, or ?: close help. Other keys stay within the modal.
- q outside help: quit. Ctrl+C exits from any page or popup.
- At 90 columns or wider, details sit beside the table; otherwise below it.
- Below 40 columns or 18 rows, an enlarge-terminal message replaces the page.

Add fields to `ItemRow` and update table cells, widths, and headers together when
adding columns. Page state persists between draws; widgets are rebuilt each frame.
`bind_key` customizes the page's action map without boxed callbacks.
