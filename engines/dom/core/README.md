# dom/core — `wega-dom`

**Status: IMPLEMENTED (Round 7)** — the engine-neutral document contract.

## What it is

`NodeKey` (stable node identity), `NodeView`/`DocumentView` (borrowed, `Copy` read-only views with static dispatch), `DocumentMutation`/`Mutator` (mutation scopes: immediate application, validate-then-apply, deferred finalization on `finish`/drop, stale keys never alias) and the helper functions built on them. It is **not** a DOM tree: there is no node storage here, so there is no second document. A document engine supplies the storage through an adapter.

## Dependencies

`html5ever` (for `QualName`) and `url`. Nothing else: no `wega-engine`, no Blitz, no adapter. `#![forbid(unsafe_code)]`. `tests/boundary.rs` fails the build if a dependency outside this list appears or if a source file names Blitz or the engine.

## Not here

Node storage, events (`dom/events`), mutation observers (`dom/mutation`), traversal/query (`dom/traversal`), form state, layout. Those leaves remain planned. See `docs/architecture/dom-round7-report.md`.
