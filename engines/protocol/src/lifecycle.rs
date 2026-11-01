//! Host-owned lifecycle. Worker messages cannot reopen a closing or dead session.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Phase {
    #[default]
    Starting,
    Loading,
    Active,
    Frozen,
    Closing,
    Crashed,
    Closed,
}

#[derive(Debug)]
pub struct Lifecycle {
    phase: Phase,
    loading: bool,
    frozen: bool,
    committed: bool,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            phase: Phase::Starting,
            loading: true,
            frozen: false,
            committed: false,
        }
    }
}

impl Lifecycle {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn loading(&self) -> bool {
        self.loading
    }
    pub fn frozen(&self) -> bool {
        self.frozen
    }
    pub fn failed(&self) -> bool {
        self.phase == Phase::Crashed
    }
    pub fn accepts_requests(&self) -> bool {
        !matches!(self.phase, Phase::Closing | Phase::Crashed | Phase::Closed)
    }
    pub fn accepts_content(&self) -> bool {
        self.committed && self.accepts_requests()
    }
    pub fn presents_frames(&self) -> bool {
        self.accepts_content() && !self.frozen
    }

    fn refresh(&mut self) {
        self.phase = if self.frozen {
            Phase::Frozen
        } else if self.loading {
            Phase::Loading
        } else {
            Phase::Active
        };
    }

    pub fn ready(&mut self) {
        if self.phase == Phase::Starting {
            self.refresh();
        }
    }

    pub fn navigate(&mut self) -> bool {
        if !self.accepts_requests() {
            return false;
        }
        self.loading = true;
        self.committed = false;
        self.refresh();
        true
    }

    pub fn commit(&mut self) -> bool {
        if !self.accepts_requests() {
            return false;
        }
        self.committed = true;
        self.refresh();
        true
    }

    pub fn observe_loading(&mut self, loading: bool) {
        if !self.accepts_requests() || (!loading && !self.committed) {
            return;
        }
        self.loading = loading;
        self.refresh();
    }

    pub fn freeze(&mut self, frozen: bool) {
        if !self.accepts_requests() {
            return;
        }
        self.frozen = frozen;
        self.refresh();
    }

    pub fn close(&mut self) {
        if matches!(self.phase, Phase::Closed | Phase::Crashed) {
            return;
        }
        self.phase = Phase::Closing;
        self.loading = false;
        self.committed = false;
    }

    pub fn acknowledge_close(&mut self) -> bool {
        if self.phase != Phase::Closing {
            return false;
        }
        self.phase = Phase::Closed;
        true
    }

    pub fn crash(&mut self) {
        if matches!(self.phase, Phase::Closing | Phase::Closed) {
            return;
        }
        self.phase = Phase::Crashed;
        self.loading = false;
        self.committed = false;
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
