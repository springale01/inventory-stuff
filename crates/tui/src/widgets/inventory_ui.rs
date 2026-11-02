//! Shell: owns the page, terminal loop, and application-wide actions.
use super::main_page::MainPage;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::DefaultTerminal;
use std::io;

pub(crate) struct InventoryUi {
    main_page: MainPage,
}

impl InventoryUi {
    pub(crate) fn new() -> Self {
        Self {
            main_page: MainPage::demo(),
        }
    }

    pub(crate) fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        loop {
            terminal.draw(|frame| {
                let area = frame.area();
                self.main_page.draw(frame, area);
            })?;

            // Blocks until input or resize. When adding worker updates, wake this
            // loop for those messages too. Don't block a Tokio task with read().
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                // Modal input first: q closes help before it quits the UI.
                if key.code == KeyCode::Char('q') && !self.main_page.help_open() {
                    return Ok(());
                }
                self.main_page.handle_key(key);
            }
        }
    }
}
