# Migration Phases

**Status: PLANNED — NOT IMPLEMENTED.** Only Phase 1 is in progress or complete. Later phases are proposals and may change.

## Phase 1 — Directory structure and documentation (this change)

- Create planned directories under `engines/` with README placeholders.
- Write the module map and this document.
- No Cargo manifests, Rust source, workspace members, dependency versions, build profiles or CI are touched.

Exit criteria: all directories documented; `cargo metadata --no-deps` unchanged; existing manifests and sources unmodified.

## Phase 2 — Architecture tooling

- Implement `tools/dependency-graph` and `tools/architecture-check` against the current crates.
- Record a baseline of current dependency edges and build/memory benchmarks.

Exit criteria: baseline graph and layering report committed; checks run in report-only mode.

## Phase 3 — Foundation extraction

- Create real crates for `foundation/*` and add them to the workspace.
- Move shared types, IDs, errors, geometry and config out of `engines/wega` with re-exports to keep call sites stable.

Exit criteria: behavior unchanged; tests green.

## Phase 4 — Services and runtime boundaries

- Introduce traits for `services/*` over `engines/network` and `engines/platform`.
- Extract `runtime/*` (tab manager, process, scheduler, memory) behind these traits.

## Phase 5 — Adapter boundaries

- Define Wega-owned traits for DOM, style, layout, raster and JS runtime.
- Wrap Blitz, Stylo, Skia and V8 in `adapters/*`; stop leaking their types into other crates.

## Phase 6 — DOM, HTML and JavaScript/Web API split

- Extract `dom/*`, `html/*`, `javascript/*` and `webapi/*` with WebIDL generation in `javascript/webidl`.

## Phase 7 — CSS, layout and graphics split

- Extract `css/*`, `layout/*` and `graphics/*` behind the adapter traits, with invalidation and caching leaves.

## Phase 8 — Replace Blitz components incrementally

- Implement Wega-owned components per subsystem, switchable against the adapter, validated by `tests/compatibility` and `tests/performance`.
- Retire each Blitz-backed adapter only after parity and performance criteria are met.

## Phase 9 — Enforcement and cleanup

- Turn `architecture-check` into a blocking CI gate.
- Remove superseded code and update documentation.

## General rules

- One phase per reviewable change set; each keeps the build and tests green.
- No phase changes dependency versions or build profiles unless explicitly scoped.
