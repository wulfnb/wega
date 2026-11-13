//! `wega-architecture-check --manifest-path <Cargo.toml>... [--output FILE]`
//!
//! REPORT-ONLY: findings are printed as a Markdown report and the process exits
//! with status 0 regardless of findings. A non-zero status means the tool itself
//! failed (bad arguments, `cargo metadata` failure, unwritable output).
#![forbid(unsafe_code)]

mod rules;

use std::path::PathBuf;
use std::process::ExitCode;
use wega_dependency_graph::Graph;

fn main() -> ExitCode {
    let mut manifests: Vec<PathBuf> = Vec::new();
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest-path" => match args.next() {
                Some(value) => manifests.push(PathBuf::from(value)),
                None => {
                    eprintln!("--manifest-path needs a value");
                    return ExitCode::from(2);
                }
            },
            "--output" => output = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!(
                    "usage: wega-architecture-check --manifest-path <Cargo.toml>... [--output FILE]  (report-only, exit 0 on findings)"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::from(2);
            }
        }
    }
    if manifests.is_empty() {
        eprintln!("at least one --manifest-path is required");
        return ExitCode::from(2);
    }
    let mut graphs = Vec::new();
    for manifest in &manifests {
        match Graph::from_manifest(manifest) {
            Ok(graph) => graphs.push(graph),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let report = rules::render_report(&graphs);
    match output {
        Some(path) => {
            if let Err(error) = std::fs::write(&path, report) {
                eprintln!("cannot write {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        }
        None => print!("{report}"),
    }
    ExitCode::SUCCESS
}
