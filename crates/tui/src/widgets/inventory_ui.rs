//! Shell: owns pages, active-page routing, and the terminal loop.

pub(crate) use expirmental_inv_ui::InventoryUiExp as InventoryUi;

// Kept in the experimental module so callers can continue iterating on V2.
pub(crate) mod expirmental_inv_ui {
    use std::{collections::HashMap, io};

    use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
    use ratatui::{DefaultTerminal, Frame, layout::Rect};

    use crate::widgets::main_page::MainPageV2;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PageName(pub &'static str);

    impl AsRef<str> for PageName {
        fn as_ref(&self) -> &str {
            self.0
        }
    }

    pub enum UiAction {
        Quit,
        SwitchPage(PageName),
    }

    /// A page updates its own state synchronously and asks the shell to perform
    /// navigation/quit actions. Async work belongs outside rendering.
    pub trait Page {
        fn handle_key(&mut self, key: KeyEvent) -> Option<UiAction>;
        fn draw(&mut self, frame: &mut Frame<'_>, area: Rect);
    }

    pub(crate) struct InventoryUiExp {
        pages: HashMap<PageName, Box<dyn Page>>,
        active_page: PageName,
        quit: bool,
    }

    impl InventoryUiExp {
        pub(crate) fn new() -> Self {
            let main = PageName("inventory");
            let mut ui = Self {
                pages: HashMap::new(),
                active_page: main,
                quit: false,
            };
            ui.register_page(main, MainPageV2::demo());
            ui
        }

        /// Registration does not change focus; a page requests focus with SwitchPage.
        pub(crate) fn register_page(&mut self, name: PageName, page: impl Page + 'static) {
            self.pages.insert(name, Box::new(page));
        }

        fn handle_key(&mut self, key: KeyEvent) {
            if key.kind != KeyEventKind::Press {
                return;
            }
            let action = self
                .pages
                .get_mut(&self.active_page)
                .expect("active page is registered")
                .handle_key(key);
            match action {
                Some(UiAction::Quit) => self.quit = true,
                Some(UiAction::SwitchPage(name)) if self.pages.contains_key(&name) => {
                    self.active_page = name;
                }
                // An unknown destination leaves the active page unchanged.
                _ => {}
            }
        }

        fn draw(&mut self, frame: &mut Frame<'_>) {
            let area = frame.area();
            self.pages
                .get_mut(&self.active_page)
                .expect("active page is registered")
                .draw(frame, area);
        }

        pub(crate) fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
            while !self.quit {
                terminal.draw(|frame| self.draw(frame))?;
                // This loop is synchronous. Worker results will need to wake it
                // alongside input when backend operations are connected.
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crossterm::event::{KeyCode, KeyModifiers};
        use ratatui::{Terminal, backend::TestBackend};
        use std::{cell::Cell, rc::Rc};

        struct TestPage {
            keys: Rc<Cell<usize>>,
            draws: Rc<Cell<usize>>,
            destination: PageName,
        }

        impl Page for TestPage {
            fn handle_key(&mut self, key: KeyEvent) -> Option<UiAction> {
                self.keys.set(self.keys.get() + 1);
                match key.code {
                    KeyCode::Char('s') => Some(UiAction::SwitchPage(self.destination)),
                    KeyCode::Char('q') => Some(UiAction::Quit),
                    _ => None,
                }
            }

            fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) {
                self.draws.set(self.draws.get() + 1);
                frame.render_widget("Active page", area);
            }
        }

        #[test]
        fn only_active_page_receives_input_and_draws_and_missing_destinations_preserve_focus() {
            let mut ui = InventoryUiExp::new();
            let first = PageName("inventory");
            let second = PageName("settings");
            let first_keys = Rc::new(Cell::new(0));
            let first_draws = Rc::new(Cell::new(0));
            let second_keys = Rc::new(Cell::new(0));
            let second_draws = Rc::new(Cell::new(0));
            ui.register_page(
                first,
                TestPage {
                    keys: first_keys.clone(),
                    draws: first_draws.clone(),
                    destination: second,
                },
            );
            ui.register_page(
                second,
                TestPage {
                    keys: second_keys.clone(),
                    draws: second_draws.clone(),
                    destination: first,
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
            terminal.draw(|frame| ui.draw(frame)).unwrap();
            assert_eq!((first_draws.get(), second_draws.get()), (1, 0));
            let switch = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
            ui.handle_key(KeyEvent::new_with_kind(
                KeyCode::Char('s'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ));
            assert_eq!(first_keys.get(), 0);
            ui.handle_key(switch);
            assert_eq!(ui.active_page, second);
            assert_eq!((first_keys.get(), second_keys.get()), (1, 0));
            terminal.draw(|frame| ui.draw(frame)).unwrap();
            assert_eq!((first_draws.get(), second_draws.get()), (1, 1));
            ui.handle_key(switch);
            assert_eq!(ui.active_page, first);
            ui.register_page(
                first,
                TestPage {
                    keys: first_keys,
                    draws: first_draws,
                    destination: PageName("missing"),
                },
            );
            ui.handle_key(switch);
            assert_eq!(ui.active_page, first);
            ui.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
            assert!(ui.quit);
        }

        #[test]
        fn live_shell_closes_help_before_quitting() {
            let mut ui = InventoryUiExp::new();
            ui.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT));
            ui.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
            assert!(!ui.quit);
            ui.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
            assert!(ui.quit);
        }
    }
}

pub(crate) mod async_inventory_ui {
    use crate::widgets::inventory_ui::async_inventory_ui::page_stuff::PageId;

    pub struct NavigationState {
        current: PageId,
        previous: Option<PageId>,
    }

    pub(crate) mod inventory {
        use settings::Settings;
        use std::{collections::HashMap, sync::Arc};

        use crate::widgets::inventory_ui::async_inventory_ui::{
            NavigationState,
            page_stuff::{Page, PageId, PageName},
        };
        pub struct AsyncInventoryUi {
            nav_state: tokio::sync::watch::Sender<NavigationState>,
            pages: HashMap<PageName, Box<dyn Page + Send>>,
            pub(crate) settings: Arc<Settings>,
        }
        #[derive(Debug, thiserror::Error)]
        pub enum AsyncInventoryError {
            #[error("Tui Exiting process started, no state avalible")]
            NoAppStateAvalible,
        }

        impl AsyncInventoryUi {
            pub fn new(&self) -> Result<Self, AsyncInventoryError> {
                let (tx, rx) = tokio::sync::watch::channel(NavigationState {
                    current: PageId::InventoryPage,
                    previous: None,
                });

                let settings = app::statics::PineappleState::runtime()
                    .block_on(app::statics::PineappleState::state())
                    .as_ref()
                    .ok_or(AsyncInventoryError::NoAppStateAvalible)?
                    .settings();
                // inventory/main page and others next
                todo!()
            }
        }
    }
    // --------------------------- Page Stuff ---------------------------
    pub(crate) mod page_stuff {
        use std::{
            any::{Any, TypeId},
            ops::Deref,
        };

        use settings::{
            Settings,
            langs::{LanguagePack, TextKey},
        };

        use crate::widgets::inventory_ui::async_inventory_ui::NavigationState;
        // maybe we can do a [From<PageId> for TextKey] ?
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum PageId {
            InventoryPage,
        }
        impl PageId {
            pub fn label<'a>(&self, pack: &'a LanguagePack) -> &'a str {
                let key = TextKey::TuiName; // replace later on with a actual key
                pack.get(key).unwrap_or("Key Pair not found in dictionary")
            }

            pub fn with_str<R>(&self, settings: &Settings, f: impl FnOnce(&str) -> R) -> R {
                let p_textkey = TextKey::TuiName; // placeholder, change to smt later on

                let lang = settings.lang();
                let text = lang.get(p_textkey).unwrap_or("Key Pair not Found");

                f(text)
            }
        }
        #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct PageName {
            name: PageId,
            id: TypeId,
        }
        impl PageName {
            pub fn with_str<R>(&self, settings: &Settings, f: impl FnOnce(&str) -> R) -> R {
                self.name.with_str(settings, f)
            }

            pub fn label<'a>(&self, pack: &'a LanguagePack) -> &'a str {
                self.name.label(pack)
            }
        }
        pub trait Page: Any {
            /// store it somewhere I guess, preferably somewhere accessable
            fn subscribe(&mut self, rx: tokio::sync::watch::Receiver<NavigationState>) -> ();
        }
    }

    pub(crate) mod async_inventory_page {
        use ratatui::widgets::TableState;

        use crate::widgets::inventory_ui::async_inventory_ui::{NavigationState, page_stuff::Page};

        pub struct InventoryPage {
            subscriptions: Option<tokio::sync::watch::Receiver<NavigationState>>,
            table: TableState,
        }
        /*
            pub struct Item {
            pub id: Id<Item>,
            pub sku: Sku,
            pub name: String,
            pub description: Option<String>,

        } */
        struct ItemsRows {}

        impl Page for InventoryPage {
            /// Loads the subscribtions, overwritting if it has already been written before
            fn subscribe(&mut self, rx: tokio::sync::watch::Receiver<NavigationState>) -> () {
                self.subscriptions = Some(rx);
            }
        }
    }
}
