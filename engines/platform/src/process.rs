//! Own a child handle through actual exit and reaping, without waiting on the UI thread.
#![forbid(unsafe_code)]
use std::{
    io,
    process::{Child, ExitStatus},
    sync::{Arc, Mutex, OnceLock},
    thread::JoinHandle,
    time::{Duration, Instant},
};

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
    child: Option<Child>,
    pid: u32,
    grace: Duration,
    observer: Option<Observer>,
}

impl OwnedProcess {
    pub fn new(child: Child, observer: impl FnOnce(ExitReport) + Send + 'static) -> Self {
        Self {
            pid: child.id(),
            child: Some(child),
            grace: Duration::ZERO,
            observer: Some(Box::new(observer)),
        }
    }
    pub fn id(&self) -> u32 {
        self.pid
    }
    pub fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.child.as_mut()?.stdin.take()
    }
    pub fn take_stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.as_mut()?.stdout.take()
    }
    pub fn take_stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.child.as_mut()?.stderr.take()
    }
    /// The caller must first successfully enqueue the negotiated shutdown command.
    pub fn allow_graceful_shutdown(&mut self, grace: Duration) {
        self.grace = grace.min(MAX_SHUTDOWN_GRACE);
    }
}

struct Reaper {
    child: Child,
    deadline: Instant,
    forced: bool,
    observer: Option<Observer>,
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
            if self.forced || Instant::now() >= self.deadline {
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
                forced: self.forced,
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
        let Some(mut child) = self.child.take() else {
            return;
        };
        let forced =
            self.grace.is_zero() && matches!(child.try_wait(), Ok(None)) && child.kill().is_ok();
        let reaper = Reaper {
            child,
            deadline: Instant::now() + self.grace,
            forced,
            observer: self.observer.take(),
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
