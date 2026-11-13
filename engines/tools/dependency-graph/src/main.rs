//! `wega-dependency-graph --manifest-path <Cargo.toml> [--format text|mermaid|json] [--output FILE]`
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use wega_dependency_graph::Graph;

fn main() -> ExitCode {
    let mut manifest = None;
    let mut format = String::from("text");
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest-path" => manifest = args.next().map(PathBuf::from),
            "--format" => format = args.next().unwrap_or_default(),
            "--output" => output = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!(
                    "usage: wega-dependency-graph --manifest-path <Cargo.toml> [--format text|mermaid|json] [--output FILE]"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(manifest) = manifest else {
        eprintln!("--manifest-path is required");
        return ExitCode::from(2);
    };
    let graph = match Graph::from_manifest(&manifest) {
        Ok(graph) => graph,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let rendered = match format.as_str() {
        "text" => graph.to_text(),
        "mermaid" => graph.to_mermaid(),
        "json" => graph.to_json(),
        other => {
            eprintln!("unknown format: {other}");
            return ExitCode::from(2);
        }
    };
    match output {
        Some(path) => {
            if let Err(error) = std::fs::write(&path, rendered) {
                eprintln!("cannot write {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        }
        None => print!("{rendered}"),
    }
    ExitCode::SUCCESS
}
