//! Inventory screen. State lives in InventoryPage; widgets are rebuilt each draw.
//!
//! Extend it by adding fields for inputs/filters, updating them in handle_key,
//! and splitting another Rect in draw. Keep database calls out of drawing.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};

use super::{
    page::{NavigationState, Page, PageId, UiAction},
    sku::SkuClonglomerate,
};
use crate::tools::key::KeyMap;
use app::domain::Sku;
use settings::LanguagePack;
use tokio::sync::watch;

/// Display data for this count-based demo, not a replacement domain item model.
/// Later, map the result of an app operation into these rows.
pub struct ItemRow {
    pub id: Box<str>,
    pub sku: SkuClonglomerate,
    pub description: Option<String>,
    pub name: String,
    pub quantity: u64,
    pub low_stock_at: u64,
}

impl ItemRow {
    pub fn as_parts(&self) -> impl ExactSizeIterator<Item = &str> {
        [
            self.id.as_ref(),
            self.sku.sku().as_str(),
            self.name.as_str(),
            self.description.as_deref().unwrap_or(""),
        ]
        .into_iter()
    }

    fn is_low(&self) -> bool {
        self.quantity <= self.low_stock_at
    }
}

/// Owns everything the page remembers between frames.
pub struct InventoryPage {
    items: Vec<ItemRow>,
    // Stores both the selected row and the scrolling offset.
    table: TableState,
    help_open: bool,
    navigation: Option<watch::Receiver<NavigationState>>,
    keys: KeyMap<InventoryAction>,
    demo: bool,
    editor: Option<ItemEditor>,
}

impl InventoryPage {
    pub fn new(items: Vec<ItemRow>) -> Self {
        let mut table = TableState::default();
        table.select((!items.is_empty()).then_some(0));
        Self {
            items,
            table,
            help_open: false,
            navigation: None,
            keys: default_keys(),
            demo: false,
            editor: None,
        }
    }

    /// Sample display data, explicitly labelled in the heading.
    pub fn demo() -> Self {
        let items = [
            ("GPU-0001", "Graphics card", 12, 3),
            ("CBL-0001", "USB-C cable", 2, 5),
            ("KEY-0001", "Keyboard", 8, 2),
            ("MON-0001", "Monitor", 0, 2),
        ]
        .into_iter()
        .map(|(sku, name, quantity, low_stock_at)| ItemRow {
            id: sku.into(),
            sku: SkuClonglomerate::manual(Sku::new(sku).expect("demo SKU is valid")),
            description: None,
            name: name.into(),
            quantity,
            low_stock_at,
        })
        .collect();
        let mut page = Self::new(items);
        page.demo = true;
        page
    }

    pub fn items(&self) -> &[ItemRow] {
        &self.items
    }
    pub fn selected_index(&self) -> Option<usize> {
        self.table.selected()
    }
    pub fn navigation(&self) -> Option<NavigationState> {
        self.navigation.as_ref().map(|rx| *rx.borrow())
    }
    pub fn bind_key(&mut self, code: KeyCode, modifiers: KeyModifiers, action: InventoryAction) {
        self.keys.bind(code, modifiers, action);
    }

    pub fn help_open(&self) -> bool {
        self.help_open
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<UiAction> {
        if self.help_open {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.help_open = false;
            }
            return None;
        }
        match self.keys.action(key).copied()? {
            InventoryAction::NextItem => self.move_selection(true),
            InventoryAction::PreviousItem => self.move_selection(false),
            InventoryAction::FirstItem if !self.items.is_empty() => self.table.select(Some(0)),
            InventoryAction::LastItem if !self.items.is_empty() => {
                self.table.select(Some(self.items.len() - 1))
            }
            InventoryAction::OpenHelp => self.help_open = true,
            InventoryAction::Quit => return Some(UiAction::Quit),
            InventoryAction::Edit => {
                if let Some(item) = self
                    .table
                    .selected()
                    .and_then(|index| self.items.get(index))
                {
                    ()
                }
            }
            _ => {}
        }
        None
    }

    fn move_selection(&mut self, down: bool) {
        if self.items.is_empty() {
            self.table.select(None);
            return;
        }
        let current = self.table.selected().unwrap_or(0);
        let next = if down {
            current.saturating_add(1).min(self.items.len() - 1)
        } else {
            current.saturating_sub(1)
        };
        self.table.select(Some(next));
    }

    /// Layout is a tree of rectangles. Each helper draws into its assigned area.
    /// This renders display state; it never mutates inventory or queries SQL.
    pub fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, language: &LanguagePack) {
        if area.width < 40 || area.height < 18 {
            frame.render_widget(
                Paragraph::new("Enlarge terminal to at least 40 x 18.\nq: quit | ?: help")
                    .wrap(Wrap { trim: true }),
                area,
            );
            if self.help_open {
                self.draw_help(frame, area);
            }
            return;
        }

        // Vertical shell: header, summary, flexible content, controls.
        let [header, summary, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(
            Paragraph::new(format!(
                " {} / Overview{}",
                PageId::InventoryPage.label(language),
                if self.demo { " / SAMPLE DATA" } else { "" }
            ))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::bordered()),
            header,
        );
        self.draw_summary(frame, summary);

        // Wide terminals use side-by-side panels; narrow ones stack them.
        let [table, details] = if body.width >= 90 {
            Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)]).areas(body)
        } else {
            Layout::vertical([Constraint::Fill(1), Constraint::Length(5)]).areas(body)
        };
        self.draw_table(frame, table);
        self.draw_details(frame, details);
        frame.render_widget(
            Paragraph::new(" Up/Down: select | ?: help | q: quit")
                .style(Style::default().fg(Color::DarkGray)),
            footer,
        );

        // Overlays go LAST because later widgets paint over earlier ones.
        if self.help_open {
            self.draw_help(frame, area);
        }
    }

    fn draw_summary(&self, frame: &mut Frame, area: Rect) {
        let low = self.items.iter().filter(|item| item.is_low()).count();
        frame.render_widget(
            Paragraph::new(format!(
                "{} item types | {} low stock",
                self.items.len(),
                low
            ))
            .block(Block::bordered().title("Summary")),
            area,
        );
    }

    fn draw_table(&mut self, frame: &mut Frame, area: Rect) {
        if self.items.is_empty() {
            frame.render_widget(
                Paragraph::new("No items yet. Load items through your app layer.")
                    .wrap(Wrap { trim: true })
                    .block(Block::bordered().title("Items")),
                area,
            );
            return;
        }
        // To add a column, update the cells, widths, AND header below.
        let rows = self.items.iter().map(|item| {
            let color = if item.is_low() {
                Color::Yellow
            } else {
                Color::Green
            };
            Row::new(vec![
                Cell::from(item.sku.sku().as_str()),
                Cell::from(item.name.as_str()),
                Cell::from(item.quantity.to_string()),
                Cell::from(if item.is_low() { "LOW" } else { "OK" })
                    .style(Style::default().fg(color)),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Fill(1),
                Constraint::Length(3),
                Constraint::Length(5),
            ],
        )
        .header(
            Row::new(["SKU", "Name", "Qty", "Stock"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(Block::bordered().title("Items"))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
        // Widget borrows item text; persistent scrolling lives in self.table.
        frame.render_stateful_widget(table, area, &mut self.table);
    }

    fn draw_details(&self, frame: &mut Frame, area: Rect) {
        let text = match self
            .table
            .selected()
            .and_then(|index| self.items.get(index))
        {
            Some(item) => format!(
                "{}\nSKU: {}\nQuantity: {} | Low at: {}",
                item.name,
                item.sku.sku(),
                item.quantity,
                item.low_stock_at
            ),
            None => "Select an item to inspect it.".into(),
        };
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: true })
                .block(Block::bordered().title("Selected item")),
            area,
        );
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(9),
            Constraint::Fill(1),
        ])
        .areas(area);
        let [_, popup, _] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(56),
            Constraint::Fill(1),
        ])
        .areas(middle);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new("Up/Down or j/k: select an item\nHome/End: first/last item\n?: open help\nEsc, q or ?: close help\nq outside help: quit")
                .wrap(Wrap { trim: true })
                .block(Block::bordered().title("Help").border_style(Style::default().fg(Color::Cyan))), popup,
        );
    }

    fn draw_edit(&self, fram: &mut Frame, area: Rect) {
        ()
    }
}

struct ItemEditor {
    item_id: Box<str>,
    name: String,
    error: Option<String>,
}

impl Default for InventoryPage {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryAction {
    NextItem,
    PreviousItem,
    FirstItem,
    LastItem,
    OpenHelp,
    Quit,
    Edit,
}

fn default_keys() -> KeyMap<InventoryAction> {
    let mut keys = KeyMap::default();
    for (code, action) in [
        (KeyCode::Down, InventoryAction::NextItem),
        (KeyCode::Char('j'), InventoryAction::NextItem),
        (KeyCode::Up, InventoryAction::PreviousItem),
        (KeyCode::Char('k'), InventoryAction::PreviousItem),
        (KeyCode::Home, InventoryAction::FirstItem),
        (KeyCode::End, InventoryAction::LastItem),
        (KeyCode::Char('?'), InventoryAction::OpenHelp),
        (KeyCode::Char('q'), InventoryAction::Quit),
        (KeyCode::Char('e'), InventoryAction::Edit),
    ] {
        keys.bind(code, KeyModifiers::NONE, action);
    }
    keys.bind(
        KeyCode::Char('?'),
        KeyModifiers::SHIFT,
        InventoryAction::OpenHelp,
    );
    keys
}

impl Page for InventoryPage {
    fn subscribe(&mut self, navigation: watch::Receiver<NavigationState>) {
        self.navigation = Some(navigation);
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<UiAction> {
        InventoryPage::handle_key(self, key)
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, language: &LanguagePack) {
        InventoryPage::draw(self, frame, area, language);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn navigation_help_and_custom_bindings_preserve_their_behavior() {
        let mut page = InventoryPage::demo();
        page.handle_key(key(KeyCode::Down));
        assert_eq!(page.selected_index(), Some(1));
        page.handle_key(key(KeyCode::Char('?')));
        assert!(page.help_open());
        page.handle_key(key(KeyCode::Down));
        assert_eq!(page.selected_index(), Some(1));
        assert!(page.handle_key(key(KeyCode::Char('q'))).is_none());
        assert!(!page.help_open());
        page.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        assert_eq!(page.selected_index(), Some(1));
        page.bind_key(
            KeyCode::Char('n'),
            KeyModifiers::CONTROL,
            InventoryAction::NextItem,
        );
        page.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(page.selected_index(), Some(2));
        page.handle_key(key(KeyCode::End));
        page.handle_key(key(KeyCode::Down));
        assert_eq!(page.selected_index(), Some(3));
        page.handle_key(key(KeyCode::Home));
        page.handle_key(key(KeyCode::Up));
        assert_eq!(page.selected_index(), Some(0));
        assert_eq!(
            page.handle_key(key(KeyCode::Char('q'))),
            Some(UiAction::Quit)
        );
    }

    #[test]
    fn empty_tables_render_without_input_and_small_terminals_do_not_panic() {
        let mut page = InventoryPage::default();
        for code in [KeyCode::Home, KeyCode::End, KeyCode::Down, KeyCode::Up] {
            page.handle_key(key(code));
        }
        assert_eq!(page.selected_index(), None);
        let mut terminal = Terminal::new(TestBackend::new(90, 25)).unwrap();
        let pack = LanguagePack::default();
        terminal
            .draw(|frame| {
                let area = frame.area();
                page.draw(frame, area, &pack);
            })
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("No items yet."));
        terminal.resize(Rect::new(0, 0, 10, 3)).unwrap();
        page.handle_key(key(KeyCode::Char('?')));
        terminal
            .draw(|frame| {
                let area = frame.area();
                page.draw(frame, area, &pack);
            })
            .unwrap();
    }

    #[test]
    fn rows_keep_owned_skus_and_display_parts_after_their_supplier_expires() {
        use super::super::sku::SkuSupplierRegistry;
        use app::domain::{CounterSkuBuilder, SkuFamily};
        struct Hardware;
        impl SkuFamily for Hardware {}
        let mut registry = SkuSupplierRegistry::default();
        let id = registry.register(std::sync::Arc::new(
            CounterSkuBuilder::default()
                .add("BOLT")
                .build::<Hardware>(None)
                .unwrap(),
        ));
        let row = ItemRow {
            id: "item".into(),
            sku: registry.generate(id).unwrap(),
            name: "Bolts".into(),
            description: Some("M8".into()),
            quantity: 3,
            low_stock_at: 1,
        };
        registry.remove(id);
        assert!(row.sku.supplier().is_none());
        assert_eq!(
            row.as_parts().collect::<Vec<_>>(),
            ["item", "BOLT-0001", "Bolts", "M8"]
        );
    }
}
