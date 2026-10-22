use std::marker::PhantomData;

use uuid::Uuid;
#[derive(Debug)]
pub struct Id<T> {
    id: Uuid,

    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub fn from_uuid(id: Uuid) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }
    pub fn new() -> Self {
        Self {
            id: Uuid::now_v7(),
            _marker: PhantomData,
        }
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> Default for Id<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> Eq for Id<T> {}
impl<T> std::hash::Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.id, state);
    }
}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
