# tools/dependency-graph

**Status: IMPLEMENTED (Wave 1, development tool, report-only)**

## Purpose

Generate and visualize the crate dependency graph.

## Intended responsibilities

- Parse `cargo metadata` and emit graphs
- Diff graphs between revisions

## Expected dependencies

Standard library, `cargo metadata` output, and file-system access.

## Dependencies to avoid

Being depended on by production engine crates.

## Phase 2 decision

Development-only tool; `cargo_metadata` permitted. Never a dependency of production crates.

## Current status

IMPLEMENTED (Wave 1). Package `wega-dependency-graph` is a member of the standalone tools workspace (`engines/tools/Cargo.toml`, own `Cargo.lock`), which is excluded from the `engines` workspace. See `docs/architecture/module-map.md`.

## What it does

Prints the workspace-internal dependency graph (text, Mermaid or JSON) from `cargo metadata --no-deps`, plus cross-workspace path dependencies and direct third-party engine dependencies.

## Usage

```sh
cargo run --manifest-path engines/tools/Cargo.toml -p wega-dependency-graph -- --manifest-path engines/Cargo.toml
```
Add `--format mermaid|json` or `--output FILE`.
