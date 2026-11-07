//! Page identity, shared navigation state, and the interface used by the shell.

use std::any::Any;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};
use settings::{LanguagePack, TextKey};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageId {
    InventoryPage,
    SettingsPage,
}

impl PageId {
    /// Stable identifiers do not change when the selected language changes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InventoryPage => "inventory",
            Self::SettingsPage => "settings",
        }
    }

    pub fn label(self, pack: &LanguagePack) -> &str {
        let key = match self {
            Self::InventoryPage => TextKey::InventoryPageTitle,
            Self::SettingsPage => TextKey::SettingsPageTitle,
        };
        pack.get(key).unwrap_or(self.as_str())
    }
}

impl AsRef<str> for PageId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationState {
    pub current: PageId,
    /// One previous page, rather than a complete back stack.
    pub previous: Option<PageId>,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            current: PageId::InventoryPage,
            previous: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    Quit,
    SwitchPage(PageId),
    Back,
    /// Requests a fresh render, including when a background task finishes.
    Redraw,
}

/// Pages retain display state; the shell owns navigation and async event waiting.
/// Rendering stays synchronous and never keeps a frame across an await.
pub trait Page: Any + Send {
    fn subscribe(&mut self, navigation: watch::Receiver<NavigationState>);
    fn handle_key(&mut self, key: KeyEvent) -> Option<UiAction>;
    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, language: &LanguagePack);
}
