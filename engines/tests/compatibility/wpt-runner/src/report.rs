//! Merging shard results, per-suite summaries, and the regression baseline.
//!
//! The baseline records, per suite, how many tests and subtests pass, plus
//! the list of fully passing tests. A run regresses when a test in that list
//! no longer passes; newly passing tests are reported so the baseline can be
//! raised with `--update-baseline`.
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[derive(Default, Clone, Copy)]
pub struct Tally {
    pub tests: u64,
    pub passing: u64,
    pub subtests: u64,
    pub subtests_passing: u64,
}

pub fn suite_of<'a>(url: &str, suites: &'a [String]) -> &'a str {
    suites
        .iter()
        .filter(|s| url.starts_with(s.trim_end_matches('/')))
        .max_by_key(|s| s.len())
        .map_or("other", |s| s.as_str())
}

pub fn tally(results: &BTreeMap<String, Value>, suites: &[String]) -> BTreeMap<String, Tally> {
    let mut out: BTreeMap<String, Tally> = BTreeMap::new();
    for (url, r) in results {
        let t = out.entry(suite_of(url, suites).to_string()).or_default();
        t.tests += 1;
        t.passing += u64::from(r["status"] == "PASS");
        t.subtests += r["total"].as_u64().unwrap_or(1);
        t.subtests_passing += r["passed"].as_u64().unwrap_or(0);
    }
    out
}

pub fn print(tallies: &BTreeMap<String, Tally>) {
    let pct = |a: u64, b: u64| if b == 0 { 0.0 } else { a as f64 * 100.0 / b as f64 };
    println!(
        "{:<28} {:>14} {:>7} {:>16} {:>7}",
        "suite", "tests", "%", "subtests", "%"
    );
    let mut sum = Tally::default();
    for (suite, t) in tallies {
        println!(
            "{:<28} {:>6}/{:<7} {:>6.1} {:>7}/{:<8} {:>6.1}",
            suite,
            t.passing,
            t.tests,
            pct(t.passing, t.tests),
            t.subtests_passing,
            t.subtests,
            pct(t.subtests_passing, t.subtests)
        );
        sum.tests += t.tests;
        sum.passing += t.passing;
        sum.subtests += t.subtests;
        sum.subtests_passing += t.subtests_passing;
    }
    println!(
        "{:<28} {:>6}/{:<7} {:>6.1} {:>7}/{:<8} {:>6.1}",
        "TOTAL",
        sum.passing,
        sum.tests,
        pct(sum.passing, sum.tests),
        sum.subtests_passing,
        sum.subtests,
        pct(sum.subtests_passing, sum.subtests)
    );
}

pub fn baseline(
    results: &BTreeMap<String, Value>,
    tallies: &BTreeMap<String, Tally>,
    wpt_commit: &str,
) -> Value {
    let suites: BTreeMap<&String, Value> = tallies
        .iter()
        .map(|(s, t)| {
            (
                s,
                json!({"tests": t.tests, "passing": t.passing,
                       "subtests": t.subtests, "subtests_passing": t.subtests_passing}),
            )
        })
        .collect();
    let passing: Vec<&String> = results
        .iter()
        .filter(|(_, r)| r["status"] == "PASS")
        .map(|(u, _)| u)
        .collect();
    json!({"wpt_commit": wpt_commit, "suites": suites, "passing": passing})
}

/// (regressions, newly passing) for the tests that were run.
pub fn compare(results: &BTreeMap<String, Value>, baseline: &Path) -> (Vec<String>, Vec<String>) {
    let Ok(text) = std::fs::read_to_string(baseline) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(base) = serde_json::from_str::<Value>(&text) else {
        return (Vec::new(), Vec::new());
    };
    let before: std::collections::HashSet<&str> = base["passing"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();
    for (url, r) in results {
        let passes = r["status"] == "PASS";
        match (before.contains(url.as_str()), passes) {
            (true, false) => regressions.push(format!("{url}: {}", r["status"])),
            (false, true) => improvements.push(url.clone()),
            _ => {}
        }
    }
    (regressions, improvements)
}
