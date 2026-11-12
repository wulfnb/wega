//! Dependency boundary: this crate may depend only on what is listed here. Adding any other
//! dependency (above all `wega-engine`, another adapter or a second document engine) fails
//! this test, so the boundary is enforced by `cargo test` and not only by the report-only
//! architecture checker. Extend the lists deliberately, with the architecture docs.
const ALLOWED: &[&str] = &[
    "wega-dom",
    "blitz-dom",
    "blitz-html",
    "blitz-traits",
    "html5ever",
    "url",
];
const ALLOWED_DEV: &[&str] = &["wega-html"];

/// Dependency names declared in the given Cargo.toml tables.
fn dependencies(manifest: &str, tables: &[&str]) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = String::new();
    for line in manifest.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = header.to_owned();
        } else if tables.contains(&current.as_str())
            && !line.is_empty()
            && !line.starts_with('#')
            && let Some((name, _)) = line.split_once('=')
        {
            names.push(name.trim().to_owned());
        }
    }
    names
}

#[test]
fn dependencies_stay_inside_the_boundary() {
    let manifest = include_str!("../Cargo.toml");
    for name in dependencies(manifest, &["dependencies", "build-dependencies"]) {
        assert!(
            ALLOWED.contains(&name.as_str()),
            "unexpected dependency `{name}`"
        );
    }
    for name in dependencies(manifest, &["dev-dependencies"]) {
        assert!(
            ALLOWED.contains(&name.as_str()) || ALLOWED_DEV.contains(&name.as_str()),
            "unexpected dev-dependency `{name}`"
        );
    }
    assert!(
        !manifest.contains("[target."),
        "target-specific dependencies are not allowed"
    );
}

#[test]
fn sources_do_not_name_the_engine_or_other_document_engines() {
    let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            stack.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
        } else if path.extension().is_some_and(|e| e == "rs") {
            let source = std::fs::read_to_string(&path).unwrap();
            for (n, line) in source.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                assert!(
                    !code.contains("wega_engine"),
                    "{}:{} names the engine",
                    path.display(),
                    n + 1
                );
                assert!(
                    !code.contains("wega_html"),
                    "{}:{} the adapter does not use the parser",
                    path.display(),
                    n + 1
                );
            }
        }
    }
}
