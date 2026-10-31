//! Browser-owned resource accounting and OS process monitoring. Renderer telemetry
//! is never used to decide whether a process stays within its budget.
#![forbid(unsafe_code)]
use crate::process::{OwnedProcess, ProcessControl, Sample};
use std::{
    collections::BTreeMap,
    fmt, io,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub struct Budgets {
    pub workers: usize,
    pub worker_resident_bytes: u64,
    pub aggregate_resident_bytes: u64,
    pub worker_frame_bytes: u64,
    pub aggregate_frame_bytes: u64,
    pub startup_timeout: Duration,
}
impl Default for Budgets {
    fn default() -> Self {
        Self {
            workers: 8,
            worker_resident_bytes: 512 * 1024 * 1024,
            aggregate_resident_bytes: 2 * 1024 * 1024 * 1024,
            worker_frame_bytes: 2 * 4096 * 2160 * 4,
            aggregate_frame_bytes: 1024 * 1024 * 1024,
            startup_timeout: Duration::from_secs(20),
        }
    }
}
impl Budgets {
    fn validate(&self) -> io::Result<()> {
        if self.workers == 0
            || self.worker_resident_bytes == 0
            || self.aggregate_resident_bytes < self.worker_resident_bytes
            || self.worker_frame_bytes == 0
            || self.aggregate_frame_bytes < self.worker_frame_bytes
            || self.startup_timeout.is_zero()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid content-process budgets",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    StartupTimeout,
    ResidentMemory,
    AggregateMemory,
    UnavailableMetrics,
    ProcessMonitor,
    ProcessExit,
}
impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::StartupTimeout => "The content process did not finish starting in time",
            Self::ResidentMemory => "The content process exceeded its memory budget",
            Self::AggregateMemory => "Wega content processes exceeded their shared memory budget",
            Self::UnavailableMetrics => "The host could not measure the content process's memory",
            Self::ProcessMonitor => "The host could not monitor the content process",
            Self::ProcessExit => "The content process exited",
        })
    }
}
type Failure = Box<dyn FnOnce(Violation) + Send>;
struct Entry {
    frames: u64,
    started: Option<Instant>,
    ready: bool,
    exited: bool,
    resident: u64,
    missing_metrics: u8,
    terminating: bool,
    control: Option<ProcessControl>,
    failure: Option<Failure>,
}
#[derive(Default)]
struct State {
    next: u64,
    entries: BTreeMap<u64, Entry>,
}
pub struct Supervisor {
    budgets: Budgets,
    state: Mutex<State>,
}
static SHARED: OnceLock<Result<Arc<Supervisor>, String>> = OnceLock::new();

impl Supervisor {
    pub fn shared() -> Result<Arc<Self>, String> {
        SHARED
            .get_or_init(|| Self::new(Budgets::default()).map_err(|error| error.to_string()))
            .clone()
    }
    pub fn new(budgets: Budgets) -> io::Result<Arc<Self>> {
        budgets.validate()?;
        let owner = Arc::new(Self {
            budgets,
            state: Mutex::new(State::default()),
        });
        let weak = Arc::downgrade(&owner);
        std::thread::Builder::new()
            .name("wega-content-supervisor".into())
            .spawn(move || {
                loop {
                    let Some(owner) = weak.upgrade() else {
                        break;
                    };
                    owner.poll(Instant::now());
                    drop(owner);
                    std::thread::sleep(SAMPLE_INTERVAL);
                }
            })?;
        Ok(owner)
    }
    /// Reserve before spawning or allocating frame storage. A stopped tab still
    /// retains its frame reservation until its presentation and process owners drop.
    pub fn reserve(self: &Arc<Self>, frame_bytes: u64) -> io::Result<Ticket> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let frames = state
            .entries
            .values()
            .try_fold(frame_bytes, |sum, entry| sum.checked_add(entry.frames));
        if frame_bytes > self.budgets.worker_frame_bytes
            || frames.is_none_or(|frames| frames > self.budgets.aggregate_frame_bytes)
            || state.entries.values().filter(|entry| !entry.exited).count() >= self.budgets.workers
        {
            return Err(io::Error::other(
                "Wega content-process capacity reached. Close another Wega tab and try again.",
            ));
        }
        let next = state
            .next
            .checked_add(1)
            .ok_or_else(|| io::Error::other("Supervisor identity exhausted"))?;
        state.next = next;
        state.entries.insert(
            next,
            Entry {
                frames: frame_bytes,
                started: None,
                ready: false,
                exited: false,
                resident: 0,
                missing_metrics: 0,
                terminating: false,
                control: None,
                failure: None,
            },
        );
        Ok(Ticket(Arc::new(Reservation {
            owner: self.clone(),
            id: next,
        })))
    }
    fn poll(&self, now: Instant) {
        let controls: Vec<_> = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                (!entry.exited)
                    .then(|| entry.control.clone().map(|control| (*id, control)))
                    .flatten()
            })
            .collect();
        let samples: Vec<_> = controls
            .iter()
            .map(|(id, control)| (*id, control.sample()))
            .collect();
        let mut failures = Vec::new();
        let mut terminate = Vec::new();
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (id, sample) in samples {
                let Some(entry) = state.entries.get_mut(&id) else {
                    continue;
                };
                let mut violation = None;
                match sample {
                    Ok(Sample::Exited) => {
                        entry.exited = true;
                        entry.resident = 0;
                        Self::mark(entry, Violation::ProcessExit, &mut failures);
                        continue;
                    }
                    Ok(Sample::Reaping) => continue, // The reaper's retained Ticket still owns this reservation.
                    Ok(Sample::Busy) => {}
                    Ok(Sample::Alive {
                        resident_bytes: Some(bytes),
                    }) => {
                        entry.resident = bytes;
                        entry.missing_metrics = 0;
                    }
                    Ok(Sample::Alive {
                        resident_bytes: None,
                    }) => {
                        entry.missing_metrics = entry.missing_metrics.saturating_add(1);
                        if entry.missing_metrics >= 3 {
                            violation = Some(Violation::UnavailableMetrics);
                        }
                    }
                    Err(_) => violation = Some(Violation::ProcessMonitor),
                }
                if !entry.ready
                    && entry.started.is_some_and(|start| {
                        now.saturating_duration_since(start) >= self.budgets.startup_timeout
                    })
                {
                    violation = Some(Violation::StartupTimeout);
                }
                if entry.resident > self.budgets.worker_resident_bytes {
                    violation = Some(Violation::ResidentMemory);
                }
                if let Some(reason) = violation {
                    Self::mark(entry, reason, &mut failures);
                }
            }
            // Deterministic pressure relief: stop the largest active consumer first.
            let total = state
                .entries
                .values()
                .filter(|entry| !entry.exited)
                .fold(0_u64, |sum, entry| sum.saturating_add(entry.resident));
            if total > self.budgets.aggregate_resident_bytes {
                let reclaiming = state
                    .entries
                    .values()
                    .filter(|entry| entry.terminating && !entry.exited)
                    .fold(0_u64, |sum, entry| sum.saturating_add(entry.resident));
                let mut needed = total
                    .saturating_sub(reclaiming)
                    .saturating_sub(self.budgets.aggregate_resident_bytes);
                let mut candidates: Vec<_> = state
                    .entries
                    .iter()
                    .filter(|(_, entry)| {
                        !entry.exited && !entry.terminating && entry.control.is_some()
                    })
                    .map(|(id, entry)| (entry.resident, *id))
                    .collect();
                candidates.sort_unstable_by(|left, right| right.cmp(left));
                for (bytes, id) in candidates {
                    if needed == 0 {
                        break;
                    }
                    Self::mark(
                        state.entries.get_mut(&id).unwrap(),
                        Violation::AggregateMemory,
                        &mut failures,
                    );
                    needed = needed.saturating_sub(bytes);
                }
            }
            for entry in state
                .entries
                .values()
                .filter(|entry| entry.terminating && !entry.exited)
            {
                if let Some(control) = &entry.control {
                    terminate.push(control.clone());
                }
            }
        }
        // Revoke host grants before killing; callbacks never run under the accounting lock.
        for (failure, reason) in failures {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| failure(reason)));
        }
        for control in terminate {
            let _ = control.terminate();
        }
    }
    fn mark(entry: &mut Entry, reason: Violation, failures: &mut Vec<(Failure, Violation)>) {
        entry.terminating = true;
        if let Some(failure) = entry.failure.take() {
            failures.push((failure, reason));
        }
    }
}

/// A lease, never a PID. Keep one with presentation resources; attach() also
/// retains a copy in the process reaper until actual OS exit.
#[derive(Clone)]
pub struct Ticket(Arc<Reservation>);
struct Reservation {
    owner: Arc<Supervisor>,
    id: u64,
}
impl Ticket {
    pub fn worker_memory_limit(&self) -> u64 {
        self.0.owner.budgets.worker_resident_bytes
    }
    pub fn attach(
        &self,
        process: &mut OwnedProcess,
        failure: impl FnOnce(Violation) + Send + 'static,
    ) -> io::Result<()> {
        let mut state = self
            .0
            .owner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = state
            .entries
            .get_mut(&self.0.id)
            .ok_or_else(|| io::Error::other("Supervisor reservation expired"))?;
        if entry.control.is_some() {
            return Err(io::Error::other("Supervisor reservation already attached"));
        }
        entry.control = Some(process.control());
        entry.started = Some(Instant::now());
        entry.failure = Some(Box::new(failure));
        process.retain_until_exit(self.clone());
        Ok(())
    }
    pub fn ready(&self) {
        if let Some(entry) = self
            .0
            .owner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .get_mut(&self.0.id)
            && entry.control.is_some()
            && !entry.terminating
            && !entry.exited
        {
            entry.ready = true;
        }
    }
    pub fn terminate(&self) {
        if let Some(entry) = self
            .0
            .owner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .get_mut(&self.0.id)
        {
            entry.terminating = true;
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.owner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .remove(&self.id);
    }
}

#[cfg(test)]
#[path = "supervision_tests.rs"]
mod tests;
