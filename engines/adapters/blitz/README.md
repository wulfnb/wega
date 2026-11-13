# adapters/blitz — `wega-adapter-blitz`

**Status: PARTIALLY IMPLEMENTED (Round 7)** — the Blitz document behind the `wega-dom` contract.

Owns `BlitzDocument` (an owning, zero-cost wrapper of Blitz's `BaseDocument` that derefs to it, so layout/paint/style/form APIs that are not behind the contract yet keep working), `BlitzNode` (borrowed node view), `BlitzMutator` (Blitz's `DocumentMutator` behind `Mutator`) and the `NodeId`<->`NodeKey` conversions (`ToKey`/`ToNodeId`).

Not yet here: layout, style, paint, events, form state (the engine still uses Blitz directly for these; see `docs/architecture/dom-round7-report.md`).

## Dependencies

`wega-dom`, `blitz-dom`, `html5ever`, `url`; dev: `wega-html`, `blitz-html` (parser parity tests). No dependency on `wega-engine`; `tests/boundary.rs` enforces it. `#![forbid(unsafe_code)]`.
