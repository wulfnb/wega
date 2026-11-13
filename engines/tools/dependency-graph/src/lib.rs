//! Workspace dependency graph extracted from `cargo metadata --no-deps`.
//!
//! Development tool only: never a dependency of production crates.
#![forbid(unsafe_code)]

use cargo_metadata::{DependencyKind, MetadataCommand};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Third-party engines that the target architecture confines to `wega-adapter-*` crates.
pub const THIRD_PARTY_ENGINES: &[&str] = &[
    "blitz-dom",
    "blitz-html",
    "blitz-paint",
    "blitz-traits",
    "stylo",
    "skia-safe",
    "skia-bindings",
    "anyrender",
    "anyrender_skia",
    "deno_core",
    "deno_ops",
    "deno_v8",
    "serde_v8",
    "v8",
    "html5ever",
    "servo",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Normal,
    Build,
    Dev,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Normal => "normal",
            Kind::Build => "build",
            Kind::Dev => "dev",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dependency {
    pub name: String,
    pub kind: Kind,
    pub optional: bool,
    pub req: String,
    /// Declared with a `path = ...` source.
    pub path: bool,
}

#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
    pub dependencies: Vec<Dependency>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: Kind,
    pub optional: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub manifest: PathBuf,
    pub packages: BTreeMap<String, Package>,
}

impl Graph {
    /// Reads workspace members and their declared dependencies. Does not resolve
    /// or download anything (`--no-deps`).
    pub fn from_manifest(manifest: &Path) -> Result<Self, String> {
        let metadata = MetadataCommand::new()
            .manifest_path(manifest)
            .no_deps()
            .exec()
            .map_err(|error| {
                format!("cargo metadata failed for {}: {error}", manifest.display())
            })?;
        let mut packages = BTreeMap::new();
        for package in metadata.packages {
            let name = package.name.to_string();
            let dir = package
                .manifest_path
                .parent()
                .map(|path| PathBuf::from(path.as_str()))
                .unwrap_or_default();
            let dependencies = package
                .dependencies
                .iter()
                .map(|dependency| Dependency {
                    name: dependency.name.clone(),
                    kind: match dependency.kind {
                        DependencyKind::Development => Kind::Dev,
                        DependencyKind::Build => Kind::Build,
                        _ => Kind::Normal,
                    },
                    optional: dependency.optional,
                    req: dependency.req.to_string(),
                    path: dependency.path.is_some(),
                })
                .collect();
            packages.insert(
                name.clone(),
                Package {
                    name,
                    version: package.version.to_string(),
                    dir,
                    dependencies,
                },
            );
        }
        Ok(Self {
            manifest: manifest.to_path_buf(),
            packages,
        })
    }

    /// Edges between workspace members only, sorted and de-duplicated.
    pub fn internal_edges(&self) -> Vec<Edge> {
        let mut edges = BTreeSet::new();
        for package in self.packages.values() {
            for dependency in &package.dependencies {
                if self.packages.contains_key(&dependency.name) && dependency.name != package.name {
                    edges.insert(Edge {
                        from: package.name.clone(),
                        to: dependency.name.clone(),
                        kind: dependency.kind,
                        optional: dependency.optional,
                    });
                }
            }
        }
        edges.into_iter().collect()
    }

    /// Strongly connected groups over normal and build edges (dev-dependency
    /// cycles are legal in Cargo and are not reported). Self-loops are included.
    pub fn cycles(&self) -> Vec<Vec<String>> {
        let mut adjacency: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let edges = self.internal_edges();
        for name in self.packages.keys() {
            adjacency.entry(name.as_str()).or_default();
        }
        for edge in &edges {
            if edge.kind != Kind::Dev {
                adjacency
                    .entry(edge.from.as_str())
                    .or_default()
                    .push(edge.to.as_str());
            }
        }
        struct State<'a> {
            index: usize,
            indices: BTreeMap<&'a str, usize>,
            low: BTreeMap<&'a str, usize>,
            stack: Vec<&'a str>,
            on_stack: BTreeSet<&'a str>,
            found: Vec<Vec<String>>,
        }
        fn visit<'a>(
            node: &'a str,
            graph: &BTreeMap<&'a str, Vec<&'a str>>,
            state: &mut State<'a>,
        ) {
            state.indices.insert(node, state.index);
            state.low.insert(node, state.index);
            state.index += 1;
            state.stack.push(node);
            state.on_stack.insert(node);
            for &next in &graph[node] {
                if !state.indices.contains_key(next) {
                    visit(next, graph, state);
                    let low = state.low[node].min(state.low[next]);
                    state.low.insert(node, low);
                } else if state.on_stack.contains(next) {
                    let low = state.low[node].min(state.indices[next]);
                    state.low.insert(node, low);
                }
            }
            if state.low[node] == state.indices[node] {
                let mut component = Vec::new();
                while let Some(top) = state.stack.pop() {
                    state.on_stack.remove(top);
                    component.push(top.to_string());
                    if top == node {
                        break;
                    }
                }
                let self_loop = component.len() == 1 && graph[node].contains(&node);
                if component.len() > 1 || self_loop {
                    component.sort();
                    state.found.push(component);
                }
            }
        }
        let mut state = State {
            index: 0,
            indices: BTreeMap::new(),
            low: BTreeMap::new(),
            stack: Vec::new(),
            on_stack: BTreeSet::new(),
            found: Vec::new(),
        };
        let nodes: Vec<&str> = adjacency.keys().copied().collect();
        for node in nodes {
            if !state.indices.contains_key(node) {
                visit(node, &adjacency, &mut state);
            }
        }
        state.found.sort();
        state.found
    }

    /// Path dependencies on packages that are **not** members of this workspace
    /// (for example the root application depending on `engines/*` crates).
    pub fn cross_workspace_path_dependencies(&self) -> Vec<(String, String, Kind)> {
        let mut out = BTreeSet::new();
        for package in self.packages.values() {
            for dependency in &package.dependencies {
                if dependency.path && !self.packages.contains_key(&dependency.name) {
                    out.insert((
                        package.name.clone(),
                        dependency.name.clone(),
                        dependency.kind,
                    ));
                }
            }
        }
        out.into_iter().collect()
    }

    /// `(package, third-party engine dependency, kind)` for every direct dependency
    /// on a crate listed in [`THIRD_PARTY_ENGINES`].
    pub fn third_party_engine_dependencies(&self) -> Vec<(String, String, Kind)> {
        let mut out = BTreeSet::new();
        for package in self.packages.values() {
            for dependency in &package.dependencies {
                if THIRD_PARTY_ENGINES.contains(&dependency.name.as_str()) {
                    out.insert((
                        package.name.clone(),
                        dependency.name.clone(),
                        dependency.kind,
                    ));
                }
            }
        }
        out.into_iter().collect()
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "manifest: {}", self.manifest.display());
        let edges = self.internal_edges();
        for package in self.packages.values() {
            let _ = writeln!(out, "{} {}", package.name, package.version);
            let mut any = false;
            for edge in edges.iter().filter(|edge| edge.from == package.name) {
                any = true;
                let _ = writeln!(
                    out,
                    "  -> {} ({}{})",
                    edge.to,
                    edge.kind.as_str(),
                    if edge.optional { ", optional" } else { "" }
                );
            }
            if !any {
                let _ = writeln!(out, "  (no workspace dependencies)");
            }
        }
        for (from, to, kind) in self.cross_workspace_path_dependencies() {
            let _ = writeln!(
                out,
                "path dependency outside this workspace: {from} -> {to} ({})",
                kind.as_str()
            );
        }
        out
    }

    pub fn to_mermaid(&self) -> String {
        let id = |name: &str| name.replace(['-', '.'], "_");
        let mut out = String::from("graph TD\n");
        for package in self.packages.values() {
            let _ = writeln!(out, "  {}[\"{}\"]", id(&package.name), package.name);
        }
        for edge in self.internal_edges() {
            let label = match (edge.kind, edge.optional) {
                (Kind::Normal, false) => String::new(),
                (kind, optional) => format!(
                    "|{}{}|",
                    kind.as_str(),
                    if optional { ", optional" } else { "" }
                ),
            };
            let arrow = if edge.kind == Kind::Normal {
                "-->"
            } else {
                "-.->"
            };
            let _ = writeln!(out, "  {} {arrow}{label} {}", id(&edge.from), id(&edge.to));
        }
        out
    }

    pub fn to_json(&self) -> String {
        fn esc(value: &str) -> String {
            let mut out = String::new();
            for ch in value.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    c if (c as u32) < 0x20 => {
                        let _ = write!(out, "\\u{:04x}", c as u32);
                    }
                    c => out.push(c),
                }
            }
            out
        }
        let packages: Vec<String> = self
            .packages
            .values()
            .map(|p| {
                format!(
                    "{{\"name\":\"{}\",\"version\":\"{}\"}}",
                    esc(&p.name),
                    esc(&p.version)
                )
            })
            .collect();
        let edges: Vec<String> = self
            .internal_edges()
            .iter()
            .map(|e| {
                format!(
                    "{{\"from\":\"{}\",\"to\":\"{}\",\"kind\":\"{}\",\"optional\":{}}}",
                    esc(&e.from),
                    esc(&e.to),
                    e.kind.as_str(),
                    e.optional
                )
            })
            .collect();
        let engines: Vec<String> = self
            .third_party_engine_dependencies()
            .iter()
            .map(|(p, d, k)| {
                format!(
                    "{{\"package\":\"{}\",\"dependency\":\"{}\",\"kind\":\"{}\"}}",
                    esc(p),
                    esc(d),
                    k.as_str()
                )
            })
            .collect();
        format!(
            "{{\"manifest\":\"{}\",\"packages\":[{}],\"edges\":[{}],\"third_party_engines\":[{}]}}\n",
            esc(&self.manifest.display().to_string()),
            packages.join(","),
            edges.join(","),
            engines.join(",")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, deps: &[(&str, Kind)]) -> Package {
        Package {
            name: name.into(),
            version: "0.1.0".into(),
            dir: PathBuf::from(format!("/w/{name}")),
            dependencies: deps
                .iter()
                .map(|(n, k)| Dependency {
                    name: (*n).into(),
                    kind: *k,
                    optional: false,
                    req: "*".into(),
                    path: false,
                })
                .collect(),
        }
    }

    pub(crate) fn graph(packages: Vec<Package>) -> Graph {
        Graph {
            manifest: PathBuf::from("/w/Cargo.toml"),
            packages: packages.into_iter().map(|p| (p.name.clone(), p)).collect(),
        }
    }

    #[test]
    fn external_dependencies_are_not_edges() {
        let g = graph(vec![
            package("a", &[("serde", Kind::Normal), ("b", Kind::Normal)]),
            package("b", &[]),
        ]);
        let edges = g.internal_edges();
        assert_eq!(edges.len(), 1);
        assert_eq!((edges[0].from.as_str(), edges[0].to.as_str()), ("a", "b"));
    }

    #[test]
    fn acyclic_graph_has_no_cycles() {
        let g = graph(vec![
            package("a", &[("b", Kind::Normal), ("c", Kind::Normal)]),
            package("b", &[("c", Kind::Normal)]),
            package("c", &[]),
        ]);
        assert!(g.cycles().is_empty());
    }

    #[test]
    fn normal_cycle_is_reported() {
        let g = graph(vec![
            package("a", &[("b", Kind::Normal)]),
            package("b", &[("c", Kind::Normal)]),
            package("c", &[("a", Kind::Build)]),
            package("d", &[]),
        ]);
        assert_eq!(
            g.cycles(),
            vec![vec!["a".to_string(), "b".into(), "c".into()]]
        );
    }

    #[test]
    fn dev_dependency_cycles_are_legal() {
        let g = graph(vec![
            package("a", &[("b", Kind::Normal)]),
            package("b", &[("a", Kind::Dev)]),
        ]);
        assert!(g.cycles().is_empty());
    }

    #[test]
    fn mermaid_and_json_mention_edges() {
        let g = graph(vec![
            package("a-x", &[("b", Kind::Normal)]),
            package("b", &[]),
        ]);
        assert!(g.to_mermaid().contains("a_x --> b"));
        assert!(g.to_json().contains("\"from\":\"a-x\""));
    }

    #[test]
    fn engine_dependencies_are_listed() {
        let g = graph(vec![package(
            "a",
            &[("deno_core", Kind::Normal), ("serde", Kind::Normal)],
        )]);
        assert_eq!(g.third_party_engine_dependencies().len(), 1);
    }
}
