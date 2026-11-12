# Migration Phases

**Status: Phase 1 and Phase 2 Wave 1 are complete.** Phase 1: commit `d168b71`. Wave 1: see [`wave1-implementation-report.md`](./wave1-implementation-report.md). Plan: [`crate-initialization-plan.md`](./crate-initialization-plan.md). Wave 2 gates were evaluated in [`wave2-gate-report.md`](./wave2-gate-report.md) (only `wega-adapter-skia` approved, limited to raster); Wave 2 implementation and Wave 3 have not started.

## Phase 1 — Directory structure and documentation (complete)

Placeholder directories and READMEs under `engines/`, module map, migration phases. No manifests, source or CI changes.

## Phase 2 — Crate boundaries and extractions (planned, in three waves)

Rules for every step: one reviewable commit; new crates join the `engines` workspace only; compatibility re-exports at old paths; `#![forbid(unsafe_code)]` by default; no dependency version changes; CI-equivalent tests green; revert-by-commit rollback.

### Wave 1 — tooling and clean service seams (COMPLETE)
1. Architecture tooling (`wega-dependency-graph`, `wega-architecture-check`; report-only; `cargo_metadata` allowed).
2. `wega-cookies` (from `network/src/cookies.rs`).
3. `wega-storage` (Cache Storage, Cache API, IndexedDB from `wega-network`).
4. `wega-security` (CSP and pure secure-context logic).
5. `wega-permissions` (from `engines/wega/src/permissions`).

No gate beyond tests; seams were verified in the plan.

### Wave 2 — runtime, adapters, DOM/HTML contracts, bindings, workers
Order: `wega-js-runtime` → `wega-adapter-v8` → `wega-adapter-skia` → `wega-dom` → `wega-html` → `wega-js-bindings` → `wega-workers`.

**Entry requires architecture gates G2-JS, G2-SKIA, G2-DOM and G2-ARCH** (inventory of real usage, no speculative abstractions, clean architecture-check report). A failed gate keeps the module documentation-only.

### Wave 3 — style, layout, display list, Blitz/Stylo adapters (conditional)
`wega-css`, `wega-layout`, `wega-display-list` are created only if gates G3-CSS, G3-LAYOUT, G3-DL pass (independent of each other; integration through adapters). Then `wega-adapter-stylo` and `wega-adapter-blitz`, validated by parity gate G3-PARITY and G3-ARCH.

## Phase 3 — Foundation extraction (deferred)

Create `foundation/*` crates only when a stable shared interface has multiple real consumers (decision 4). May be empty.

## Phase 4 — Services and runtime boundaries (as needed)

Narrow traits for navigation, history, downloads, bookmarks, tab management once Servo-coupled root code is retired or abstracted. `runtime/process` stays documentation-only; `wega-platform`/`wega-protocol` remain the implementation.

## Phase 5 — Replace Blitz components incrementally

Implement Wega-owned components per subsystem behind the adapter boundaries; retire a Blitz-backed path only after parity and performance criteria are met (`tests/compatibility`, `tests/performance`).

## Phase 6 — Enforcement and cleanup

Make `architecture-check` blocking in CI; remove compatibility re-export shims and superseded duplicates (e.g. an unused second `public_suffix_list.dat`); update documentation.

## General rules

- No phase changes dependency versions or build profiles unless explicitly scoped.
- `engines/vendor/*` is never a migration source or destination.
