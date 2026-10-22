use crate::InventoryError;

/// A one-based page number and a bounded number of rows per page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    number: u64,
    size: u32,
}

impl PageRequest {
    pub const MAX_SIZE: u32 = 200;

    pub fn new(number: u64, size: u32) -> Result<Self, InventoryError> {
        if number == 0 {
            return Err(InventoryError::InvalidInput("page numbers start at 1"));
        }
        if size == 0 || size > Self::MAX_SIZE {
            return Err(InventoryError::InvalidInput(
                "page size must be between 1 and 200",
            ));
        }
        let offset = (number - 1)
            .checked_mul(u64::from(size))
            .and_then(|value| i64::try_from(value).ok());
        if offset.is_none() {
            return Err(InventoryError::InvalidInput(
                "page offset exceeds SQLite's supported range",
            ));
        }
        Ok(Self { number, size })
    }

    pub fn number(self) -> u64 {
        self.number
    }
    pub fn size(self) -> u32 {
        self.size
    }

    pub(crate) fn offset(self) -> i64 {
        // Validated at construction; fields are private.
        ((self.number - 1) * u64::from(self.size)) as i64
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            number: 1,
            size: 25,
        }
    }
}

#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total_items: u64,
    request: PageRequest,
}

impl<T> Page<T> {
    pub(crate) fn new(items: Vec<T>, total_items: u64, request: PageRequest) -> Self {
        Self {
            items,
            total_items,
            request,
        }
    }

    pub fn request(&self) -> PageRequest {
        self.request
    }

    /// Zero when the result set is empty.
    pub fn total_pages(&self) -> u64 {
        self.total_items.div_ceil(u64::from(self.request.size))
    }

    pub fn next_page(&self) -> Option<PageRequest> {
        if self.request.number < self.total_pages() {
            PageRequest::new(self.request.number + 1, self.request.size).ok()
        } else {
            None
        }
    }

    /// Also allows navigating back from a requested page beyond the last page.
    pub fn previous_page(&self) -> Option<PageRequest> {
        if self.request.number > 1 {
            PageRequest::new(self.request.number - 1, self.request.size).ok()
        } else {
            None
        }
    }
}
