//! Wega contributors, 2026: bound image admission before decoding and retain
//! charges while decoded bytes/trees survive in nodes, caches or display lists.
use std::sync::{Arc, Mutex, Weak};
const DECODE_RESERVATION: usize = 128 * 1024 * 1024;
const RETAINED_LIMIT: usize = 256 * 1024 * 1024;
const IMAGE_COUNT: usize = 4096;
struct Retained {
    owner: Weak<dyn Send + Sync>,
    bytes: usize,
}
struct Budget {
    decoding: usize,
    retained: Vec<Retained>,
}
static BUDGET: Mutex<Budget> = Mutex::new(Budget {
    decoding: 0,
    retained: Vec::new(),
});
pub(crate) struct DecodePermit {
    active: bool,
}
impl DecodePermit {
    pub fn reserve() -> Result<Self, String> {
        let mut budget = BUDGET
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        budget
            .retained
            .retain(|entry| entry.owner.strong_count() != 0);
        let used: usize = budget.retained.iter().map(|entry| entry.bytes).sum();
        if budget.retained.len() >= IMAGE_COUNT
            || used
                .saturating_add(budget.decoding)
                .saturating_add(DECODE_RESERVATION)
                > RETAINED_LIMIT
        {
            return Err("Decoded-image admission limit exceeded".into());
        }
        budget.decoding += DECODE_RESERVATION;
        Ok(Self { active: true })
    }
    pub fn retain<T: Send + Sync + 'static>(
        mut self,
        owner: &Arc<T>,
        bytes: usize,
    ) -> Result<(), String> {
        if bytes > DECODE_RESERVATION {
            return Err("Decoded-image retention exceeds reservation".into());
        }
        let owner: Arc<dyn Send + Sync> = owner.clone();
        let mut budget = BUDGET
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        budget.retained.push(Retained {
            owner: Arc::downgrade(&owner),
            bytes,
        });
        budget.decoding -= DECODE_RESERVATION;
        self.active = false;
        Ok(())
    }
}
impl Drop for DecodePermit {
    fn drop(&mut self) {
        if self.active {
            BUDGET
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .decoding -= DECODE_RESERVATION;
        }
    }
}
