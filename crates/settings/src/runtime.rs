#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    mpsc_channel_capacity: usize,
}

impl RuntimeSettings {
    pub fn new(channel_capacity: usize) -> Self {
        Self {
            mpsc_channel_capacity: channel_capacity,
        }
    }

    pub fn mpsc_channel_capacity(&self) -> usize {
        self.mpsc_channel_capacity
    }
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            mpsc_channel_capacity: 100,
        }
    }
}
