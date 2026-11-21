//! A child process that runs every Nth test of the list. Results are appended
//! to a JSON-lines file as they finish, preceded by a `start` line, so the
//! parent can attribute a crash or hang to the test that caused it and
//! restart the shard where it stopped.
use crate::{discover, execute};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use wega_engine::{Engine, Limits, NetworkPolicy, Url};

/// A test running longer than this is killed and reported as a timeout.
pub const HARD_TIMEOUT: Duration = Duration::from_secs(45);
pub const EXIT_TIMEOUT: i32 = 3;
pub const EXIT_CRASH: i32 = 4;

pub struct Shard {
    pub port: u16,
    pub list: PathBuf,
    pub index: usize,
    pub count: usize,
    pub results: PathBuf,
    pub failures: Option<PathBuf>,
}

pub fn finished(results: &Path) -> HashSet<String> {
    read_lines(results)
        .into_iter()
        .filter(|v| v.get("status").is_some())
        .filter_map(|v| v["url"].as_str().map(str::to_string))
        .collect()
}

pub fn read_lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub fn run(shard: Shard) {
    let list: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&shard.list).expect("test list"))
            .expect("test list json");
    let done = finished(&shard.results);
    let tests: Vec<_> = list
        .iter()
        .enumerate()
        .filter(|(i, _)| i % shard.count == shard.index)
        .filter_map(|(_, v)| discover::from_json(v))
        .filter(|t| !done.contains(&t.url))
        .collect();
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&shard.results)
        .expect("results file");

    let engine = Engine::new(
        Limits {
            script_time: Duration::from_secs(5),
            ..Limits::default()
        },
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .expect("engine");
    let mut cx = execute::Context {
        engine: &engine,
        base: Url::parse(&format!("http://127.0.0.1:{}/", shard.port)).unwrap(),
        failures: shard.failures.as_deref(),
        references: HashMap::new(),
    };

    let current: Arc<Mutex<Option<Instant>>> = Arc::default();
    let watchdog = current.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let started = *watchdog.lock().unwrap_or_else(|e| e.into_inner());
            if started.is_some_and(|s| s.elapsed() > HARD_TIMEOUT) {
                std::process::exit(EXIT_TIMEOUT);
            }
        }
    });

    for test in tests {
        let _ = writeln!(out, "{}", json!({"start": test.url}));
        let _ = out.flush();
        *current.lock().unwrap() = Some(Instant::now());
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute::run(&mut cx, &test)));
        *current.lock().unwrap() = None;
        match outcome {
            Ok(result) => {
                let _ = writeln!(out, "{result}");
                let _ = out.flush();
            }
            Err(panic) => {
                let message = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "{}",
                    json!({"url": test.url, "status": "CRASH", "passed": 0, "total": 1, "message": message})
                );
                let _ = out.flush();
                // A panic can leave V8 or the document in a bad state: restart.
                std::process::exit(EXIT_CRASH);
            }
        }
    }
}
