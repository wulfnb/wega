//! Low-frequency resource sampling; no subprocesses or system-wide process scans.
#![forbid(unsafe_code)]
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use wega_platform::supervision::WorkerUsage;

const INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct Sample {
    cpu_seconds: f64,
    memory_bytes: u64,
}

#[derive(Clone, Copy)]
enum Scope {
    Process,
    #[cfg(target_os = "linux")]
    Container,
}

pub struct Monitor {
    scope: Scope,
    previous: Option<Sample>,
    previous_workers: BTreeMap<u64, Duration>,
    sampled_at: Instant,
    started_at: Instant,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
}

impl Monitor {
    pub fn new() -> Self {
        let scope = Scope::Process;
        #[cfg(target_os = "linux")]
        let scope = if std::env::var("BROWSER_METRICS_SCOPE").as_deref() == Ok("container")
            && container_sample().is_some()
        {
            Scope::Container
        } else {
            scope
        };
        let initial = sample(scope);
        let workers = worker_usage(scope);
        let now = Instant::now();
        Self {
            scope,
            previous: initial,
            previous_workers: worker_clocks(&workers),
            sampled_at: now,
            started_at: now,
            cpu_percent: None,
            memory_bytes: initial.and_then(|sample| combined_memory(sample.memory_bytes, &workers)),
        }
    }

    pub fn refresh(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.sampled_at);
        if elapsed < INTERVAL {
            return false;
        }
        let current = sample(self.scope);
        let workers = worker_usage(self.scope);
        self.cpu_percent = current.zip(self.previous).and_then(|(new, old)| {
            let browser = cpu_percent(old.cpu_seconds, new.cpu_seconds, elapsed.as_secs_f64())?;
            let content = worker_cpu_percent(
                &self.previous_workers,
                &workers,
                self.sampled_at,
                elapsed.as_secs_f64(),
            )?;
            Some(browser + content).filter(|value| value.is_finite())
        });
        self.memory_bytes =
            current.and_then(|sample| combined_memory(sample.memory_bytes, &workers));
        self.previous_workers = worker_clocks(&workers);
        self.previous = current;
        self.sampled_at = now;
        true
    }

    /// When `refresh` will next take a sample.
    pub fn next_sample(&self) -> Instant {
        self.sampled_at + INTERVAL
    }

    pub fn label(&self) -> String {
        let cpu = self
            .cpu_percent
            .map_or_else(|| "—".into(), |v| format!("{v:.1}%"));
        let ram = self
            .memory_bytes
            .map_or_else(|| "—".into(), |v| format!("{} MiB", v / 1_048_576));
        format!("CPU {cpu} · RAM {ram}")
    }

    pub fn description(&self) -> String {
        let scope = match self.scope {
            Scope::Process => {
                "Browser process and its Wega content processes. Summed resident memory can count shared pages more than once."
            }
            #[cfg(target_os = "linux")]
            Scope::Container => {
                "Whole Docker container, including the browser, display and viewer. RAM excludes inactive file cache, as in docker stats."
            }
        };
        let seconds = self.started_at.elapsed().as_secs();
        format!(
            "{scope}\nUpdated every 2 seconds. CPU: 100% = one fully used core; multiple cores can exceed 100%.\nRAM is current resident/working memory, not peak usage.\nBrowser uptime: {}m {:02}s",
            seconds / 60,
            seconds % 60
        )
    }
}

fn worker_usage(scope: Scope) -> Vec<WorkerUsage> {
    if matches!(scope, Scope::Process) {
        wega_platform::supervision::shared_usage()
    } else {
        Vec::new()
    }
}
fn worker_clocks(workers: &[WorkerUsage]) -> BTreeMap<u64, Duration> {
    workers
        .iter()
        .filter_map(|worker| Some((worker.id, worker.cpu_time?)))
        .collect()
}
fn combined_memory(browser: u64, workers: &[WorkerUsage]) -> Option<u64> {
    workers.iter().try_fold(browser, |total, worker| {
        total.checked_add(worker.resident_bytes?)
    })
}
fn worker_cpu_percent(
    previous: &BTreeMap<u64, Duration>,
    workers: &[WorkerUsage],
    since: Instant,
    seconds: f64,
) -> Option<f64> {
    workers.iter().try_fold(0.0, |total, worker| {
        let current = worker.cpu_time?;
        let old = match previous.get(&worker.id) {
            Some(old) => *old,
            None if worker.started >= since => Duration::ZERO,
            None => return None,
        };
        Some(total + cpu_percent(old.as_secs_f64(), current.as_secs_f64(), seconds)?)
    })
}

fn cpu_percent(previous: f64, current: f64, seconds: f64) -> Option<f64> {
    if current < previous || seconds <= 0.0 {
        return None;
    }
    let value = (current - previous) / seconds * 100.0;
    value.is_finite().then_some(value)
}

fn sample(scope: Scope) -> Option<Sample> {
    match scope {
        Scope::Process => process_sample(),
        #[cfg(target_os = "linux")]
        Scope::Container => container_sample(),
    }
}

fn process_sample() -> Option<Sample> {
    Some(Sample {
        cpu_seconds: wega_platform::cpu_seconds()?,
        memory_bytes: wega_platform::resident_bytes()?,
    })
}

#[cfg(any(target_os = "linux", test))]
fn counter(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let mut values = line.split_whitespace();
        (values.next()? == key)
            .then(|| values.next()?.parse().ok())
            .flatten()
    })
}

#[cfg(target_os = "linux")]
fn container_sample() -> Option<Sample> {
    let cpu = std::fs::read_to_string("/sys/fs/cgroup/cpu.stat").ok()?;
    let memory = std::fs::read_to_string("/sys/fs/cgroup/memory.current").ok()?;
    let stats = std::fs::read_to_string("/sys/fs/cgroup/memory.stat").ok()?;
    container_usage(&cpu, &memory, &stats)
}

#[cfg(any(target_os = "linux", test))]
fn container_usage(cpu: &str, memory: &str, stats: &str) -> Option<Sample> {
    Some(Sample {
        cpu_seconds: counter(cpu, "usage_usec")? as f64 / 1_000_000.0,
        memory_bytes: memory
            .trim()
            .parse::<u64>()
            .ok()?
            .saturating_sub(counter(stats, "inactive_file")?),
    })
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
