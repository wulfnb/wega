# engines/tools — Tools

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. `dependency-graph` and `architecture-check` are implemented as members of the standalone tools workspace (`engines/tools/Cargo.toml`, own `Cargo.lock`, excluded from the `engines` workspace); the benchmark leaves are still planned.

Leaf modules:

- [`dependency-graph`](./dependency-graph/README.md) — Generate and visualize the crate dependency graph.
- [`build-benchmark`](./build-benchmark/README.md) — Measure and track build times.
- [`memory-benchmark`](./memory-benchmark/README.md) — Measure and track memory use.
- [`architecture-check`](./architecture-check/README.md) — Enforce architectural dependency rules.

Default dependencies for this group: Standard library, `cargo metadata` output, and file-system access.

Dependencies to avoid: Being depended on by production engine crates.
