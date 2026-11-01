//! Own a child handle through actual exit and reaping, without waiting on the UI thread.
#![forbid(unsafe_code)]
use std::{
    io,
    process::ExitStatus,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crate::child::ChildOwner;

pub const MAX_SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub struct ExitReport {
    pub pid: u32,
    pub forced: bool,
    pub status: io::Result<ExitStatus>,
}

type Observer = Box<dyn FnOnce(ExitReport) + Send>;
static REAPERS: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();

/// Call after stopping admission and dropping all owners, outside the UI event loop.
pub fn wait_for_shutdowns() {
    loop {
        let tasks = std::mem::take(
            &mut *REAPERS
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if tasks.is_empty() {
            break;
        }
        for task in tasks {
            let _ = task.join();
        }
    }
}

pub struct OwnedProcess {
    child: Arc<Mutex<Option<ChildOwner>>>,
    terminate: Arc<AtomicBool>,
    killed: Arc<AtomicBool>,
    retained: Vec<Box<dyn Send>>,
    pid: u32,
    grace: Duration,
    observer: Option<Observer>,
}

impl OwnedProcess {
    pub fn new(
        child: impl Into<ChildOwner>,
        observer: impl FnOnce(ExitReport) + Send + 'static,
    ) -> Self {
        let child = child.into();
        Self {
            pid: child.id(),
            child: Arc::new(Mutex::new(Some(child))),
            terminate: Arc::new(AtomicBool::new(false)),
            killed: Arc::new(AtomicBool::new(false)),
            retained: Vec::new(),
            grace: Duration::ZERO,
            observer: Some(Box::new(observer)),
        }
    }
    pub fn id(&self) -> u32 {
        self.pid
    }
    pub fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()?
            .take_stdin()
    }
    pub fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()?
            .take_stdout()
    }
    pub fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()?
            .take_stderr()
    }
    /// The caller must first successfully enqueue the negotiated shutdown command.
    pub fn allow_graceful_shutdown(&mut self, grace: Duration) {
        self.grace = grace.min(MAX_SHUTDOWN_GRACE);
    }
    /// Cancellation uses the owned process handle; it never targets a stored PID.
    pub fn control(&self) -> ProcessControl {
        ProcessControl {
            child: Arc::downgrade(&self.child),
            terminate: self.terminate.clone(),
            killed: self.killed.clone(),
        }
    }
    /// Budgets, private directories and containment handles outlive the page owner.
    /// They are released only after the reaper observes actual OS process exit.
    pub fn retain_until_exit(&mut self, value: impl Send + 'static) {
        self.retained.push(Box::new(value));
    }

    #[cfg(windows)]
    pub(crate) fn resume(&self, job: &std::os::windows::io::OwnedHandle) -> io::Result<()> {
        self.with_child(|child| child.resume(job))
    }
    #[cfg(windows)]
    pub(crate) fn with_child<T>(
        &self,
        action: impl FnOnce(&ChildOwner) -> io::Result<T>,
    ) -> io::Result<T> {
        let child = self
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        action(
            child
                .as_ref()
                .ok_or_else(|| io::Error::other("Process is already reaping"))?,
        )
    }
}

#[derive(Clone)]
pub struct ProcessControl {
    child: Weak<Mutex<Option<ChildOwner>>>,
    terminate: Arc<AtomicBool>,
    killed: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample {
    Alive {
        resident_bytes: Option<u64>,
    },
    Exited,
    /// The reaper retains ownership; its exit guard still retains all reservations.
    Reaping,
    Busy,
}
impl ProcessControl {
    pub fn sample(&self) -> io::Result<Sample> {
        let Some(shared) = self.child.upgrade() else {
            return Ok(Sample::Reaping);
        };
        let mut owner = match shared.try_lock() {
            Ok(owner) => owner,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(Sample::Busy),
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        };
        let Some(child) = owner.as_mut() else {
            return Ok(Sample::Reaping);
        };
        if child.try_wait()?.is_some() {
            return Ok(Sample::Exited);
        }
        // The child is unreaped while this lock is held, preventing PID reuse on Unix.
        Ok(Sample::Alive {
            resident_bytes: crate::resident_for_child(child),
        })
    }
    pub fn terminate(&self) -> io::Result<()> {
        self.terminate.store(true, Ordering::Release);
        let Some(shared) = self.child.upgrade() else {
            return Ok(());
        };
        let mut owner = match shared.try_lock() {
            Ok(owner) => owner,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        };
        if let Some(child) = owner.as_mut()
            && child.try_wait()?.is_none()
        {
            child.kill()?;
            self.killed.store(true, Ordering::Release);
        }
        Ok(())
    }
}

struct Reaper {
    child: ChildOwner,
    deadline: Instant,
    forced: bool,
    observer: Option<Observer>,
    terminate: Arc<AtomicBool>,
    killed: Arc<AtomicBool>,
    _retained: Vec<Box<dyn Send>>,
}

impl Reaper {
    fn run(mut self) {
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => break Err(error),
                Ok(None) => {}
            }
            if self.forced
                || self.terminate.load(Ordering::Acquire)
                || Instant::now() >= self.deadline
            {
                if !self.forced {
                    self.forced = self.child.kill().is_ok();
                }
                break self.child.wait();
            }
            std::thread::sleep(
                Duration::from_millis(10)
                    .min(self.deadline.saturating_duration_since(Instant::now())),
            );
        };
        if let Some(observer) = self.observer.take() {
            observer(ExitReport {
                pid: self.child.id(),
                forced: self.forced || self.killed.load(Ordering::Acquire),
                status,
            });
        }
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        // Also keep ownership safe if spawning the reaper or its observer panics/fails.
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let Some(mut child) = self
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return;
        };
        let forced =
            self.grace.is_zero() && matches!(child.try_wait(), Ok(None)) && child.kill().is_ok();
        let reaper = Reaper {
            child,
            deadline: Instant::now() + self.grace,
            forced,
            observer: self.observer.take(),
            terminate: self.terminate.clone(),
            killed: self.killed.clone(),
            _retained: std::mem::take(&mut self.retained),
        };
        // Keep a recoverable owner outside the closure if the OS refuses a new thread.
        let pending = Arc::new(Mutex::new(Some(reaper)));
        let task = pending.clone();
        match std::thread::Builder::new()
            .name(format!("wega-reap-{}", self.pid))
            .spawn(move || {
                let reaper = task
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(reaper) = reaper {
                    reaper.run();
                }
            }) {
            Ok(task) => {
                let mut tasks = REAPERS
                    .get_or_init(Default::default)
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                tasks.retain(|task| !task.is_finished());
                tasks.push(task);
            }
            Err(_) => {
                // Exceptional thread exhaustion: terminate and reap synchronously rather
                // than silently orphaning the owned process handle.
                if let Some(mut reaper) = pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    reaper.deadline = Instant::now();
                    reaper.run();
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
