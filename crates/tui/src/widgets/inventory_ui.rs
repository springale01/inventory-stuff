//! Async UI coordinator: page registry, navigation watch, requests, and suppliers.

use std::{any::Any, collections::HashMap, io, sync::Arc, time::Duration};

use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::{Stream, StreamExt};
use ratatui::{Frame, Terminal, backend::Backend};
use settings::Settings;
use tokio::sync::{mpsc, watch};

use super::{
    inventory_page::InventoryPage,
    page::{NavigationState, Page, PageId, UiAction},
    sku::SkuSupplierRegistry,
};

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error(transparent)]
    Terminal(#[from] io::Error),
    #[error(transparent)]
    Application(#[from] app::StaticsError),
    #[error("Page is not registered: {0:?}")]
    UnknownPage(PageId),
}

pub struct InventoryUi {
    navigation: watch::Sender<NavigationState>,
    pages: HashMap<PageId, Box<dyn Page>>,
    settings: Arc<Settings>,
    suppliers: SkuSupplierRegistry,
    requests_tx: mpsc::Sender<UiAction>,
    requests_rx: mpsc::Receiver<UiAction>,
    quit: bool,
}

impl InventoryUi {
    pub async fn new() -> Result<Self, UiError> {
        Ok(Self::with_settings(app::PineappleState::settings().await?))
    }

    /// Owns pages and suppliers without borrowing its own fields. The demo rows
    /// are display data; callers can replace the inventory page with real rows.
    pub fn with_settings(settings: Arc<Settings>) -> Self {
        let (navigation, _) = watch::channel(NavigationState::default());
        let (requests_tx, requests_rx) =
            mpsc::channel(settings.runtime_settings().mpsc_channel_capacity());
        let mut ui = Self {
            navigation,
            pages: HashMap::new(),
            settings,
            suppliers: SkuSupplierRegistry::default(),
            requests_tx,
            requests_rx,
            quit: false,
        };
        ui.register_page(PageId::InventoryPage, InventoryPage::demo());
        ui
    }

    pub fn register_page(&mut self, id: PageId, mut page: impl Page) {
        page.subscribe(self.navigation.subscribe());
        self.pages.insert(id, Box::new(page));
    }

    /// Exact concrete-type lookup, for shell requests needing page-specific methods.
    pub fn page<T: Page>(&self, id: PageId) -> Option<&T> {
        let page: &dyn Any = self.pages.get(&id)?.as_ref();
        page.downcast_ref()
    }

    pub fn page_mut<T: Page>(&mut self, id: PageId) -> Option<&mut T> {
        let page: &mut dyn Any = self.pages.get_mut(&id)?.as_mut();
        page.downcast_mut()
    }

    pub fn subscribe_navigation(&self) -> watch::Receiver<NavigationState> {
        self.navigation.subscribe()
    }

    pub fn navigation(&self) -> NavigationState {
        *self.navigation.borrow()
    }
    pub fn requests(&self) -> mpsc::Sender<UiAction> {
        self.requests_tx.clone()
    }
    pub fn settings(&self) -> Arc<Settings> {
        Arc::clone(&self.settings)
    }
    pub fn suppliers(&self) -> &SkuSupplierRegistry {
        &self.suppliers
    }
    pub fn suppliers_mut(&mut self) -> &mut SkuSupplierRegistry {
        &mut self.suppliers
    }

    pub fn navigate(&mut self, target: PageId) -> Result<(), UiError> {
        if !self.pages.contains_key(&target) {
            return Err(UiError::UnknownPage(target));
        }
        let state = self.navigation();
        if state.current != target {
            self.navigation.send_replace(NavigationState {
                current: target,
                previous: Some(state.current),
            });
        }
        Ok(())
    }

    fn handle_action(&mut self, action: UiAction) -> Result<(), UiError> {
        match action {
            UiAction::Quit => self.quit = true,
            UiAction::SwitchPage(target) => self.navigate(target)?,
            UiAction::Back => {
                if let Some(previous) = self.navigation().previous {
                    self.navigate(previous)?;
                }
            }
            UiAction::Redraw => {}
        }
        Ok(())
    }

    pub fn handle_event(&mut self, event: Event) -> Result<(), UiError> {
        if let Event::Key(key) = event {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.quit = true;
                return Ok(());
            }
            let id = self.navigation().current;
            let action = self
                .pages
                .get_mut(&id)
                .ok_or(UiError::UnknownPage(id))?
                .handle_key(key);
            if let Some(action) = action {
                self.handle_action(action)?;
            }
        }
        // Resize/focus events also cause a redraw on the next iteration.
        Ok(())
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let language = self.settings.lang();
        let id = self.navigation().current;
        let area = frame.area();
        self.pages
            .get_mut(&id)
            .expect("active page is registered")
            .draw(frame, area, &language);
    }

    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), UiError>
    where
        B::Error: Send + Sync + 'static,
    {
        self.run_with_events(terminal, EventStream::new()).await
    }

    /// Injecting a stream makes the actual async loop testable without a real TTY.
    pub async fn run_with_events<B, S>(
        &mut self,
        terminal: &mut Terminal<B>,
        mut events: S,
    ) -> Result<(), UiError>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
        S: Stream<Item = io::Result<Event>> + Unpin,
    {
        // Refresh language snapshots even while no keys or worker requests arrive.
        let mut refresh = tokio::time::interval(Duration::from_millis(100));
        refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while !self.quit {
            terminal
                .draw(|frame| self.draw(frame))
                .map_err(io::Error::other)?;
            tokio::select! {
                event = events.next() => match event {
                    Some(event) => self.handle_event(event?)?,
                    None => break,
                },
                Some(action) = self.requests_rx.recv() => self.handle_action(action)?,
                _ = refresh.tick() => {},
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::sku::SkuSupplierError;
    use super::*;
    use app::domain::{CounterSkuBuilder, SkuFamily};
    use futures_util::stream;
    use ratatui::{backend::TestBackend, layout::Rect};
    use settings::{HashMapDictionary, LanguagePack, TextKey};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ProbePage {
        keys: Arc<AtomicUsize>,
        draws: Arc<AtomicUsize>,
        navigation: Option<watch::Receiver<NavigationState>>,
    }

    impl Page for ProbePage {
        fn subscribe(&mut self, navigation: watch::Receiver<NavigationState>) {
            self.navigation = Some(navigation);
        }
        fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<UiAction> {
            self.keys.fetch_add(1, Ordering::SeqCst);
            (key.code == KeyCode::Char('s')).then_some(UiAction::SwitchPage(PageId::InventoryPage))
        }
        fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, language: &LanguagePack) {
            self.draws.fetch_add(1, Ordering::SeqCst);
            frame.render_widget(PageId::SettingsPage.label(language), area);
        }
    }

    fn ui() -> InventoryUi {
        InventoryUi::with_settings(Arc::new(Settings::default()))
    }
    fn terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(90, 25)).unwrap()
    }
    fn key(code: KeyCode) -> Event {
        Event::Key(crossterm::event::KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn screen(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn routing_downcasts_and_navigation_keep_one_consistent_state() {
        let mut ui = ui();
        let mut navigation = ui.subscribe_navigation();
        assert!(matches!(
            ui.navigate(PageId::SettingsPage),
            Err(UiError::UnknownPage(PageId::SettingsPage))
        ));
        assert_eq!(ui.navigation(), NavigationState::default());
        assert!(!navigation.has_changed().unwrap());
        let keys = Arc::new(AtomicUsize::new(0));
        let draws = Arc::new(AtomicUsize::new(0));
        ui.register_page(
            PageId::SettingsPage,
            ProbePage {
                keys: keys.clone(),
                draws: draws.clone(),
                navigation: None,
            },
        );
        let mut terminal = terminal();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        assert_eq!(draws.load(Ordering::SeqCst), 0);
        ui.navigate(PageId::SettingsPage).unwrap();
        let expected = NavigationState {
            current: PageId::SettingsPage,
            previous: Some(PageId::InventoryPage),
        };
        assert_eq!(*navigation.borrow_and_update(), expected);
        assert_eq!(
            *ui.page::<ProbePage>(PageId::SettingsPage)
                .unwrap()
                .navigation
                .as_ref()
                .unwrap()
                .borrow(),
            expected
        );
        assert_eq!(
            ui.page::<InventoryPage>(PageId::InventoryPage)
                .unwrap()
                .navigation(),
            Some(expected)
        );
        assert!(ui.page::<InventoryPage>(PageId::SettingsPage).is_none());
        ui.navigate(PageId::SettingsPage).unwrap();
        assert!(!navigation.has_changed().unwrap());
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        assert_eq!(draws.load(Ordering::SeqCst), 1);
        ui.handle_event(Event::Key(crossterm::event::KeyEvent::new_with_kind(
            KeyCode::Char('s'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )))
        .unwrap();
        assert_eq!(keys.load(Ordering::SeqCst), 0);
        ui.handle_event(key(KeyCode::Char('s'))).unwrap();
        assert_eq!(keys.load(Ordering::SeqCst), 1);
        assert_eq!(ui.navigation().current, PageId::InventoryPage);
        ui.handle_action(UiAction::Back).unwrap();
        assert_eq!(ui.navigation().current, PageId::SettingsPage);
    }

    #[tokio::test]
    async fn async_loop_handles_page_requests_without_keyboard_events() {
        let mut ui = ui();
        let draws = Arc::new(AtomicUsize::new(0));
        ui.register_page(
            PageId::SettingsPage,
            ProbePage {
                keys: Arc::new(AtomicUsize::new(0)),
                draws: draws.clone(),
                navigation: None,
            },
        );
        let tx = ui.requests();
        let sender = tokio::spawn(async move {
            tx.send(UiAction::SwitchPage(PageId::SettingsPage))
                .await
                .unwrap();
            tx.send(UiAction::Redraw).await.unwrap();
            tx.send(UiAction::Quit).await.unwrap();
        });
        let mut terminal = terminal();
        tokio::time::timeout(
            Duration::from_secs(5),
            ui.run_with_events(&mut terminal, stream::pending::<io::Result<Event>>()),
        )
        .await
        .unwrap()
        .unwrap();
        sender.await.unwrap();
        assert!(ui.quit);
        assert_eq!(ui.navigation().current, PageId::SettingsPage);
        assert!(draws.load(Ordering::SeqCst) > 0);
        assert!(screen(&terminal).contains("Settings"));
    }

    #[tokio::test]
    async fn async_loop_uses_fresh_language_snapshots_after_background_redraw() {
        let settings = Arc::new(Settings::default());
        let old = settings.language_snapshot();
        let mut ui = InventoryUi::with_settings(settings.clone());
        let tx = ui.requests();
        let sender = tokio::spawn(async move {
            settings.swap_lang(
                HashMapDictionary::new(
                    [(TextKey::InventoryPageTitle, "Entrepot".to_owned())],
                    "French",
                )
                .into_language_pack(),
            );
            tx.send(UiAction::Redraw).await.unwrap();
            tx.send(UiAction::Quit).await.unwrap();
        });
        let mut terminal = terminal();
        tokio::time::timeout(
            Duration::from_secs(5),
            ui.run_with_events(&mut terminal, stream::pending::<io::Result<Event>>()),
        )
        .await
        .unwrap()
        .unwrap();
        sender.await.unwrap();
        assert!(screen(&terminal).contains("Entrepot"));
        assert_eq!(PageId::InventoryPage.label(&old), "Inventory");
        assert_eq!(PageId::InventoryPage.as_str(), "inventory");
    }

    #[tokio::test]
    async fn async_input_closes_help_before_quitting_and_keeps_selection() {
        let mut ui = ui();
        let mut terminal = terminal();
        let events = stream::iter([
            Ok(key(KeyCode::Char('?'))),
            Ok(key(KeyCode::Down)),
            Ok(key(KeyCode::Char('q'))),
            Ok(key(KeyCode::Char('q'))),
        ]);
        ui.run_with_events(&mut terminal, events).await.unwrap();
        assert!(ui.quit);
        let inventory = ui.page::<InventoryPage>(PageId::InventoryPage).unwrap();
        assert!(!inventory.help_open());
        assert_eq!(inventory.selected_index(), Some(0));
    }

    #[tokio::test]
    async fn event_stream_errors_propagate_and_eof_exits_without_spinning() {
        let mut ui = ui();
        let mut terminal = terminal();
        let events = stream::iter([Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "event failure",
        ))]);
        assert!(matches!(ui.run_with_events(&mut terminal, events).await,
            Err(UiError::Terminal(error)) if error.kind() == io::ErrorKind::PermissionDenied));
        tokio::time::timeout(
            Duration::from_secs(5),
            ui.run_with_events(&mut terminal, stream::empty::<io::Result<Event>>()),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[test]
    fn shell_owns_typed_supplier_registry_and_weak_links_expire_on_drop() {
        struct Hardware;
        impl SkuFamily for Hardware {}
        let mut ui = ui();
        let id = ui.suppliers_mut().register(Arc::new(
            CounterSkuBuilder::default()
                .add("BOLT")
                .build::<Hardware>(None)
                .unwrap(),
        ));
        let mut sku = ui.suppliers().generate(id).unwrap();
        assert_eq!(sku.supplier_id(), Some(id));
        drop(ui);
        assert!(matches!(sku.regenerate(), Err(SkuSupplierError::Expired(source)) if source == id));
        assert_eq!(sku.sku().as_str(), "BOLT-0001");
    }
}
