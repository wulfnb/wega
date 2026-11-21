//! `wpt-runner`: run upstream web-platform-tests against the Wega engine.
//!
//!     wpt-runner [--wpt DIR] [--jobs N] [--out DIR] [--save-failures]
//!                [--baseline FILE] [--update-baseline] [SUITE_OR_FILE ...]
//!
//! The WPT checkout defaults to `$WPT_DIR`, then `~/my/wpt`, pinned to the
//! commit in `WPT_COMMIT`. Suites default to `DEFAULT_SUITES`. The parent serves
//! the checkout over loopback HTTP, splits the tests into `--jobs` child
//! processes, restarts a child after a crash or hang, and compares the merged
//! results with `baseline.json`. The exit code is 1 when a test listed as
//! passing in the baseline no longer passes.
//!
//! Reference: `blitz/wpt/runner` (same pin, same suite layout).
#![forbid(unsafe_code)]
mod discover;
mod execute;
mod page;
mod report;
mod server;
mod shard;

use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const DEFAULT_SUITES: &[&str] = &[
    "css/selectors",
    "css/css-cascade",
    "css/css-color",
    "css/css-values",
    "css/mediaqueries",
    "css/css-conditional",
    "css/css-variables",
    "css/css-logical",
    "css/css-flexbox",
    "css/css-grid",
    "css/CSS2/floats",
    "css/CSS2/floats-clear",
    "css/CSS2/tables",
    "css/css-backgrounds",
];

struct Args {
    wpt: PathBuf,
    jobs: usize,
    out: PathBuf,
    save_failures: bool,
    baseline: PathBuf,
    update_baseline: bool,
    suites: Vec<String>,
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("--child") {
        return shard::run(child_args(&raw[1..]));
    }
    let args = parse_args(raw);
    std::process::exit(run_parent(args));
}

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn child_args(args: &[String]) -> shard::Shard {
    let need = |flag| value(args, flag).unwrap_or_else(|| panic!("missing {flag}"));
    let (index, count) = need("--shard").split_once('/').expect("--shard K/N");
    shard::Shard {
        port: need("--port").parse().expect("port"),
        list: need("--list").into(),
        index: index.parse().expect("shard index"),
        count: count.parse().expect("shard count"),
        results: need("--results").into(),
        failures: value(args, "--failures").map(PathBuf::from),
    }
}

fn parse_args(raw: Vec<String>) -> Args {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut args = Args {
        wpt: std::env::var("WPT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(&home).join("my/wpt")),
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get().min(8)),
        out: PathBuf::from("target/wpt"),
        save_failures: false,
        baseline: Path::new(env!("CARGO_MANIFEST_DIR")).join("baseline.json"),
        update_baseline: false,
        suites: Vec::new(),
    };
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--wpt" => args.wpt = it.next().expect("--wpt DIR").into(),
            "--jobs" | "-j" => args.jobs = it.next().and_then(|n| n.parse().ok()).expect("--jobs N"),
            "--out" => args.out = it.next().expect("--out DIR").into(),
            "--save-failures" => args.save_failures = true,
            "--baseline" => args.baseline = it.next().expect("--baseline FILE").into(),
            "--update-baseline" => args.update_baseline = true,
            "-h" | "--help" => {
                println!("{}", include_str!("main.rs").lines().take(13).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            suite => args.suites.push(suite.trim_matches('/').to_string()),
        }
    }
    if args.suites.is_empty() {
        args.suites = DEFAULT_SUITES.iter().map(|s| s.to_string()).collect();
    }
    args.jobs = args.jobs.max(1);
    args
}

fn wpt_commit(wpt: &Path) -> String {
    Command::new("git")
        .arg("-C")
        .arg(wpt)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn run_parent(args: Args) -> i32 {
    let pinned = include_str!("../WPT_COMMIT").trim();
    let commit = wpt_commit(&args.wpt);
    if commit != pinned {
        eprintln!("warning: {} is at {commit:?}, expected pinned {pinned}", args.wpt.display());
    }
    let tests = discover::discover(&args.wpt, &args.suites);
    if tests.is_empty() {
        eprintln!("no tests found under {} for {:?}", args.wpt.display(), args.suites);
        return 2;
    }
    let _ = std::fs::remove_dir_all(&args.out);
    std::fs::create_dir_all(&args.out).expect("output directory");
    let list = args.out.join("tests.json");
    let list_json: Vec<Value> = tests.iter().map(discover::to_json).collect();
    std::fs::write(&list, serde_json::to_string(&list_json).unwrap()).expect("write test list");
    let port = server::start(args.wpt.clone()).expect("start server");
    let jobs = args.jobs.min(tests.len());
    eprintln!(
        "running {} tests from {} suites with {jobs} jobs (WPT served on 127.0.0.1:{port})",
        tests.len(),
        args.suites.len()
    );

    let started = Instant::now();
    let results_of = |k: usize| args.out.join(format!("results-{k}.jsonl"));
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..jobs)
            .map(|k| {
                let results = results_of(k);
                let (list, out, save) = (&list, &args.out, args.save_failures);
                scope.spawn(move || run_shard(k, jobs, port, list, &results, out, save))
            })
            .collect();
        let mut last_report = Instant::now();
        while !handles.iter().all(|h| h.is_finished()) {
            std::thread::sleep(Duration::from_millis(200));
            if last_report.elapsed() >= Duration::from_secs(15) {
                last_report = Instant::now();
                let done: usize = (0..jobs).map(|k| shard::finished(&results_of(k)).len()).sum();
                eprintln!("  {done}/{} after {}s", tests.len(), started.elapsed().as_secs());
            }
        }
    });

    let mut results: BTreeMap<String, Value> = BTreeMap::new();
    for k in 0..jobs {
        for line in shard::read_lines(&results_of(k)) {
            if line.get("status").is_some()
                && let Some(url) = line["url"].as_str()
            {
                results.insert(url.to_string(), line);
            }
        }
    }
    let results_path = args.out.join("results.json");
    std::fs::write(&results_path, serde_json::to_string_pretty(&results).unwrap()).unwrap();
    let tallies = report::tally(&results, &args.suites);
    report::print(&tallies);
    eprintln!("finished in {:?}; results in {}", started.elapsed(), results_path.display());

    let (regressions, improvements) = report::compare(&results, &args.baseline);
    if args.update_baseline {
        write_baseline(&args, &results, &tallies, &commit);
        eprintln!("baseline updated: {}", args.baseline.display());
        return 0;
    }
    if !improvements.is_empty() {
        eprintln!(
            "{} tests now pass that the baseline does not list (raise it with --update-baseline)",
            improvements.len()
        );
    }
    if !regressions.is_empty() {
        eprintln!("{} REGRESSIONS against {}:", regressions.len(), args.baseline.display());
        for r in &regressions {
            eprintln!("  {r}");
        }
        return 1;
    }
    0
}

/// Run one shard to completion, restarting the child after a crash or hang
/// and recording the test that caused it.
fn run_shard(k: usize, n: usize, port: u16, list: &Path, results: &Path, out: &Path, save: bool) {
    let exe = std::env::current_exe().expect("current exe");
    let log = std::fs::File::create(out.join(format!("child-{k}.log"))).expect("child log");
    let mut progress = 0;
    for _attempt in 0..10_000 {
        let mut command = Command::new(&exe);
        command
            .arg("--child")
            .args(["--port", &port.to_string(), "--shard", &format!("{k}/{n}")])
            .arg("--list")
            .arg(list)
            .arg("--results")
            .arg(results)
            .stdout(log.try_clone().unwrap())
            .stderr(log.try_clone().unwrap());
        if save {
            command.arg("--failures").arg(out.join("failures"));
        }
        let status = command.status().expect("spawn child");
        if status.success() {
            return;
        }
        // Attribute the failure to the last test that started but never finished.
        let lines = shard::read_lines(results);
        let finished = shard::finished(results);
        let Some(culprit) = lines
            .iter()
            .rev()
            .filter_map(|l| l["start"].as_str())
            .find(|u| !finished.contains(*u))
        else {
            // The child recorded the crash itself (a caught panic) before exiting.
            if finished.len() > progress {
                progress = finished.len();
                continue;
            }
            eprintln!("shard {k} failed without making progress ({status}); giving up");
            return;
        };
        progress = finished.len() + 1;
        let state = if status.code() == Some(shard::EXIT_TIMEOUT) {
            "TIMEOUT"
        } else {
            "CRASH"
        };
        let line = json!({"url": culprit, "status": state, "passed": 0, "total": 1,
                          "message": format!("child exited: {status}")});
        let mut text = std::fs::read_to_string(results).unwrap_or_default();
        text.push_str(&format!("{line}\n"));
        std::fs::write(results, text).expect("record crash");
    }
}

fn write_baseline(
    args: &Args,
    results: &BTreeMap<String, Value>,
    tallies: &BTreeMap<String, report::Tally>,
    commit: &str,
) {
    let mut baseline = report::baseline(results, tallies, commit);
    // Keep suites that this run did not cover.
    if let Ok(old) = std::fs::read_to_string(&args.baseline)
        && let Ok(old) = serde_json::from_str::<Value>(&old)
    {
        let covered = |url: &str| report::suite_of(url, &args.suites) != "other";
        let mut passing: Vec<Value> = old["passing"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|u| u.as_str().is_some_and(|u| !covered(u)))
            .cloned()
            .collect();
        passing.extend(baseline["passing"].as_array().cloned().unwrap_or_default());
        passing.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        baseline["passing"] = Value::Array(passing);
        if let Some(old_suites) = old["suites"].as_object() {
            for (suite, value) in old_suites {
                if !args.suites.contains(suite) {
                    baseline["suites"][suite] = value.clone();
                }
            }
        }
    }
    std::fs::write(&args.baseline, serde_json::to_string_pretty(&baseline).unwrap() + "\n")
        .expect("write baseline");
}
