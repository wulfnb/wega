# docs/architecture

**Status: PARTIALLY IMPLEMENTED** — Phase 1 structure and Phase 2 Wave 1 are complete; Wave 2 gate decisions are in [`wave2-gate-report.md`](./wave2-gate-report.md); Wave 3 is not started

## Purpose

Holds architecture documentation for the planned modular engine layout.

## Intended responsibilities

- [`module-map.md`](./module-map.md): full planned structure.
- [`migration-phases.md`](./migration-phases.md): phased migration plan.
- [`crate-initialization-plan.md`](./crate-initialization-plan.md): crate boundaries and execution plan.
- [`wave1-implementation-report.md`](./wave1-implementation-report.md): Wave 1 results.
- [`wave2-gate-report.md`](./wave2-gate-report.md): Wave 2 architecture gate decisions (no implementation).
- [`architecture-refactor-progress.md`](./architecture-refactor-progress.md): architecture-first refactoring progress (ownership model, V8/network/worker decoupling, `wega-adapter-skia`).
- [`baseline/`](./baseline/): baseline and post-wave dependency graphs and architecture reports.

## Expected dependencies

None (documentation only).

## Dependencies to avoid

None.

## Current status

See the status line above. These documents are the authoritative architecture record; individual module READMEs under `engines/` carry their own status.
