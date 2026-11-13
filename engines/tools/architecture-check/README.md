# tools/architecture-check

**Status: IMPLEMENTED (Wave 1, development tool, report-only)**

## Purpose

Enforce architectural dependency rules.

## Intended responsibilities

- Check allowed/forbidden dependencies from the module map
- Fail CI on layering violations (future)

## Expected dependencies

Standard library, `cargo metadata` output, and file-system access. Specifically: `cargo metadata` output.

## Dependencies to avoid

Being depended on by production engine crates.

## Phase 2 decision

Development-only tool. `cargo_metadata` is permitted as a dependency here and in `tools/dependency-graph`; it must never be a dependency of production crates.

## Current status

IMPLEMENTED (Wave 1). Package `wega-architecture-check` is a member of the standalone tools workspace (`engines/tools/Cargo.toml`, own `Cargo.lock`), which is excluded from the `engines` workspace. See `docs/architecture/module-map.md`.

## What it does

Report-only checker for the rules in `docs/architecture/module-map.md`: dependency cycles, forbidden edges, independent contract crates, dev-tool isolation, `forbid(unsafe_code)` and third-party-engine confinement.

## Usage

```sh
cargo run --manifest-path engines/tools/Cargo.toml -p wega-architecture-check -- --manifest-path engines/Cargo.toml
```
Repeat `--manifest-path` for several workspaces; add `--output FILE`. Exit status is always 0 on findings (report-only).
