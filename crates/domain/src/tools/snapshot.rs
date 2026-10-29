#[derive(Debug, Clone)]
pub struct Snapshot<T> {
    value: T,
}

impl<T> Snapshot<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: Clone> From<&T> for Snapshot<T> {
    fn from(value: &T) -> Self {
        Self::new(value.clone())
    }
}
