pub(crate) trait KeyHandler {
    fn handle_key(&self, key: crossterm::event::KeyEvent) -> () {}
}
