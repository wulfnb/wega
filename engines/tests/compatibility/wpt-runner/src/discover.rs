//! Test discovery, following the classification rules of WPT's manifest
//! generator (`tools/manifest/sourcefile.py`) closely enough for CSS suites:
//! a file is a reftest when it has `<link rel=match|mismatch>`, a testharness
//! test when it loads `/resources/testharness.js`, and otherwise not a test.
//! References (`*-ref*`, `*-notref*`, `reference/`) and support files are skipped.
//!
//! Reference: `blitz/wpt/runner/src/manifest.rs` (which uses the `wpt_manifest`
//! crate for the same purpose).
use serde_json::{Value, json};
use std::path::Path;

#[derive(Clone, Debug)]
pub enum Kind {
    Harness,
    Ref {
        matches: Vec<String>,
        mismatches: Vec<String>,
        fuzzy: Option<Fuzzy>,
    },
}

/// Allowed `[low, high]` ranges for the largest per-channel difference and the
/// number of differing pixels (`<meta name=fuzzy>`).
#[derive(Clone, Copy, Debug)]
pub struct Fuzzy {
    pub max_difference: (u32, u32),
    pub total_pixels: (u32, u32),
}

#[derive(Clone, Debug)]
pub struct Test {
    /// Root-relative URL path, e.g. `css/css-color/t31-color-text-a.xht`.
    pub url: String,
    pub kind: Kind,
}

const TEST_EXTENSIONS: &[&str] = &["html", "htm", "xht", "xhtml", "svg"];
const SKIPPED_DIRS: &[&str] = &[
    "support",
    "resources",
    "reference",
    "references",
    "tools",
    "crashtests",
    "work-in-progress",
];

pub fn discover(root: &Path, suites: &[String]) -> Vec<Test> {
    let mut tests = Vec::new();
    for suite in suites {
        let start = root.join(suite);
        if start.is_file() {
            tests.extend(classify(root, &start));
        } else {
            walk(root, &start, &mut tests);
        }
    }
    tests.sort_by(|a, b| a.url.cmp(&b.url));
    tests.dedup_by(|a, b| a.url == b.url);
    tests
}

fn walk(root: &Path, dir: &Path, tests: &mut Vec<Test>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !SKIPPED_DIRS.contains(&name) && !name.starts_with('.') {
                walk(root, &path, tests);
            }
        } else if let Some(test) = classify(root, &path) {
            tests.push(test);
        }
    }
}

fn classify(root: &Path, path: &Path) -> Option<Test> {
    let name = path.file_name()?.to_str()?;
    let ext = path.extension()?.to_str()?;
    if !TEST_EXTENSIONS.contains(&ext)
        || name.contains("-ref")
        || name.contains("-notref")
        || name.starts_with("ref-")
        || name.ends_with("-ref.html")
        // Need wptserve features this runner does not have.
        || name.contains(".sub.")
        || name.contains(".https.")
        || name.contains("-manual.")
        || name.contains(".window.")
        || name.contains(".any.")
        || name.contains(".worker.")
    {
        return None;
    }
    let source = std::fs::read_to_string(path).ok()?;
    let url = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    let base = url.rsplit_once('/').map_or("", |(dir, _)| dir).to_string();
    let mut matches = Vec::new();
    let mut mismatches = Vec::new();
    let mut fuzzy = None;
    for tag in tags(&source, "link") {
        let rel = attr(tag, "rel").unwrap_or_default().to_ascii_lowercase();
        let Some(href) = attr(tag, "href") else {
            continue;
        };
        let target = resolve(&base, &href);
        match rel.as_str() {
            "match" => matches.push(target),
            "mismatch" => mismatches.push(target),
            _ => {}
        }
    }
    for tag in tags(&source, "meta") {
        if attr(tag, "name").is_some_and(|n| n.eq_ignore_ascii_case("fuzzy")) {
            fuzzy = attr(tag, "content").and_then(|c| parse_fuzzy(&c));
        }
    }
    let kind = if !matches.is_empty() || !mismatches.is_empty() {
        Kind::Ref {
            matches,
            mismatches,
            fuzzy,
        }
    } else if source.contains("/resources/testharness.js") {
        Kind::Harness
    } else {
        return None;
    };
    Some(Test { url, kind })
}

/// The raw attribute text of every `<name ...>` start tag.
fn tags<'a>(source: &'a str, name: &str) -> Vec<&'a str> {
    let lower = source.to_ascii_lowercase();
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = lower[from..].find(&open).map(|i| i + from) {
        let after = start + open.len();
        let Some(end) = lower[after..].find('>').map(|i| i + after) else {
            break;
        };
        if lower[after..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_whitespace() || c == '/')
        {
            out.push(&source[after..end]);
        }
        from = end;
    }
    out
}

/// One attribute value from a start tag's attribute text (quoted or bare).
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name).map(|i| i + from) {
        let boundary = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric();
        let rest = lower[at + name.len()..].trim_start();
        if boundary && rest.starts_with('=') {
            let offset = tag.len() - rest.len() + 1;
            let value = tag[offset..].trim_start();
            return Some(match value.chars().next()? {
                q @ ('"' | '\'') => value[1..].split(q).next()?.to_string(),
                _ => value
                    .split(|c: char| c.is_ascii_whitespace() || c == '/')
                    .next()?
                    .to_string(),
            });
        }
        from = at + name.len();
    }
    None
}

fn resolve(base: &str, href: &str) -> String {
    let joined = if let Some(absolute) = href.strip_prefix('/') {
        absolute.to_string()
    } else if base.is_empty() {
        href.to_string()
    } else {
        format!("{base}/{href}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// `maxDifference=0-3;totalPixels=0-100`, the short `0-3;0-100`, optionally
/// prefixed by `ref.html:`. A single number `n` means exactly `n`.
fn parse_fuzzy(content: &str) -> Option<Fuzzy> {
    let content = content.rsplit_once(':').map_or(content, |(_, v)| v);
    let mut values = content.split(';').map(str::trim);
    let range = |part: &str| -> Option<(u32, u32)> {
        let value = part.rsplit_once('=').map_or(part, |(_, v)| v).trim();
        match value.split_once('-') {
            Some((lo, hi)) => Some((lo.trim().parse().ok()?, hi.trim().parse().ok()?)),
            None => {
                let n = value.parse().ok()?;
                Some((n, n))
            }
        }
    };
    Some(Fuzzy {
        max_difference: range(values.next()?)?,
        total_pixels: range(values.next()?)?,
    })
}

pub fn to_json(test: &Test) -> Value {
    match &test.kind {
        Kind::Harness => json!({"url": test.url, "kind": "harness"}),
        Kind::Ref {
            matches,
            mismatches,
            fuzzy,
        } => json!({
            "url": test.url,
            "kind": "ref",
            "match": matches,
            "mismatch": mismatches,
            "fuzzy": fuzzy.map(|f| [f.max_difference.0, f.max_difference.1, f.total_pixels.0, f.total_pixels.1]),
        }),
    }
}

pub fn from_json(value: &Value) -> Option<Test> {
    let url = value["url"].as_str()?.to_string();
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect()
    };
    let kind = match value["kind"].as_str()? {
        "harness" => Kind::Harness,
        _ => Kind::Ref {
            matches: strings(&value["match"]),
            mismatches: strings(&value["mismatch"]),
            fuzzy: value["fuzzy"].as_array().and_then(|f| {
                let n = |i: usize| f.get(i)?.as_u64().map(|v| v as u32);
                Some(Fuzzy {
                    max_difference: (n(0)?, n(1)?),
                    total_pixels: (n(2)?, n(3)?),
                })
            }),
        },
    };
    Some(Test { url, kind })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links_and_fuzzy() {
        let tag = tags(
            r#"<link rel="match" href="../reference/x-ref.html"><meta name=fuzzy content="maxDifference=0-3;totalPixels=0-100">"#,
            "link",
        );
        assert_eq!(attr(tag[0], "rel").as_deref(), Some("match"));
        assert_eq!(resolve("css/a/b", "../reference/x-ref.html"), "css/a/reference/x-ref.html");
        let f = parse_fuzzy("ref.html:0-3;5").unwrap();
        assert_eq!((f.max_difference, f.total_pixels), ((0, 3), (5, 5)));
    }
}
