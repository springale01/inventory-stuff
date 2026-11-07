use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Maps key chords to actions. Event kind is filtered by the UI shell, rather
/// than included in the binding, so terminal event metadata cannot break lookup.
pub(crate) struct KeyMap<A> {
    bindings: HashMap<(KeyCode, KeyModifiers), A>,
}

impl<A> Default for KeyMap<A> {
    fn default() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }
}

impl<A> KeyMap<A> {
    pub(crate) fn bind(&mut self, code: KeyCode, modifiers: KeyModifiers, action: A) {
        self.bindings.insert((code, modifiers), action);
    }

    pub(crate) fn action(&self, key: KeyEvent) -> Option<&A> {
        self.bindings.get(&(key.code, key.modifiers))
    }
}
