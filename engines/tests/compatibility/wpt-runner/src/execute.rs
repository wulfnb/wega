//! Running one test: testharness tests report through the completion callback
//! that `server::TESTHARNESS_REPORT` installs; reftests compare screenshots.
//!
//! Reference: `blitz/wpt/runner/src/test_runners/{harness_test,ref_test,fuzzy}.rs`.
use crate::{
    discover::{Fuzzy, Kind, Test},
    page,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};
use wega_engine::{Engine, Url};

pub struct Context<'a> {
    pub engine: &'a Engine,
    pub base: Url,
    pub failures: Option<&'a Path>,
    /// Reference screenshots shared by many tests.
    pub references: HashMap<String, Vec<u8>>,
}

pub fn run(cx: &mut Context, test: &Test) -> Value {
    let started = Instant::now();
    let mut result = match &test.kind {
        Kind::Harness => harness(cx, test),
        Kind::Ref {
            matches,
            mismatches,
            fuzzy,
        } => reftest(cx, test, matches, mismatches, *fuzzy),
    };
    result["url"] = json!(test.url);
    result["ms"] = json!(started.elapsed().as_millis() as u64);
    result
}

fn url(cx: &Context, path: &str) -> Result<Url, String> {
    cx.base.join(path).map_err(|e| e.to_string())
}

fn failed(message: String) -> Value {
    let status = if message.starts_with("TIMEOUT") {
        "TIMEOUT"
    } else {
        "ERROR"
    };
    json!({"status": status, "passed": 0, "total": 1, "message": message})
}

fn harness(cx: &mut Context, test: &Test) -> Value {
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut view = match url(cx, &test.url).and_then(|u| page::load(cx.engine, &u, deadline)) {
        Ok(view) => view,
        Err(e) => return failed(e),
    };
    let report = loop {
        if let Err(e) = page::pump(&mut view) {
            return failed(e);
        }
        match view.evaluate_json("globalThis.__wptResults ?? null") {
            Ok(r) if r != "null" => match serde_json::from_str::<Value>(&r) {
                Ok(v) => break v,
                Err(e) => return failed(format!("bad report: {e}")),
            },
            Ok(_) => {}
            Err(e) => return failed(e),
        }
        if Instant::now() > deadline {
            return failed("TIMEOUT: no harness completion".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let tests = report["tests"].as_array().cloned().unwrap_or_default();
    let passed = tests.iter().filter(|t| t["status"] == 0).count();
    let total = tests.len();
    let status = match report["status"].as_u64() {
        Some(0) if total > 0 && passed == total => "PASS",
        Some(0) => "FAIL",
        Some(2) => "TIMEOUT",
        Some(3) => "PRECONDITION_FAILED",
        _ => "ERROR",
    };
    let failures: Vec<Value> = tests
        .iter()
        .filter(|t| t["status"] != 0)
        .take(20)
        .map(|t| json!({"name": t["name"], "status": t["status"], "message": t["message"]}))
        .collect();
    json!({
        "status": status,
        "passed": passed,
        "total": total,
        "message": report["message"],
        "failures": failures,
    })
}

fn reftest(
    cx: &mut Context,
    test: &Test,
    matches: &[String],
    mismatches: &[String],
    fuzzy: Option<Fuzzy>,
) -> Value {
    let actual = match url(cx, &test.url).and_then(|u| page::screenshot(cx.engine, &u)) {
        Ok(pixels) => pixels,
        Err(e) => return failed(e),
    };
    let mut message = None;
    for (reference, expect_match) in matches
        .iter()
        .map(|r| (r, true))
        .chain(mismatches.iter().map(|r| (r, false)))
    {
        let expected = match reference_pixels(cx, reference) {
            Ok(pixels) => pixels,
            Err(e) => return failed(format!("reference {reference}: {e}")),
        };
        let (max_difference, total_pixels) = compare(&actual, &expected);
        let same = total_pixels == 0
            || fuzzy.is_some_and(|f| {
                (f.max_difference.0..=f.max_difference.1).contains(&max_difference)
                    && (f.total_pixels.0..=f.total_pixels.1).contains(&total_pixels)
            });
        if same != expect_match {
            message = Some(format!(
                "{} {reference}: {total_pixels} pixels differ, max channel difference {max_difference}",
                if expect_match { "!=" } else { "==" }
            ));
            if let Some(dir) = cx.failures {
                let stem = dir.join(&test.url);
                page::save_png(&stem.with_extension("test.png"), &actual);
                page::save_png(&stem.with_extension("ref.png"), &expected);
            }
            break;
        }
    }
    match message {
        None => json!({"status": "PASS", "passed": 1, "total": 1}),
        Some(m) => json!({"status": "FAIL", "passed": 0, "total": 1, "message": m}),
    }
}

fn reference_pixels(cx: &mut Context, reference: &str) -> Result<Vec<u8>, String> {
    if let Some(pixels) = cx.references.get(reference) {
        return Ok(pixels.clone());
    }
    let pixels = page::screenshot(cx.engine, &url(cx, reference)?)?;
    if cx.references.len() >= 32 {
        cx.references.clear();
    }
    cx.references.insert(reference.to_string(), pixels.clone());
    Ok(pixels)
}

/// (largest per-channel difference, number of differing pixels).
fn compare(a: &[u8], b: &[u8]) -> (u32, u32) {
    if a.len() != b.len() {
        return (255, (a.len().max(b.len()) / 4) as u32);
    }
    let mut max = 0u32;
    let mut count = 0u32;
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let diff = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y) as u32)
            .max()
            .unwrap_or(0);
        if diff > 0 {
            count += 1;
            max = max.max(diff);
        }
    }
    (max, count)
}
