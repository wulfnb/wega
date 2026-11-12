//! Architecture rules from `docs/architecture/module-map.md` (report-only).

use std::fmt::Write as _;
use std::path::Path;
use wega_dependency_graph::{Graph, Kind};

/// Crates that predate the modular layout and are exempt from the
/// `forbid(unsafe_code)` expectation (reported as informational only).
const LEGACY_PACKAGES: &[&str] = &[
    "wega-engine",
    "wega-network",
    "wega-platform",
    "wega-protocol",
    "wega",
];

/// New crates with a documented, reviewed `unsafe` exception (denied crate-wide, allowed on
/// one guard type with written safety invariants).
const DOCUMENTED_UNSAFE: &[&str] = &["wega-adapter-v8"];

/// Independent contract crates: none may depend on another.
const CONTRACT_CRATES: &[&str] = &[
    "wega-dom",
    "wega-html",
    "wega-css",
    "wega-layout",
    "wega-display-list",
];

/// Development tools that production crates must never depend on.
const DEV_TOOLS: &[&str] = &["wega-dependency-graph", "wega-architecture-check"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// Violates a rule the repository is expected to satisfy today.
    Violation,
    /// Describes current state that differs from the future target.
    Informational,
}

#[derive(Debug)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
}

pub fn glob(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pattern == name,
    }
}

/// `(from, to, except_from, reason)`; `*` suffix is a prefix wildcard.
type ForbiddenEdge = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
);

const FORBIDDEN: &[ForbiddenEdge] = &[
    (
        "wega-protocol",
        "wega-*",
        &[],
        "wire contract must not depend on other Wega crates",
    ),
    (
        "wega-platform",
        "wega-*",
        &[],
        "OS layer must not depend on other Wega crates",
    ),
    (
        "wega-network",
        "wega-engine",
        &[],
        "services must not depend on the engine",
    ),
    (
        "wega-network",
        "wega-adapter-*",
        &[],
        "services must not depend on adapters",
    ),
    (
        "wega-cookies",
        "wega-network",
        &[],
        "cookies sit below the HTTP crate",
    ),
    (
        "wega-cookies",
        "wega-engine",
        &[],
        "services must not depend on the engine",
    ),
    (
        "wega-storage",
        "wega-network",
        &[],
        "storage must not depend on HTTP",
    ),
    (
        "wega-storage",
        "wega-engine",
        &[],
        "services must not depend on the engine",
    ),
    (
        "wega-security",
        "wega-network",
        &[],
        "security policy sits below the HTTP crate",
    ),
    (
        "wega-security",
        "wega-engine",
        &[],
        "services must not depend on the engine",
    ),
    (
        "wega-permissions",
        "wega-engine",
        &[],
        "services must not depend on the engine",
    ),
    (
        "wega-permissions",
        "wega-adapter-*",
        &[],
        "services must not depend on adapters",
    ),
    (
        "*",
        "wega-adapter-*",
        &["wega-engine"],
        "only the composition root may depend on adapters",
    ),
];

fn matches(pattern: &str, name: &str) -> bool {
    pattern == "*" || glob(pattern, name)
}

pub fn check_graph(graph: &Graph) -> Vec<Finding> {
    let mut findings = Vec::new();
    let edges = graph.internal_edges();

    for cycle in graph.cycles() {
        findings.push(Finding {
            rule: "ARCH-001 no dependency cycles",
            severity: Severity::Violation,
            message: format!("cycle among: {}", cycle.join(", ")),
        });
    }

    for edge in &edges {
        if edge.kind == Kind::Dev {
            continue;
        }
        for (from, to, except, reason) in FORBIDDEN {
            if matches(from, &edge.from)
                && matches(to, &edge.to)
                && !except.contains(&edge.from.as_str())
                && edge.from != edge.to
            {
                findings.push(Finding {
                    rule: "ARCH-002 forbidden dependency edge",
                    severity: Severity::Violation,
                    message: format!("{} -> {} ({reason})", edge.from, edge.to),
                });
            }
        }
        if CONTRACT_CRATES.contains(&edge.from.as_str())
            && CONTRACT_CRATES.contains(&edge.to.as_str())
        {
            findings.push(Finding {
                rule: "ARCH-003 independent contract crates",
                severity: Severity::Violation,
                message: format!(
                    "{} -> {} (contract crates must stay independent)",
                    edge.from, edge.to
                ),
            });
        }
        if DEV_TOOLS.contains(&edge.to.as_str()) {
            findings.push(Finding {
                rule: "ARCH-004 dev tools are not production dependencies",
                severity: Severity::Violation,
                message: format!("{} -> {}", edge.from, edge.to),
            });
        }
    }

    let is_tools_workspace = graph
        .packages
        .values()
        .all(|package| path_has_component(&package.dir, "tools"));
    for package in graph.packages.values() {
        if !is_tools_workspace
            && path_has_component(&package.dir, "tools")
            && DEV_TOOLS.contains(&package.name.as_str())
        {
            findings.push(Finding {
                rule: "ARCH-005 tools workspace separation",
                severity: Severity::Violation,
                message: format!(
                    "{} is a member of {} (dev tools belong to the standalone tools workspace)",
                    package.name,
                    graph.manifest.display()
                ),
            });
        }
        // forbid(unsafe_code) expectation for crates written under the modular layout.
        let forbids = ["src/lib.rs", "src/main.rs"].iter().any(|entry| {
            std::fs::read_to_string(package.dir.join(entry))
                .map(|text| text.contains("#![forbid(unsafe_code)]"))
                .unwrap_or(false)
        });
        if !forbids {
            let documented = DOCUMENTED_UNSAFE.contains(&package.name.as_str());
            let legacy = documented || LEGACY_PACKAGES.contains(&package.name.as_str());
            findings.push(Finding {
                rule: "ARCH-006 forbid(unsafe_code) by default",
                severity: if legacy {
                    Severity::Informational
                } else {
                    Severity::Violation
                },
                message: format!(
                    "{} does not declare #![forbid(unsafe_code)]{}",
                    package.name,
                    if documented {
                        " (documented unsafe exception)"
                    } else if legacy {
                        " (legacy crate; documented exception/not yet reviewed)"
                    } else {
                        ""
                    }
                ),
            });
        }
    }

    for (package, dependency, kind) in graph.third_party_engine_dependencies() {
        if kind == Kind::Dev || glob("wega-adapter-*", &package) {
            continue;
        }
        findings.push(Finding {
            rule: "ARCH-007 third-party engines only in adapters (future target)",
            severity: Severity::Informational,
            message: format!(
                "{package} depends directly on `{dependency}` ({})",
                kind.as_str()
            ),
        });
    }
    findings
}

fn path_has_component(path: &Path, component: &str) -> bool {
    path.components().any(|c| c.as_os_str() == component)
}

pub fn render_report(graphs: &[Graph]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Architecture check report\n");
    let _ = writeln!(
        out,
        "**Mode: REPORT-ONLY.** Exit status is 0 regardless of findings.\n"
    );
    for graph in graphs {
        let findings = check_graph(graph);
        let violations = findings
            .iter()
            .filter(|f| f.severity == Severity::Violation)
            .count();
        let info = findings.len() - violations;
        let _ = writeln!(out, "## Workspace `{}`\n", graph.manifest.display());
        let _ = writeln!(out, "- Members: {}", graph.packages.len());
        let _ = writeln!(out, "- Internal edges: {}", graph.internal_edges().len());
        let cross = graph.cross_workspace_path_dependencies();
        let _ = writeln!(
            out,
            "- Path dependencies outside this workspace: {}",
            cross.len()
        );
        let _ = writeln!(out, "- Violations: {violations}");
        let _ = writeln!(out, "- Informational: {info}\n");
        if !cross.is_empty() {
            let _ = writeln!(out, "### Path dependencies outside this workspace\n");
            for (from, to, kind) in &cross {
                let _ = writeln!(out, "- {from} -> {to} ({})", kind.as_str());
            }
            out.push('\n');
        }
        for (title, severity) in [
            ("Violations", Severity::Violation),
            ("Informational", Severity::Informational),
        ] {
            let _ = writeln!(out, "### {title}\n");
            let selected: Vec<_> = findings.iter().filter(|f| f.severity == severity).collect();
            if selected.is_empty() {
                let _ = writeln!(out, "None.\n");
            } else {
                for finding in selected {
                    let _ = writeln!(out, "- `{}`: {}", finding.rule, finding.message);
                }
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use wega_dependency_graph::{Dependency, Package};

    fn graph(spec: &[(&str, &[&str])]) -> Graph {
        let mut packages = BTreeMap::new();
        for (name, deps) in spec {
            packages.insert(
                (*name).to_string(),
                Package {
                    name: (*name).into(),
                    version: "0.1.0".into(),
                    dir: PathBuf::from(format!("/nonexistent/{name}")),
                    dependencies: deps
                        .iter()
                        .map(|d| Dependency {
                            name: (*d).into(),
                            kind: Kind::Normal,
                            optional: false,
                            req: "*".into(),
                            path: false,
                        })
                        .collect(),
                },
            );
        }
        Graph {
            manifest: PathBuf::from("/nonexistent/Cargo.toml"),
            packages,
        }
    }

    fn violations(graph: &Graph) -> Vec<String> {
        check_graph(graph)
            .into_iter()
            .filter(|f| f.severity == Severity::Violation && !f.rule.starts_with("ARCH-006"))
            .map(|f| f.message)
            .collect()
    }

    #[test]
    fn current_layout_is_clean() {
        let g = graph(&[
            ("wega-protocol", &[]),
            ("wega-platform", &[]),
            ("wega-network", &["wega-platform", "wega-protocol"]),
            (
                "wega-engine",
                &["wega-network", "wega-platform", "wega-protocol"],
            ),
        ]);
        assert!(violations(&g).is_empty());
    }

    #[test]
    fn storage_depending_on_network_is_flagged() {
        let g = graph(&[("wega-storage", &["wega-network"]), ("wega-network", &[])]);
        assert_eq!(violations(&g).len(), 1);
    }

    #[test]
    fn adapter_dependency_outside_engine_is_flagged() {
        let g = graph(&[
            ("wega-network", &["wega-adapter-v8"]),
            ("wega-adapter-v8", &[]),
        ]);
        // flagged by both the services rule and the composition-root rule
        assert_eq!(violations(&g).len(), 2);
        let ok = graph(&[
            ("wega-engine", &["wega-adapter-v8"]),
            ("wega-adapter-v8", &[]),
        ]);
        assert!(violations(&ok).is_empty());
    }

    #[test]
    fn contract_crates_must_be_independent() {
        let g = graph(&[("wega-css", &["wega-dom"]), ("wega-dom", &[])]);
        assert_eq!(violations(&g).len(), 1);
    }

    #[test]
    fn protocol_must_stay_a_leaf() {
        let g = graph(&[("wega-protocol", &["wega-cookies"]), ("wega-cookies", &[])]);
        assert_eq!(violations(&g).len(), 1);
    }

    #[test]
    fn glob_matching() {
        assert!(glob("wega-adapter-*", "wega-adapter-v8"));
        assert!(!glob("wega-adapter-*", "wega-engine"));
        assert!(glob("wega-engine", "wega-engine"));
    }
}
