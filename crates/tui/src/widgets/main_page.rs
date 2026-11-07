//! Inventory screen. State lives in MainPage; widgets are rebuilt each draw.
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

use super::inventory_ui::expirmental_inv_ui::{Page, UiAction};
use crate::tools::key::KeyMap;

/// Display data for this count-based demo, not a replacement domain item model.
/// Later, map the result of an app operation into these rows.
pub struct ItemRow {
    pub sku: String,
    pub name: String,
    pub quantity: u64,
    pub low_stock_at: u64,
}

impl ItemRow {
    fn is_low(&self) -> bool {
        self.quantity <= self.low_stock_at
    }
}

/// Owns everything the page remembers between frames.
pub struct MainPage {
    items: Vec<ItemRow>,
    // Stores both the selected row and the scrolling offset.
    table: TableState,
    help_open: bool,
}

impl MainPage {
    pub fn new(items: Vec<ItemRow>) -> Self {
        let mut table = TableState::default();
        table.select((!items.is_empty()).then_some(0));
        Self {
            items,
            table,
            help_open: false,
        }
    }

    /// Replace this at startup with MainPage::new(your_display_rows).
    pub fn demo() -> Self {
        Self::new(vec![
            ItemRow {
                sku: "GPU-0001".into(),
                name: "Graphics card".into(),
                quantity: 12,
                low_stock_at: 3,
            },
            ItemRow {
                sku: "CBL-0001".into(),
                name: "USB-C cable".into(),
                quantity: 2,
                low_stock_at: 5,
            },
            ItemRow {
                sku: "KEY-0001".into(),
                name: "Keyboard".into(),
                quantity: 8,
                low_stock_at: 2,
            },
            ItemRow {
                sku: "MON-0001".into(),
                name: "Monitor".into(),
                quantity: 0,
                low_stock_at: 2,
            },
        ])
    }

    pub fn help_open(&self) -> bool {
        self.help_open
    }

    /// The shell forwards key presses here. Add page-local actions in this match.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Modal input goes here first; arrows must not move the table behind help.
        if self.help_open {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.help_open = false;
            }
            return;
        }
        match key.code {
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(true),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(false),
            KeyCode::Home if !self.items.is_empty() => self.table.select(Some(0)),
            KeyCode::End if !self.items.is_empty() => self.table.select(Some(self.items.len() - 1)),
            _ => {}
        }
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
    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
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
            Paragraph::new(" INVENTORY / Overview / SAMPLE DATA")
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
                Cell::from(item.sku.as_str()),
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
                item.name, item.sku, item.quantity, item.low_stock_at
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
            Paragraph::new("Up/Down or j/k: select an item\nHome/End: first/last item\n?: open help\nEsc, q or ?: close help\nq outside help: quit\n\nSample data only; no stock is changed.")
                .wrap(Wrap { trim: true })
                .block(Block::bordered().title("Help").border_style(Style::default().fg(Color::Cyan))), popup,
        );
    }
}
/// Key bindings produce actions; the page applies them to its persistent state.
/// Reuses the original page's rendering and table state during the experiment.
pub struct MainPageV2 {
    page: MainPage,
    keys: KeyMap<MainAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainAction {
    NextItem,
    PreviousItem,
    FirstItem,
    LastItem,
    OpenHelp,
    Quit,
}

impl MainPageV2 {
    pub fn new(items: Vec<ItemRow>) -> Self {
        Self::with_page(MainPage::new(items))
    }

    pub fn demo() -> Self {
        Self::with_page(MainPage::demo())
    }

    fn with_page(page: MainPage) -> Self {
        let mut result = Self {
            page,
            keys: KeyMap::default(),
        };
        for (code, action) in [
            (KeyCode::Down, MainAction::NextItem),
            (KeyCode::Char('j'), MainAction::NextItem),
            (KeyCode::Up, MainAction::PreviousItem),
            (KeyCode::Char('k'), MainAction::PreviousItem),
            (KeyCode::Home, MainAction::FirstItem),
            (KeyCode::End, MainAction::LastItem),
            (KeyCode::Char('?'), MainAction::OpenHelp),
            (KeyCode::Char('q'), MainAction::Quit),
        ] {
            result.bind_key(code, KeyModifiers::NONE, action);
        }
        // Terminals can report '?' with or without the Shift modifier.
        result.bind_key(
            KeyCode::Char('?'),
            KeyModifiers::SHIFT,
            MainAction::OpenHelp,
        );
        result
    }

    pub fn bind_key(&mut self, code: KeyCode, modifiers: KeyModifiers, action: MainAction) {
        self.keys.bind(code, modifiers, action);
    }
}

impl Page for MainPageV2 {
    fn handle_key(&mut self, key: KeyEvent) -> Option<UiAction> {
        // The page owns its help overlay. It consumes all input while open,
        // preventing navigation or quitting through the modal.
        if self.page.help_open {
            self.page.handle_key(key);
            return None;
        }
        match self.keys.action(key).copied()? {
            MainAction::NextItem => self.page.move_selection(true),
            MainAction::PreviousItem => self.page.move_selection(false),
            MainAction::FirstItem if !self.page.items.is_empty() => self.page.table.select(Some(0)),
            MainAction::LastItem if !self.page.items.is_empty() => {
                self.page.table.select(Some(self.page.items.len() - 1))
            }
            MainAction::OpenHelp => self.page.help_open = true,
            MainAction::Quit => return Some(UiAction::Quit),
            _ => {}
        }
        None
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.page.draw(frame, area);
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
    fn v2_navigation_modals_and_custom_bindings_update_the_page() {
        let mut page = MainPageV2::demo();
        assert_eq!(page.page.table.selected(), Some(0));
        page.handle_key(key(KeyCode::Down));
        assert_eq!(page.page.table.selected(), Some(1));
        page.handle_key(key(KeyCode::Char('?')));
        assert!(page.page.help_open);
        page.handle_key(key(KeyCode::Down));
        assert_eq!(page.page.table.selected(), Some(1));
        assert!(page.handle_key(key(KeyCode::Char('q'))).is_none());
        assert!(!page.page.help_open);
        page.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
        assert_eq!(page.page.table.selected(), Some(1));
        page.bind_key(
            KeyCode::Char('n'),
            KeyModifiers::CONTROL,
            MainAction::NextItem,
        );
        page.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(page.page.table.selected(), Some(2));
        page.handle_key(key(KeyCode::End));
        assert_eq!(page.page.table.selected(), Some(3));
        page.handle_key(key(KeyCode::Down));
        assert_eq!(page.page.table.selected(), Some(3));
        page.handle_key(key(KeyCode::Home));
        page.handle_key(key(KeyCode::Up));
        assert_eq!(page.page.table.selected(), Some(0));
        assert!(matches!(
            page.handle_key(key(KeyCode::Char('q'))),
            Some(UiAction::Quit)
        ));
    }

    #[test]
    fn v2_renders_without_input_and_empty_tables_stay_unselected() {
        let mut page = MainPageV2::new(Vec::new());
        for code in [KeyCode::Home, KeyCode::End, KeyCode::Down, KeyCode::Up] {
            page.handle_key(key(code));
        }
        assert_eq!(page.page.table.selected(), None);
        let mut terminal = Terminal::new(TestBackend::new(90, 25)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                page.draw(frame, area);
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
    }
}
