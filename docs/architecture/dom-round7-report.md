# Round 7 report: DOM/HTML crate extraction and Blitz adapter ownership

Scope: resolve the Blitz-adapter orphan-rule problem, decouple parser diagnostics, and extract `wega-dom`, `wega-html` and `wega-adapter-blitz` if the ownership model stays sound. No push. No CSS/layout work, no DOM rewrite.

**Outcome: all three crates were extracted.** The engine still owns exactly one document (Blitz's `BaseDocument`, by value, in `Rc<RefCell<…>>`); no second tree, no copied state, no new unsafe code, no external dependency or version change.

## 1. The Blitz adapter orphan-rule problem

There were **two** coherence problems, not one:

1. `impl wega_dom::DocumentView for blitz_dom::BaseDocument` (and `NodeView for &Node`) from a separate adapter crate: a foreign trait for a foreign type (E0117).
2. `impl From<NodeId> for NodeKey` and the reverse, which the engine used ~150 times: also foreign trait, and both types foreign to every crate except `wega-dom` and Blitz itself. **This one is independent of the document design:** any extraction that keeps `wega-dom` free of Blitz forces explicit conversions.

### Designs compared

| | A. Co-locate contract and Blitz impl | B. Owning newtype (`BlitzDocument`) | C. Borrowed adapter wrapper | E. Marker/"family" trait (found while prototyping) |
|---|---|---|---|---|
| Coherence | Fine (same crate) | Fine (local type) | Fine (local type) | Fine (local marker) |
| Contract crate free of Blitz | **No** (needs `blitz-dom`, even feature-gated; the contract's manifest names the engine it must be independent of) | Yes | Yes | Yes |
| GAT / borrowed `NodeView` | Works | Works (`BlitzNode<'a>` is a `Copy` `&Node`) | Works | Works |
| `Rc<RefCell<Doc>>` ownership | Unchanged | Same ownership, document type renamed | **Breaks**: a wrapper `&'a mut BaseDocument` borrows the `RefCell` guard for its whole life, so a stored `Parser` (kept across steps while scripts run) cannot exist | Unchanged document type |
| Mutable access / lifetimes | Direct | Direct (`DerefMut`) | One borrow per wrapper; needs a handle trait with `Ref` leaking into the contract to be usable long-lived | Direct |
| Engine call-site changes | 0 (but the ~150 id conversions still needed if the contract crate has no Blitz) | ~25 type sites + constructors; method syntax unchanged | ~100 wrap sites + a handle trait | ~100 `Blitz::node(&doc, k)` sites (or an engine-local extension trait) |
| Allocations / dispatch | None | None (wrapper is the value; `Deref` and `key()` inline) | None per call, but wrapper churn | None |
| Compiles without `wega-engine` | Yes | Yes | Yes | Yes |
| Blitz replaceable later | Weak (contract names Blitz) | Good (adapter owns the document type; remaining `Deref` uses are the tracked residue) | Good in theory | Good, but the contract becomes a function family instead of traits on documents |

### Prototype (scratch workspace, mock foreign types; B was then built for real)

`/tmp/orphan-proto` (3 crates: foreign type, contract with a `Parser` that keeps an `Rc<RefCell<D>>` across steps, one adapter per design):

- Separate adapter crate implementing the trait on the foreign document, and `From<NodeId> for NodeKey`: **E0117** for both.
- B: compiles, including the long-lived parser handle and use of the foreign API through `Deref`.
- C: one-shot use compiles; **returning/storing the parser fails with E0515** (`guard` is borrowed): it can only live for one call.
- E: compiles and leaves the document type untouched, at the cost of every contract call becoming a static function on the marker.

B was then implemented against the real stack (real parser, real DOM bindings, 400+ tests). The prototypes for C and E used a mock document, so they demonstrate coherence and lifetime behaviour, not performance.

### Decision: B (owning newtype) — chosen on merit, not only for coherence

- It is the smallest change that keeps the contract Blitz-free and the parser's long-lived `Rc<RefCell<…>>` handle valid. A is smaller still but defeats the purpose; C breaks the parser; E is sound but rewrites every call site into a less natural form and keeps the document type Blitz's.
- The wrapper is a plain tuple struct holding the document by value (no extra allocation or indirection, no state, no unsafe). `Deref`/`DerefMut` keep the layout/paint/style/form APIs reachable, so nothing had to move that is not behind the contract yet.
- The adapter now owns the document type, which is where Blitz-specific construction and later replacement belong.
- Cost, stated plainly: the engine's document type is now `BlitzDocument`, and node ids are converted with `.key()`/`.node_id()` instead of `.into()` (~150 mechanical edits). E remains the fallback if Blitz is ever swapped for an engine whose document cannot be wrapped.

## 2. Parser diagnostics (commit `48b5881`)

Compared a callback (dynamic dispatch, lifetime juggling inside the sink), buffering parser-owned values (changes emission order, holds memory) and a generic reporter. Chosen: parser-owned `HtmlDiagnostic { message, line, url }` delivered to a `ParseReport` type parameter as events happen. The engine's `html_report.rs` maps them to `Html` warnings. Category, severity, message, URL and line are unchanged (checked on the node-budget page test's emitted diagnostics before and after). `parser.rs` no longer names `crate::diagnostics`.

## 3. Extraction results

| Crate | Path | Contents | Dependencies |
|---|---|---|---|
| `wega-dom` | `engines/dom/core` | `NodeKey`, `NodeView`, `DocumentView`, `DocumentMutation`/`Mutator`, `MutationError`, helpers (`subtree`, `sibling`, `node_info`, `attribute_ns`, …), `&mut D` forwarding impls | `html5ever`, `url` |
| `wega-html` | `engines/html/parser` | `Parser`/`Step`, tree sink, node-budget termination, `parse_fragment`, `HtmlDiagnostic`/`ParseReport` | `wega-dom`, `html5ever`, `url` |
| `wega-adapter-blitz` | `engines/adapters/blitz` | `BlitzDocument`, `BlitzNode`, `BlitzMutator`, `ToKey`/`ToNodeId` | `wega-dom`, `blitz-dom` (no default features), `html5ever`, `url`; dev: `wega-html`, `blitz-html` |

Preserved (checked by tests, not asserted): `NodeKey` identity and stale-key rejection, static dispatch and borrowing, immediate mutation with deferred Blitz finalization, no second tree, script pausing, node-budget termination, fragment parsing, XML kept in the engine (`blitz_html::from_xml` in `view.rs`). All three crates `#![forbid(unsafe_code)]`. Form state, layout and events were not moved.

### Dependency graph, before (Round 6 end) and after

Before: 10 workspace members, 18 internal edges. After: **13 members, 24 internal edges**, no cycles, checker **0 violations** (`baseline/architecture-check-after-round7.md`, `baseline/dependency-graph-after-round7.md`; Round 6: `…-after-round6.md`).

New edges (all of the diff):

```
wega-adapter-blitz --> wega-dom
wega-adapter-blitz -.dev.-> wega-html
wega-html          --> wega-dom
wega-engine        --> wega-adapter-blitz
wega-engine        --> wega-dom
wega-engine        --> wega-html
```

Not present, by design: any edge from `wega-dom`, `wega-html` or `wega-adapter-blitz` to `wega-engine`; `wega-html → adapter`; `wega-dom → html`. The one new contract-to-contract edge (`wega-html → wega-dom`) is the designed one; the earlier plan said "no wega-dom dependency if the sink is a trait" and was superseded because the sink is the contract.

### Boundary enforcement added

- `tests/boundary.rs` in each new crate (runs under `cargo test`): allow-list of `[dependencies]`/`[dev-dependencies]` read from the crate's own `Cargo.toml`, and a scan of its sources for `wega_engine`, `blitz_`/`BaseDocument` (contract and parser) and adapter names. Verified to fail: adding `blitz-paint` to `wega-html` produced "unexpected dependency `blitz-paint`".
- Architecture checker (`ARCH-003`) now allows only `wega-html → wega-dom` among contract crates and forbids the reverse; new forbidden edges: `wega-dom|wega-html|wega-adapter-blitz → wega-engine`, `wega-dom|wega-html → wega-adapter-*`. Unit tests for each. The checker remains report-only; the `cargo test` boundary tests are the enforcing layer.
- Found and fixed by the checker: the new crates declared `unsafe_code` forbid in `Cargo.toml` lints only, not as `#![forbid(unsafe_code)]`.

## 4. Parity and validation

- Full engines workspace suite: **406 passed, 0 failed, 6 ignored** (401 before; +6 boundary tests, −1 old in-library boundary test). Tests moved with their code: 12 contract tests + 2 ignored benchmarks, and 17 parser tests, now in `wega-adapter-blitz` (they run in ~0.1 s there because that crate builds Blitz without system fonts; the engine's own copies took ~12 s).
- DOM parity: contract vs direct Blitz access for every node of a fixture (identity, kind, names, attributes, links, siblings); mutation, stale-key and clone tests unchanged.
- Parser parity: 9 tree-construction cases serialise identically to `blitz-html`; 23 fragments × 17 contexts identical to the old Blitz fragment sink (oracle kept in `tests/html_parser.rs`); 9 node-budget shapes at engine level still show the limit page and recover.
- Broker mode (`security`, `forms`, `scripts`, `documents`, `history`; plus `lib`, `dom`, `dom_identity`, `pages`) and the required-worker test (`inherited_broker`, with a content helper from `wega-content-bundle`): pass.
- Root `cargo check --locked`, `cargo metadata --locked` for the root, engines and tools workspaces, `cargo fmt --check`, workspace `clippy -D warnings` (engines and tools): pass.
- **Cargo.lock review:** `engines/Cargo.lock` +32 lines, **0 removed**: the three new workspace packages and the engine's new edges. No external package added, removed or re-versioned; root and tools lockfiles untouched.
- **Not run:** Windows/Linux builds, MSVC Skia jobs, native-sandbox ignored tests.

## 5. Performance

Debug builds. Baseline = Round 6 end (`5d30d75`), built in an isolated worktree and target directory; current = this branch. Four runs of each build, **alternating** (new, old, new, old, …), median of the four runs, same benchmark file (`tests/hot_path_bench.rs`). **The machine was not quiet** (load average ~14 from other processes), so this is not the "quiet machine" comparison asked for; the last column is the spread (max−min) of the *old* build's four runs, i.e. the noise floor.

| Benchmark | Old | New | Change | Old-run spread |
|---|---|---|---|---|
| evaluate_json(1+1) (µs/call) | 13.14 | 13.41 | +2.1% | 8% |
| dom ops: create+setAttribute+append+getAttribute (µs) | 379.89 | 379.74 | −0.0% | 2% |
| lookup + nodeType + nodeName + parentNode (µs) | 23.41 | 24.05 | +2.8% | 6% |
| deep clone, 51 nodes (µs) | 278.99 | 267.56 | −4.1% | 11% |
| appendChild + removeChild (µs/node) | 366.75 | 367.80 | +0.3% | 3% |
| setAttribute + removeAttribute (µs/pair) | 51.12 | 51.27 | +0.3% | 8% |
| text node data update (µs) | 23.19 | 24.02 | +3.6% | 5% |
| innerHTML small fragment (µs) | 175.85 | 178.40 | +1.5% | 6% |
| innerHTML large fragment, 900 nodes (ms) | 42.61 | 43.28 | +1.6% | 2% |
| createElementNS (µs) | 52.68 | 52.78 | +0.2% | 5% |
| cloneNode(false) (µs) | 95.69 | 95.30 | −0.4% | 13% |
| style.setProperty + removeProperty (µs/pair) | 67.90 | 71.77 | +5.7% | 8% |
| parse 9,000-node document (ms) | 413.94 | 408.21 | −1.4% | 2% |
| create_view + set_html (ms/document) | 198.49 | 196.43 | −1.0% | 2% |

Reading: 12 of 14 differences are inside the noise floor and the largest (+5.7%, inline style) is below the old build's own run-to-run spread. There is no sign of a systematic cost from the wrapper, the id conversions or the extracted crates (all inline: `BlitzDocument` is the value itself, `key()`/`node_id()` are identity on a `u64`). Style and text updates (+5.7%, +3.6%) are the only ones worth re-measuring; because the machine was not quiet, **treat the whole comparison as inconclusive at the few-percent level** and re-run on an idle machine before quoting a figure. A first interleaving attempt in this round was discarded because the disk was full and the benchmark binaries were corrupt; the numbers above come from the rebuilt binaries.

## 6. Remaining direct Blitz dependencies

Engine source lines matching `get_node(` / `get_node_mut(` / `element_data()` / `NodeData::` / `.mutate()` / `blitz_dom::` / `blitz_html::` (same grep as Round 6): **149 → 100** (`dom.rs` 58, `view.rs` 35, `document_host.rs` 5, `network.rs` 1, `script.rs` 1; `parser.rs` and the `dom_model` module are gone from the engine). `BaseDocument` is named in 2 engine places (construction). The engine still depends on `blitz-dom`, `blitz-html`, `blitz-paint` and `blitz-traits` directly (ARCH-007 informational) and uses `Deref` to `BaseDocument` for everything not behind the contract. Approximate usage by theme (overlapping greps): layout/geometry ~37, paint/render/viewport ~57, form/input state ~29, style/CSS ~16, query/selectors ~15, events/focus/hit-test ~15, navigation/network/font configuration ~9, serialization ~7, `TreeTraverser` ~7, document construction ~5.

## 7. Next steps (not started)

1. **Events:** event dispatch, hit-testing, focus and hover live in `view.rs` against Blitz; they need a contract (targets, propagation path, default actions) before anything else can leave `view.rs`.
2. **Layout:** `box`, `clientRects`, `scroll*`, `offset*`, `resolve`: a geometry view over the document (read-only) is the smallest first step; layout invalidation after mutation already rides on the mutation scope.
3. **Style:** inline-style blocks, `CSS.supports`, computed style and `<style>` processing; `wega-adapter-stylo` is the planned home.
4. **Rendering/paint:** `blitz_paint::paint_scene` and viewport/scale handling are untouched; the Skia side is already isolated in `wega-adapter-skia`.
5. **Form state:** text-input editors, checkbox/radio/file state and form submission stay Blitz widget state; they need their own contract (`dom/forms`-like) rather than being added to `Mutator`.
6. Move construction/configuration (`DocumentConfig`, fonts, net provider, XML) into the adapter so that `BaseDocument` is no longer named in the engine; then `Deref` can be dropped call site by call site.
7. Optional: give XML parsing its own boundary only once a second consumer exists.
8. Environment note: the machine's disk reached 100% during validation (scratch builds in `/tmp`), which corrupted one scratch build artifact; scratch targets were deleted and rebuilt, and the final suite was re-run afterwards. Keep scratch targets out of the way or clean them between rounds.

## 8. Commits (not pushed)

`48b5881` parser diagnostics boundary; `377f723` `BlitzDocument`/`BlitzNode`/explicit id conversions inside the engine; `5c4b3b2` extract `wega-dom` and `wega-adapter-blitz`; `c65c5b2` extract `wega-html`; `794aead` boundary tests, checker rules, READMEs, baselines; `e3e5b38` clippy fixes; (this commit) report and progress update.
