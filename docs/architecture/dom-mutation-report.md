# DOM mutation boundary report (Round 5)

**Result:** a Wega-owned mutation contract (`DocumentMutation`/`Mutator`) is implemented in `wega-engine::dom_model`, backed only by Blitz in `dom_model/blitz.rs`, and used by the incremental HTML parser and by the DOM creation, insertion, removal, attribute and text operations. Blitz stays the single owner of document state. No crate was created. Baseline 371 passed → **379 passed, 0 failed, 6 ignored**.

## 1. Mutation call-site inventory

Searched in `dom.rs`, `view.rs`, `parser.rs`, `document_host.rs` (Blitz `DocumentMutator` methods). **62 mutator call sites** plus ≈15 direct form-state writes before this round.

| Operation | Sites before (first locations) | Class | Shared by parser and DOM ops |
|---|---|---|---|
| `create_element` | 10 (`dom.rs:1114,1133,1215,1223,1241,1264,1265,1474`, `parser.rs:57,112`) | creation | **yes** |
| `create_text_node` | 5 (`dom.rs:1242,1427,1479`, `parser.rs:134,151`) | creation | **yes** |
| `create_comment_node` | 4 (`dom.rs:1243,1263`, `parser.rs:118,121`) | creation | **yes** |
| `template_contents` | 3 (`dom.rs:1174,1210`, `parser.rs:178`) | fragment handling | **yes** |
| `append_children` | 5 (`dom.rs:1256,1497`, `parser.rs:128,135,332`) | insertion | **yes** |
| `insert_nodes_before` | 3 (`dom.rs:1256`, `parser.rs:145,152`) | insertion | **yes** |
| `replace_children` | 4 (`dom.rs:1224,1229,1420,1428`) | insertion/removal | DOM |
| `remove_node` | 2 (`dom.rs:1502`, `parser.rs:195`) | removal (detach) | **yes** |
| `remove_and_drop_node` | 7 (`dom.rs:1135,1220,1224,1229,1776,1779`, `parser.rs:333`) | removal (drop) | **yes** |
| `reparent_children` | 1 (`parser.rs:201`) | parent/child | parser |
| `append_text_to_node` | 2 (`parser.rs:132,149`) | text | parser |
| `add_attrs_if_missing` | 1 (`parser.rs:189`) | attributes | parser |
| `last_child_id`, `previous_sibling_id`, `node_has_parent`, `child_ids` | 5 (`parser.rs`) | queries | parser |
| `set_attribute` | 5 (`dom.rs:1274,1286,1441`, `view.rs:1134,1184`) | attribute | DOM |
| `clear_attribute` | 2 (`dom.rs:1453,1463`) | attribute | DOM |
| `set_node_text` | 1 (`dom.rs:1410`) | text | DOM |
| `deep_clone_node`, `remove_style_property` | 1 + 1 (`dom.rs:1239,1283`) | clone/style | DOM |

The **~13 shared parser/DOM operations** (round 4) are the rows marked "yes" plus `replace_children`/`reparent_children`: create element/text/comment, template contents, append, insert-before, remove, remove-and-drop and the parent/sibling queries.

Direct non-mutator writes (outside this contract, unchanged): `get_node_mut`/`element_data_mut` (5: form state), `set_checkbox_input_checked` (4), `text_input_data_mut` (3), `mark_ancestors_dirty` (6), `toggle_radio` (1).

Other lifecycle facts found while reading the code:

- **Shadow DOM** is Wega-owned (`components.rs`, keyed by `u64`) and mutates plain Blitz children; the DOM ops notify it after attribute changes (`components.attribute(...)`); unaffected.
- **Mutation notifications** to script are JavaScript-side (`observers.js`); natively the only notification is `Dom::invalidate` (revision + layout-dirty) at each script task start (`begin_task`) and after a few ops.
- **Style/layout invalidation** is performed *inside* each Blitz operation (damage bits, restyle hints, `mark_ancestors_dirty`); `Dom::resolve` recomputes lazily.
- **Identity/stale handles:** Blitz ids carry a slot version, so a stale id returns `None` from `get_node`, but most `DocumentMutator` methods index the node slab directly and **panic** on a stale id; several unwrap the anchor's parent (`insert_nodes_before`) or `expect("Not an element")` (`add_attrs_if_missing`).
- **Atomicity:** `add_children_to_parent` detaches *every* child from its old parent before inserting; a stale child halfway through the list therefore leaves the earlier children detached.

## 2. Contract design

`dom_model/mod.rs` (Blitz-free; enforced by a test). Uses `html5ever::QualName` (a markup5ever type, already used throughout the engine) for element and attribute names, to avoid allocating strings per element.

```rust
pub trait DocumentMutation: DocumentView {
    type Mutator<'a>: Mutator where Self: 'a;
    fn mutate(&mut self) -> Self::Mutator<'_>;          // exclusive borrow: scopes cannot nest
    fn refresh_style_element(&mut self, key: NodeKey);   // parser: re-read <style> text when closed
}
pub trait Mutator {
    // queries (stale -> None)
    fn parent / last_child / previous_sibling(&self, NodeKey) -> Option<NodeKey>;
    // creation (new nodes are detached)
    fn create_element(&mut self, QualName, impl IntoIterator<Item = NewAttribute>) -> NodeKey;
    fn create_text / create_comment(&mut self, &str) -> NodeKey;
    fn template_contents(&mut self, NodeKey) -> Result<NodeKey, MutationError>;
    // structure
    fn append_children / insert_before / replace_children / remove / remove_and_drop / reparent_children -> Result<(), MutationError>;
    // text and attributes
    fn set_text / append_text / set_attribute / clear_attribute / add_attributes_if_missing -> Result<(), MutationError>;
    fn finish(self);                                      // explicit end of scope
}
pub enum MutationError { Stale, Detached, InvalidParent, NotElement, NotText, HierarchyCycle { parent, child } }
pub fn mutate<D: DocumentMutation>(doc: &mut D) -> D::Mutator<'_>;   // avoids the inherent-method shadowing
```

Design choices:

- **Static dispatch** (generic functions, a GAT mutator): no `dyn` anywhere. `NewAttribute` is passed as an iterator so the parser creates one `Vec` (inside the Blitz mapping), exactly as before, and a single child is passed without allocating (`with_ids` uses a stack array).
- **`NodeKey` identity and Blitz's version protection are preserved** because keys are the same `u64`; the contract only adds *checks* so a stale key becomes `MutationError::Stale` instead of a panic.
- **No Blitz type** in any signature; reads continue to use `DocumentView`/`NodeView`; `NodeView` gained `qual_name`, `DocumentView` gained `root` and `base_url`.
- Deliberately excluded: `deep_clone_node`, style properties, sub-documents and form-state writes (one consumer each, different shapes).

## 3. Commit/finalization semantics (decision)

Facts from Blitz: operations are **applied immediately**; `DocumentMutator` defers only side effects (window title, `<style>` text processing, form owners, autofocus, animation flag) to `flush()`, which `Drop` runs, and then requests a redraw. `flush` can call only the shell provider (title, redraw, focus); it never runs script.

| Question | Decision |
|---|---|
| What is a transaction? | A **scope**: one `Mutator` between `mutate()` and `finish()`/drop. It groups deferred side effects only; it is *not* a unit of atomicity. |
| Applied or staged? | **Applied immediately**; nothing is staged, so the document is consistent and readable between operations of a scope. |
| Commit-on-drop vs explicit commit | **Both, with one meaning.** Dropping runs the deferred work (Blitz's `Drop`); `finish(self)` is a named, explicit way to end the scope at a visible point. Wega adds **no `Drop`** of its own, so no Wega destructor can panic or reorder anything. |
| What on `Err`? | The operation validates *before* touching the document and changes nothing (verified by tests that compare a structural dump before and after every rejection). A caller that wants a batch to be atomic must validate before starting. |
| Panic / unwinding | There is **no rollback**. Applied operations stay applied; Blitz's drop still runs its deferred work during unwinding (best effort on a document the panic may have left inconsistent). The document is to be treated as poisoned after a panic; the next scope still works (tested). A destructor cannot provide atomic rollback, so none is claimed. |
| Nested mutations | Not possible: `mutate(&mut self)` is an exclusive borrow, enforced at compile time. Consumers that need several operations use one scope; the one-shot temporary pattern (`mutate(&mut doc).op()`) still commits per operation, as before. |
| Partial application | Per operation: no (validate-then-apply). Per sequence: yes, by design (no rollback). |
| When does invalidation occur? | Inside each operation (Blitz damage/restyle/dirty bits) — verified by a layout test (a 10 px child, then an inserted 15 px child, reads 25 px after the scope). Deferred effects (title, `<style>`, form owners, autofocus, redraw) run at scope end. |
| Can observers/callbacks run during finalization? | No script can: Blitz's flush calls only the shell provider. Callers may therefore hold their `RefCell` borrow across `finish`. |

Validation rules implemented in `BlitzMutator`: every key must resolve (`Stale`); a parent must be able to have children (`InvalidParent`); `insert_before` needs a parent for the anchor (`Detached`); attributes/template contents need an element (`NotElement`); text operations need a text node (`NotText`); no node may be inserted below itself or one of its descendants (`HierarchyCycle`, also for `reparent_children`). The ancestor walk runs only for children that themselves have children (a leaf can only be an ancestor of `parent` by being `parent`), so parser appends of fresh nodes pay one slab lookup.

## 4. Source changes and commits

| Commit | Change |
|---|---|
| `524ea1c` | benchmarks for mutation, parsing, attributes and text (baseline recorded before any change) |
| `bc94041` | the contract, `BlitzMutator`, DOM consumers (`create`/`createText`, `append`, `insert`, `remove`, `setText`, `setAttr`/`removeAttr`, innerHTML staging); 6 unit tests, overhead test |
| `498458a` | parser integration (below); parity tests |
| (this commit) | this report and the progress update |

Rejected mutations in the DOM ops keep their previous JavaScript-visible messages (`Node no longer exists`, `Cannot create a DOM cycle`) through `Dom::mutation_error`.

## 5. HTML parser integration

- html5ever's `TreeSink` is the parser-to-document interface; `Sink` and `Parser` are now **generic over `DocumentMutation`** (default `BaseDocument`) with `NodeKey` handles. All 13 tree-construction operations go through `Mutator`; `elem_name` uses `NodeView::qual_name`; `<style>` close uses `refresh_style_element`. Rejected operations (a tree-construction bug) are reported as HTML warning diagnostics instead of panicking inside Blitz.
- **Not changed:** fragment parsing (`parse_fragment`, innerHTML/insertAdjacent) and XML documents still use `blitz_html` (`DocumentHtmlParser`, `HtmlDocument::from_xml`); the initial empty document is still created by Blitz. These need a raw `DocumentMutator` and a fragment root transfer; porting them is a Round 6 candidate.
- No second parser state, no second tree, no conversion of nodes (the handle is the same `u64`), and html5ever is not reimplemented.
- **Parity:** nine tree-construction cases (misnested formatting, foster parenting in tables, `html`/`body` attribute merging, nested templates, select/optgroup, SVG foreignObject, `<style>`, textarea, nested anchors/forms) produce byte-identical serialisation to `blitz_html::HtmlDocument::from_html`; scripts still pause the parser and adjacent text merges into one node.
- **Pre-existing bug found (not introduced here):** when a document exceeds `Limits::dom_nodes` inside one chunk, every later node becomes the same placeholder handle and html5ever panics (`no current element`, `tree_builder/mod.rs:685`). Reproduced with the engine (`set_html` of 400 `<p>` under `dom_nodes = 100` panics) and with the *old* `Parser`. The ignored test `parser::tests::the_node_budget_stops_the_tree_growing` documents it. A content process can be crashed by a document with more than the node budget; this is a robustness/denial-of-service issue worth its own fix (stop feeding at the budget, or give overflow nodes distinct inert handles).
- `Step::Script` now carries a `NodeKey` (converted with a free `From` at the view edge).

## 6. Benchmarks

Quiet machine (load average ≈ 1), debug build, medians of 7, three runs per column; uncertainty is the observed spread between runs (about ±0.5%).

| Metric | Before | After |
|---|---|---|
| parse 9,000-node document (`set_html`) | 285 / 288 / 288 ms | 289 / 293 / 291 ms (**+1.5%**) |
| `appendChild` + `removeChild` per node | 244 / 242 / 244 µs | 246 / 244 / 245 µs (+0.8%) |
| `setAttribute` + `removeAttribute` per pair | 35.3 / 34.9 / 35.3 µs | 35.6 / 35.6 / 35.8 µs (+1.3%) |
| text node `data` update | 15.9 / 15.9 / 15.7 µs | 16.1 / 16.1 / 16.0 µs (+1.2%) |
| `evaluate_json("1+1")` | 8.1 / 8.2 / 8.2 µs | 7.9 / 8.1 / 8.2 µs |
| DOM ops iteration | 255 / 254 / 256 µs | 261 / 255 / 258 µs |
| lookup + nodeType + nodeName | 16.4 / 16.2 / 16.2 µs | 16.5 / 16.3 / 16.4 µs |
| deep clone (51 nodes) | 193 / 189 / 192 µs | 192 / 186 / 189 µs |
| document lifecycle | 144 / 143 / 143 ms | 141 / 143 / 143 ms |

Rust-level micro-benchmark (`dom_model::tests::mutation_overhead_against_direct_access`, 4,000 elements, create + append + set attribute + remove, medians of 9): direct Blitz 634-826 ms vs contract 624-699 ms (−1.4% to −15%, i.e. no measurable cost; the first sample was noisy).

Interpretation: all mutation-heavy JavaScript paths are 0.8-1.5% slower, consistently across three runs, which is above the run-to-run spread; the contract's per-operation validation (one or two slab lookups and a kind check) is the likely cause and is small against the 15-250 µs a Blitz operation costs. The Rust-level test, which isolates the operation cost, shows no difference. Not treated as a regression; if it matters, the validation can be dropped for parser-created keys behind a debug assertion.

Incidental finding: `remove_node` on a parent with N children is O(N) (`children.retain`), so removing N siblings is O(N²) in Blitz (20,000 siblings took ≈ 8 s in the overhead test, hence N = 4,000). This is Blitz behaviour, identical through the contract.

## 7. Test results

- Full engines suite: **379 passed, 0 failed, 6 ignored** (371 baseline + 6 `dom_model` mutation tests + 2 parser tests; the ignored tests are the two native-sandbox tests, `hot_path_bench`, `dom_model` read and mutation overhead tests, and the documented parser-budget test); 400 s wall, 1.55 GB max RSS, 181 MB peak footprint.
- New tests: ordering and same-parent moves; parent-link integrity after every operation; detached and re-inserted nodes (including `Detached`); stale keys across all 12 mutating operations and slot reuse not aliasing; rejected operations (cycles, text as parent, wrong node kinds, a batch with one stale child) leaving a structural dump unchanged; panic inside a scope (applied operation stays, document accepts a new scope); layout reflects mutations after the scope; parser parity (9 inputs), script pausing and text merging.
- Existing suites exercising the migrated paths all pass: `dom`, `dom_identity`, `scripts`, `forms`, `custom_elements`, `frames`, `documents`, `web`, `pages`, `input`, worker and Canvas/WebGL tests. Broker mode (security, forms, scripts, documents, history) and the required-worker test pass. Root `cargo check --locked`, both `cargo metadata --locked`, `fmt --check`, Clippy `-D warnings` pass; architecture checker: 0 violations. No manifest or lockfile changed; no new external dependency.
- Not run: Windows/Linux, MSVC Skia jobs, native-sandbox ignored tests.

## 8. Remaining direct Blitz references

Outside `dom_model` (the one allowed mapping layer): **514 Blitz call sites over 98 methods** (round 4: 553 / 112): `dom.rs` 289, `view.rs` 149, `parser.rs` 33, `document_host.rs` 24, `dom_host.rs` 9. Direct `DocumentMutator` operation sites fell from 62 to ≈32 (remaining: the fragment-parse path and its staging, the `createNS`/comment/fragment/document creation family, `deep_clone_node`, style-property writes, two `set_attribute` calls in `view.rs`, direct form-state writes). Still Blitz-typed: node fields (`children`, `parent`, `data`, `element_data()`), events, layout/paint/hit testing, `HostContext`/`Dom` holding `Rc<RefCell<BaseDocument>>`, `blitz_html` for fragments/XML.

## 9. Proposed Round 6

1. **Finish the mutation migration** (≈32 sites): the `createNS` family, `deep_clone_node`/style property (add only if more than one consumer), the two `view.rs` attribute writes; route fragment parsing through the generic `Sink` (html5ever `parse_fragment_for_element`) and compare against `blitz_html` with the same parity tests, then drop `blitz_html` for fragments; decide XML separately.
2. **Fix the node-budget panic** (found above) with a regression test; it is independent and security-relevant.
3. **Read-path cleanup:** move the remaining `node.children`/`.parent`/`NodeData` matches and `element_data()` reads in `dom.rs`/`view.rs` onto `NodeView` (only where it removes call sites without adding methods).
4. **Events:** wrap Blitz's event types at the `view.rs`/`document_host.rs` edge.
5. **Only after 1-3:** consider crate extraction. `wega-dom` (the contract, `html5ever::QualName` + `url`) and `wega-html` (generic `Sink`) now have two real consumers each in the engine; `wega-adapter-blitz` would hold `dom_model/blitz.rs`. The parser's dependency is `html5ever` + the contract only, so `wega-html` is viable once fragments no longer need `blitz_html`; this round did not extract it.
