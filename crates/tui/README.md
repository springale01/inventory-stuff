# Inventory TUI starter

Run from the workspace root:

```sh
cargo run -p tui
```

This is a display-only starter with sample count-based items. It does not query
the database or change stock. The summary labels its count as **item types**,
rather than adding together quantities of unrelated products.

```text
InventoryUi (owns terminal loop and MainPage)
  MainPage (owns item rows, TableState, help visibility)
    header
    summary
    body: item table + selected item details
    footer
    optional help overlay, drawn last
```

`main_page.rs` has the page state, layout, drawing helpers, and local key handling.
`inventory_ui.rs` reads terminal events and forwards key presses to the page.
`lib.rs` initializes/restores the terminal; `main.rs` makes the crate executable.
The earlier `Widgets` trait is still available but this screen doesn't need it.

## Add another panel

1. Add a field to `MainPage` if the panel needs to remember something.
2. In `draw`, split an existing rectangle with `Layout::horizontal` or `vertical`.
3. Write a `draw_your_panel(&self, frame: &mut Frame, area: Rect)` helper.
4. Call it with the new rectangle. Use `&mut self` if rendering needs mutable state.

For example, replace the body split with three sections for a table, details,
and activity log. Keep each helper concerned only with its own rectangle.

## Add a table column

Add the display field to `ItemRow`, then update the cells, width constraints,
and header in `draw_table`. For example, a location column needs all three.

## Add a key action

Add a match arm to `MainPage::handle_key`. Page state changes there; the next
draw reflects it. For a popup, store its open/closed state, draw it last after
`Clear`, and give it input before the underlying page (see the help popup).
When adding editable text, route input to the focused field before global keys
so typing `q` doesn't quit.

## Connect real data

Replace `MainPage::demo()` in `InventoryUi::new()` with
`MainPage::new(rows_from_your_app)`. These rows are display values; domain
validation and stock updates stay behind the app API. The header currently
says SAMPLE DATA; change that when the data source changes.

The event loop currently blocks on terminal input/resize. To receive background
worker updates, extend it to also wake for those messages, e.g. an async event
stream alongside the worker receiver. Don't call blocking `event::read()`
directly inside a Tokio async task. Don't query the database on every draw.

## Controls and sizing

- Up/Down or j/k: select; Home/End: first/last item.
- ?: open help; Esc, q, or ?: close help. Other keys stay within the modal.
- q outside help: quit.
- At 90 columns or wider, details sit beside the table; otherwise below it.
- Below 40 columns or 18 rows, an enlarge-terminal message replaces the page.
