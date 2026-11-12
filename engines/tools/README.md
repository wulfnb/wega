# engines/tools — Tools

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`dependency-graph`](./dependency-graph/README.md) — Generate and visualize the crate dependency graph.
- [`build-benchmark`](./build-benchmark/README.md) — Measure and track build times.
- [`memory-benchmark`](./memory-benchmark/README.md) — Measure and track memory use.
- [`architecture-check`](./architecture-check/README.md) — Enforce architectural dependency rules.

Default dependencies for this group: Standard library, `cargo metadata` output, and file-system access.

Dependencies to avoid: Being depended on by production engine crates.
