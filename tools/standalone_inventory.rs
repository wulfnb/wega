//! Source-only inventory. This tool never launches the browser or runs tests.
#![forbid(unsafe_code)]
use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).current_dir(root).output()?;
    if !output.status.success() {
        return Err(format!("git failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

fn digest(path: &Path) -> Result<String> {
    Ok(Sha256::digest(fs::read(path)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn reference(root: &Path, path: &Path) -> Result<Value> {
    Ok(json!({
        "path": path.strip_prefix(root)?.to_string_lossy().replace('\\', "/"),
        "sha256": digest(path)?,
    }))
}

fn uncomment(text: &str) -> String {
    let pattern = Regex::new(r#"(?s)"(?:\\.|[^"\\])*"|/\*.*?\*/|//[^\n]*"#).unwrap();
    pattern
        .replace_all(text, |capture: &regex::Captures<'_>| {
            let text = &capture[0];
            if text.starts_with('"') {
                text.to_string()
            } else {
                "\n".repeat(text.matches('\n').count())
            }
        })
        .into_owned()
}

fn matches(text: &str, pattern: &str) -> Vec<String> {
    Regex::new(pattern)
        .unwrap()
        .captures_iter(text)
        .map(|capture| capture[1].to_string())
        .collect()
}

fn unique_matches(text: &str, pattern: &str) -> BTreeSet<String> {
    matches(text, pattern).into_iter().collect()
}

fn files(directory: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_file() {
            result.push(entry.path());
        } else if kind.is_dir() && recursive {
            result.extend(files(&entry.path(), true)?);
        }
    }
    result.sort();
    Ok(result)
}

fn manifest(path: &Path) -> Result<toml::Value> {
    Ok(toml::from_str(&fs::read_to_string(path)?)?)
}

fn config_source(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let path = if let Some(path) = explicit {
        path.canonicalize()?
    } else {
        let cargo = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(|path| PathBuf::from(path).join(".cargo"))
            })
            .ok_or("Cannot locate Cargo home; supply --servo-config")?;
        let mut candidates = Vec::new();
        for registry in fs::read_dir(cargo.join("registry/src"))? {
            let candidate = registry?.path().join("servo-config-0.7.0/prefs.rs");
            if candidate.is_file() {
                candidates.push(candidate);
            }
        }
        candidates.sort();
        let first = candidates
            .first()
            .ok_or("Pinned servo-config unavailable; supply --servo-config")?;
        let expected = digest(first)?;
        for path in &candidates[1..] {
            if digest(path)? != expected {
                return Err("Conflicting servo-config sources; supply --servo-config".into());
            }
        }
        first.clone()
    };
    let package = manifest(
        &path
            .parent()
            .ok_or("Invalid prefs.rs path")?
            .join("Cargo.toml"),
    )?;
    if package["package"]["name"].as_str() != Some("servo-config")
        || package["package"]["version"].as_str() != Some("0.7.0")
    {
        return Err("Expected pinned servo-config 0.7.0 prefs.rs".into());
    }
    Ok(path)
}

fn replacement_steps(name: &str) -> Vec<&'static str> {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| name.starts_with(prefix));
    let contains = |words: &[&str]| words.iter().any(|word| name.contains(word));
    if starts(&["WebGL", "WEBGL", "ANGLE", "EXT", "OES", "GPU"]) {
        vec!["19"]
    } else if starts(&[
        "Canvas",
        "OffscreenCanvas",
        "ImageBitmap",
        "ImageData",
        "Path2D",
    ]) {
        vec!["18"]
    } else if starts(&["IDB", "Cache", "Storage", "Cookie"]) {
        vec!["16"]
    } else if contains(&[
        "Worker",
        "MessagePort",
        "MessageChannel",
        "BroadcastChannel",
    ]) {
        vec!["17"]
    } else if starts(&[
        "Bluetooth",
        "Gamepad",
        "XR",
        "Permission",
        "Notification",
        "Geolocation",
        "WakeLock",
    ]) {
        vec!["21"]
    } else if starts(&[
        "RTC",
        "MediaDevice",
        "MediaStream",
        "InputDevice",
        "NavigatorUserMedia",
    ]) {
        vec!["20", "21"]
    } else if contains(&[
        "Audio",
        "Video",
        "Media",
        "Track",
        "Oscillator",
        "GainNode",
        "WaveShaper",
    ]) {
        vec!["20"]
    } else if contains(&[
        "Selection",
        "Range",
        "Clipboard",
        "DataTransfer",
        "DragEvent",
    ]) {
        vec!["12"]
    } else if starts(&["Shadow", "CustomElement", "HTMLSlot"]) {
        vec!["15"]
    } else if starts(&[
        "CSS",
        "SVG",
        "Font",
        "Animation",
        "Keyframe",
        "Scroll",
        "ViewTransition",
    ]) {
        vec!["13"]
    } else if contains(&[
        "Input",
        "Form",
        "Select",
        "Option",
        "TextArea",
        "Validity",
        "File",
        "Composition",
        "Pointer",
        "Touch",
        "Keyboard",
    ]) {
        vec!["11"]
    } else if [
        "Window",
        "WindowProxy",
        "HTMLIFrameElement",
        "BrowsingContext",
    ]
    .contains(&name)
    {
        vec!["09", "14"]
    } else {
        vec!["08"]
    }
}

fn references(root: &Path, paths: impl IntoIterator<Item = PathBuf>) -> Result<Vec<Value>> {
    paths
        .into_iter()
        .map(|path| reference(root, &path))
        .collect()
}

fn capture(root: &Path, config: &Path) -> Result<Value> {
    let cargo = manifest(&root.join("Cargo.toml"))?;
    let prefs = fs::read_to_string(config)?;
    let block = prefs
        .split_once("const fn const_default() -> Self")
        .ok_or("Missing preference defaults")?
        .1
        .split("\n    }\n")
        .next()
        .unwrap();
    let defaults: BTreeMap<String, bool> = Regex::new(r"(?m)^\s+(\w+):\s+(true|false),")?
        .captures_iter(&uncomment(block))
        .map(|capture| (capture[1].into(), &capture[2] == "true"))
        .collect();
    let mut idls = Vec::new();
    for path in files(&root.join("vendor/servo-script-bindings/webidls"), false)? {
        if path
            .extension()
            .is_none_or(|extension| extension != "webidl")
        {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let code = uncomment(&text);
        let declarations: Vec<Value> =
            Regex::new(r"\b(interface\s+mixin|interface|dictionary|enum|namespace)\s+(\w+)")?
                .captures_iter(&code)
                .map(|capture| json!({"kind": &capture[1], "name": &capture[2]}))
                .collect();
        let gates: BTreeMap<String, Value> = unique_matches(&code, r#"\bPref\s*=\s*"([^"]+)""#)
            .into_iter()
            .map(|name| {
                let value = defaults
                    .get(&name)
                    .map_or(json!("unknown"), |value| json!(value));
                (name, value)
            })
            .collect();
        let mut row = reference(root, &path)?;
        row["declarations"] = json!(declarations);
        row["preference_gates"] = json!(gates);
        row["secure_context_mentioned"] = json!(Regex::new(r"\bSecureContext\b")?.is_match(&code));
        row["exposure_annotations"] =
            json!(unique_matches(&code, r"\bExposed\s*=\s*(\([^)]*\)|\w+|\*)"));
        row["build_conditions"] = json!(unique_matches(&text, r"// skip-unless ([A-Z_]+)"));
        row["replacement_steps"] = json!(replacement_steps(
            path.file_stem()
                .unwrap()
                .to_str()
                .ok_or("Non-UTF8 IDL name")?
        ));
        row["routing_status"] = json!("provisional_family_requires_semantic_audit");
        row["runtime_exposure"] = json!("not_measured");
        idls.push(row);
    }
    let mut sources = Vec::new();
    for (directory, extension) in [
        ("src", "rs"),
        ("engines/wega/src", "rs"),
        ("engines/wega/src/dom", "js"),
    ] {
        sources.extend(
            files(&root.join(directory), false)?
                .into_iter()
                .filter(|path| path.extension().is_some_and(|ext| ext == extension)),
        );
    }
    sources.sort();
    let mut adapters = Vec::new();
    for name in [
        "src/engine.rs",
        "src/permission_adapter.rs",
        "src/navigation_adapter.rs",
    ] {
        let path = root.join(name);
        let mut row = reference(root, &path)?;
        row["methods"] = json!(matches(
            &uncomment(&fs::read_to_string(path)?),
            r"\bfn (\w+)\s*\("
        ));
        adapters.push(row);
    }
    let mut fixture_paths = files(&root.join("tests/fixtures"), true)?;
    fixture_paths.extend(files(&root.join("engines/wega/tests/fixtures"), true)?);
    fixture_paths.sort();
    let mut tests = Vec::new();
    for directory in ["tests", "engines/wega/tests", "src/harness"] {
        tests.extend(
            files(&root.join(directory), false)?
                .into_iter()
                .filter(|path| {
                    path.extension()
                        .is_some_and(|ext| ext == "rs" || ext == "py")
                }),
        );
    }
    tests.sort();
    let mut language_migration = Vec::new();
    for directory in ["src", "engines/wega/src", "scripts", "docker"] {
        for path in files(&root.join(directory), true)? {
            let language = match path.extension().and_then(|ext| ext.to_str()) {
                Some("py") => "python",
                Some("js" | "mjs") => "javascript",
                Some("sh") => "shell",
                _ => continue,
            };
            let mut row = reference(root, &path)?;
            row["language"] = json!(language);
            row["status"] = json!("requires_ownership_and_migration_audit");
            row["replacement_steps"] = json!(if directory == "engines/wega/src" {
                vec!["07", "08"]
            } else {
                vec!["23", "25"]
            });
            language_migration.push(row);
        }
    }
    let config_reference =
        json!({"path": "registry:servo-config/0.7.0/prefs.rs", "sha256": digest(config)?});
    Ok(json!({
        "schema": 1,
        "kind": "source_inventory",
        "source_commit": git(root, &["rev-parse", "HEAD"])?,
        "branch": git(root, &["branch", "--show-current"])?,
        "generator": reference(root, &root.join("tools/standalone_inventory.rs"))?,
        "source_files": references(root, sources)?,
        "manifests": references(root, ["Cargo.toml", "Cargo.lock", "engines/Cargo.toml", "engines/Cargo.lock",
            "vendor/servo/Cargo.toml", "vendor/servo-script-bindings/Cargo.toml"].map(|name| root.join(name)))?,
        "cargo_features": cargo["features"],
        "servo_dependency": cargo["dependencies"]["servo"],
        "windows_servo_dependency": cargo["target"]["cfg(windows)"]["dependencies"]["servo"],
        "preferences": {"source": config_reference, "boolean_defaults": defaults,
            "browser_overrides": {"accessibility_enabled": true, "devtools_server_enabled": "--remote-devtools only",
                "dom_notification_enabled": "test-harness builds only"}},
        "webidl_files": idls,
        "servo_adapters": adapters,
        "webdriver_command_mentions": unique_matches(&uncomment(&fs::read_to_string(root.join("src/automation.rs"))?), r"\bCommand::(\w+)"),
        "test_sources": references(root, tests)?,
        "fixtures": references(root, fixture_paths)?,
        "language_migration": language_migration,
        "runtime_evidence": [],
        "pending": [
            "Runtime captures and platform tests deferred by owner until the final test pass.",
            "IDL gates are lexical mentions; member gates, build conditions, globals and function checks need runtime evaluation.",
            "Existing binary provenance is unknown until the frozen source baseline is built and recorded.",
            "Profile migration audit is source-only; no personal profile has been read, copied or modified."
        ]
    }))
}

fn run() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut config = None;
    let mut output = root.join("docs/validation/standalone/source-inventory.json");
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--servo-config") => {
                config = Some(PathBuf::from(
                    args.next().ok_or("--servo-config requires prefs.rs")?,
                ))
            }
            Some("--output") => {
                output = PathBuf::from(args.next().ok_or("--output requires a path")?)
            }
            Some("--help" | "-h") => {
                println!(
                    "wega-inventory [--servo-config /path/to/prefs.rs] [--output inventory.json]\nSource inventory only; never launches browsers or tests."
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown argument: {}", arg.to_string_lossy()).into()),
        }
    }
    let inventory = capture(&root, &config_source(config)?)?;
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(&inventory)?;
    bytes.push(b'\n');
    fs::write(output, bytes)?;
    println!(
        "Recorded {} IDL files and {} fixtures; runtime evidence deferred.",
        inventory["webidl_files"].as_array().unwrap().len(),
        inventory["fixtures"].as_array().unwrap().len()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Cannot capture source inventory: {error}");
        std::process::exit(1);
    }
}
