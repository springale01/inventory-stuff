use std::num::NonZeroUsize;

#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    mpsc_channel_capacity: NonZeroUsize,
}

impl RuntimeSettings {
    pub fn new(channel_capacity: NonZeroUsize) -> Self {
        Self {
            mpsc_channel_capacity: channel_capacity,
        }
    }

    pub fn mpsc_channel_capacity(&self) -> usize {
        self.mpsc_channel_capacity.get()
    }
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            mpsc_channel_capacity: NonZeroUsize::new(100)
                .expect("Wait the NonZeroUsize is hardcoded tho"),
        }
    }
}
