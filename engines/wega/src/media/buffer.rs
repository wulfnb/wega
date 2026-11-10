//! Bounded buffer management and memory accounting for media streams.

#[derive(Clone, Debug, PartialEq)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug)]
pub struct MediaBufferManager {
    max_bytes: usize,
    allocated_bytes: usize,
    ranges: Vec<TimeRange>,
}

impl MediaBufferManager {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            allocated_bytes: 0,
            ranges: Vec::new(),
        }
    }

    pub fn allocate(&mut self, bytes: usize) -> Result<(), &'static str> {
        if self.allocated_bytes + bytes > self.max_bytes {
            return Err("QuotaExceededError: Media buffer limit reached");
        }
        self.allocated_bytes += bytes;
        Ok(())
    }

    pub fn deallocate(&mut self, bytes: usize) {
        self.allocated_bytes = self.allocated_bytes.saturating_sub(bytes);
    }

    pub fn allocated_bytes(&self) -> usize {
        self.allocated_bytes
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    pub fn set_buffered_range(&mut self, start: f64, end: f64) {
        self.ranges.clear();
        if end > start {
            self.ranges.push(TimeRange { start, end });
        }
    }

    pub fn ranges(&self) -> &[TimeRange] {
        &self.ranges
    }
}
