# DOM migration report (Round 4)

**Result:** a Wega-owned, read-only document view (`dom_model`) is implemented inside `wega-engine`, validated against the real consumers and committed. Blitz stays the document engine; no crate was created and no DOM, CSS or layout code moved. The mutation side and the HTML parser are analysed, with a concrete Round 5 sequence.

## 1. Blitz call-site inventory (measured)

Method: public method names of Blitz's `BaseDocument`, `DocumentMutator`, `Node`, `ElementData` (vendored `blitz-dom`, 342 names) were searched across `engines/wega/src` (`.name(` / `::name(`; generic names such as `get`, `len`, `insert` excluded), then classified. Counts are call sites.

### 1.1 Where the coupling lives

| File | Blitz call sites | `NodeId` mentions | `NodeData::` matches | Notes |
|---|---|---|---|---|
| `dom.rs` | 315 | 35 | 29 | the JavaScript DOM host: ≈57% of all coupling |
| `view.rs` | 149 | 21 | 0 | lifecycle, events, layout, paint, hit testing, scroll |
| `parser.rs` | 46 | 28 | 0 | `TreeSink` over `DocumentMutator` |
| `document_host.rs` | 24 | 7 | 0 | script/event plumbing (`NodeId`, `handle_dom_event`) |
| `script.rs` | 2 types | 2 | 0 | `HostContext` carries `Rc<RefCell<BaseDocument>>` |
| `dom_host.rs`, `workers/*`, `canvas/*`, `network.rs` | ≤ 9 each | 0 | 0 | essentially none (the V8 adapter and workers are Blitz-free) |
| `components.rs` | 0 | 0 | 0 | custom elements and shadow DOM are already Wega-owned, keyed by `u64` |

Total: **553 call sites over 112 distinct Blitz methods**, plus type/enum uses (`blitz_dom::{Point, ScrollBehavior, StyleThreading, BoundingRect, TreeTraverser, SpecialElementData::FileInput}`, `blitz_traits::{events, net, shell}`, `blitz_html::{HtmlDocument, DocumentHtmlParser}`, `blitz_paint::paint_scene`).

### 1.2 By operation class

| Class | Calls | Distinct | Dominant operations |
|---|---|---|---|
| 1 Document lifecycle | 69 | 9 | `url` (52), `set_base_url`, `viewport`/`set_viewport`, `StyleThreading`, font context |
| 2 Node creation / removal | 54 | 14 | `create_element` (10), `remove_and_drop_node` (7), `create_text_node`, `append_children`, `replace_children`, `create_comment_node`, `insert_nodes_before`, `template_contents` |
| 3 Tree traversal / query | **173** | 10 | `get_node` (96), `mutate` (51, used as a one-shot temporary), `root_node`, `query_selector(_all)`, `get_element_by_id`, `.children`/`.parent` fields (≈34) |
| 4 Attribute, text, form state | **151** | 14 | `element_data` (42), `attr` (37), `set_attribute`, `clear_attribute`, `text_content`, checkbox/text-input state |
| 5 Events, focus, forms | 24 | 11 | `handle_dom_event` (5), `synthetic_click_event_data`, `submit_form`, `set_focus_to`, hover/focus ids |
| 6 Layout / computed style | 34 | 12 | `resolve` (15), `final_layout`, `has_boxes`, `resolved_style_value`, scroll geometry, `mark_ancestors_dirty` |
| 7 Rendering / hit / scroll / a11y | 14 | 8 | `hit`, `paint_scene`, `build_accessibility_tree`, `scroll_node_by`, viewport scroll |
| 8 JavaScript binding ops | (no separate API) | – | `Dom::call` dispatches ≈120 named ops; they use classes 2-6 above |
| other (one-offs) | 34 | 34 | `deep_clone_node`, `process_style_element`, client-rect helpers … |

**Takeaway:** classes 3 and 4 (read paths: node lookup, kind/name, attributes, tree links) are **59% of all call sites** and are purely read-only; creation/removal is ≈10% and concentrated in ~13 mutator operations shared by `dom.rs` and `parser.rs`. Performance-sensitive operations: `get_node` and attribute lookup (per JS DOM call), tree walks (`subtree`, `tree_root`, ancestor loops), `resolve` (style + layout) before geometry reads, and `paint_scene`.

## 2. Current ownership model

| Concern | Owner today | Boundary |
|---|---|---|
| Document tree | `Rc<RefCell<BaseDocument>>`, created by the view per navigation (`HtmlDocument::from_html`/`from_xml`, `BaseDocument::new`), shared with the document host (`Dom`), the incremental `Parser` and `HostContext` | single source of truth, Blitz-owned |
| Node identifiers | Blitz `NodeId` (`blitz_traits::node_id`): a `u64` packing a slot index and a **version**; stale ids never resolve after slot reuse | Wega already treats it as an opaque `u64` |
| JS-visible identity | the same `u64` as a decimal string in `NativeHost::call` arguments (`NodeId::from_u64`/`as_u64`); JS caches wrappers per realm; `bindingCheck` validates liveness | Wega-owned wire form |
| Retention of removed nodes | `Dom.retained` + `collect_detached`: nodes held by JS wrappers survive removal | Wega-owned semantics over Blitz storage |
| Mutation state | Blitz `DocumentMutator` (deferred processing of title/style/form nodes on drop); Wega adds `revision` and `layout_dirty` counters in `Dom` | split, by design |
| Event listeners | JavaScript (`semantics.js`/`observers.js`, `wegaDomHooks`); native dispatch through Blitz `handle_dom_event` with a callback that feeds events back to JS | Wega JS + Blitz native |
| Layout / style state | inside `BaseDocument` (Stylo + Taffy + Parley); `Dom::resolve` triggers it lazily | Blitz-owned |
| Custom elements / shadow DOM | `components.rs` (`Registry`, `ShadowTree`) keyed by `u64`; no Blitz types | Wega-owned |
| Document lifetime | view-owned; one `Dom` and one realm per committed document; teardown drops them | Wega-owned |

Boundary between Wega behaviour and Blitz implementation: Wega owns *identity, retention, budgets, shadow/custom-element semantics and the JS protocol*; Blitz owns *storage, mutation processing, style, layout, paint, hit testing and native events*. No parallel DOM trees exist; the model added this round copies no state.

## 3. The proposed Wega DOM interface

### 3.1 Options evaluated

| Option | Verdict |
|---|---|
| One trait mirroring `BaseDocument` (187 methods) | rejected: huge, mostly unused (only 112 names are used at all) |
| Trait returning `&blitz Node` | rejected: Blitz types leak |
| Wrapper structs per node | rejected: allocation and duplicated state |
| `dyn` objects | rejected for hot paths: a virtual call per node access |
| **Concrete `NodeKey(u64)` + generic `DocumentView` with an associated borrowed `Node<'a>` handle** | **chosen**: static dispatch, zero wrapper allocation, no copied state |

### 3.2 `dom_model` (`engines/wega/src/dom_model/mod.rs`, ≈150 lines, no Blitz mention)

```rust
pub struct NodeKey(u64);                       // the identity JS already holds
pub enum NodeKind { Document, Element, AnonymousBlock, Text, Comment }
pub struct ElementName<'a> { local, namespace, prefix }   // borrowed &str

pub trait NodeView<'a>: Copy {                 // borrowed, Copy handle
    fn kind(self) -> NodeKind;
    fn element_name(self) -> Option<ElementName<'a>>;
    fn attr(self, name: &str) -> Option<&'a str>;
    fn parent(self) -> Option<NodeKey>;
    fn children(self) -> impl ExactSizeIterator<Item = NodeKey> + 'a;
}
pub trait DocumentView {
    type Node<'a>: NodeView<'a> where Self: 'a;
    fn node(&self, key: NodeKey) -> Option<Self::Node<'_>>;   // None for dropped/unknown keys
    fn element_by_id(&self, id: &str) -> Option<NodeKey>;
}
// Shared behaviour written once against the interface:
pub fn attribute / subtree / node_info / tree_root
```

Contract rules: keys referring to dropped nodes must never resolve to another node (Blitz's versioned ids provide this); the view is read-only (mutation stays on the engine); the module must not name Blitz (`dom_model/blitz.rs` is the only mapping; a unit test enforces the rule). `NodeId ⇄ NodeKey` are `#[inline(always)]` `From` impls of the same `u64`.

Explicitly **not** included: mutation, events, layout/style, hit testing. Reason: each has a different shape (deferred mutator, native event callbacks, geometry), and a contract is only added when it has real consumers and a measured need.

## 4. Prototype results

Real consumers moved onto the interface (all in `dom.rs`): `bindingCheck` liveness (the JS-to-DOM guard on every typed binding call), `getById`, `info` (nodeType/nodeName/localName/namespace/prefix), `eventParent` and shadow-root walks (`tree_root`), `Dom::attribute`, `Dom::subtree` (used by clone, innerHTML, retention, components).

| Check | Result |
|---|---|
| Compiles with the real engine | yes; clippy `-D warnings` clean (no dead contract methods) |
| Node identity preserved | yes: `NodeKey` carries the identical `u64`; tests cover round trips, dropped keys not resolving and slot reuse not aliasing, wrappers staying identical across mutation/reparenting, and per-document identity spaces |
| Parity with direct Blitz access | unit tests compare the model against the pre-change implementations (kept verbatim in the test) for every node of a mixed HTML/SVG/template document: `subtree` order, `node_info`, `attribute` |
| Dependency cycle | none; `dom_model` depends only on `std` (and, in `blitz.rs`, Blitz); workspace graph unchanged |
| Duplicated document state | none; handles borrow `&Node` |
| Hot-path overhead | none measurable (below) |
| Behaviour changes | one: the `info` op returns `InvalidStateError` for a missing node instead of panicking (`unwrap`) |

### 4.1 Performance (quiet machine, debug build)

Micro-benchmark on a 24,001-node tree (`dom_model::tests::overhead_against_direct_access`, ignored test; medians of 15):

| Operation | Direct Blitz | Model |
|---|---|---|
| subtree walk (24,001 nodes) | 2,619 µs | 2,630 µs |
| attribute lookup ×24,001 | 3,243 µs | **2,781 µs** (compares the string directly instead of interning a `LocalName`) |

JavaScript-level hot paths (`tests/hot_path_bench.rs`, medians of 7, three runs each):

| Metric | Before | After |
|---|---|---|
| `evaluate_json("1+1")` | 8.4 / 8.4 / 8.4 µs | 8.2 / 8.2 / 8.1 µs |
| DOM ops iteration | 256 / 261 / 263 µs | 258 / 264 / 257 µs |
| lookup + nodeType + nodeName + parentNode | 16.3 / 17.1 / 16.9 µs | 16.8 / 16.9 / 16.2 µs |
| deep clone of a 51-node subtree | 191 / 204 / 202 µs | 203 / 208 / 193 µs |
| document lifecycle | 145 / 151 / 150 ms | 150 / 153 / 141 ms |

An intermediate version that allocated a `Vec` per `bindingCheck` showed a 2-4% regression in the DOM-op metrics; it was rewritten to check sequentially (the original behaviour), and the numbers returned to the baseline range. No dynamic dispatch is used in these paths.

## 5. Dependency graph before and after

Unchanged at crate level (10 members, 18 edges; [`baseline/dependency-graph-after-workers.md`](./baseline/dependency-graph-after-workers.md), [`baseline/architecture-check-after-dom.md`](./baseline/architecture-check-after-dom.md), 0 violations). The change is **inside** `wega-engine`: the number of files that name Blitz document types for read paths dropped by the call sites listed in §4 (new module `dom_model/` has one Blitz-aware file, `blitz.rs`, and a Blitz-free contract). No `Cargo.toml` or lockfile changed.

## 6. HTML parser findings

- `parser.rs` (335 lines) implements `html5ever::tree_builder::TreeSink` with `Handle = NodeId`, writing through ~13 `DocumentMutator` operations: `create_element` (2 sites), `create_text_node` (2), `create_comment_node` (2), `append_children` (3), `insert_nodes_before` (2), `append_text_to_node` (2), `remove_node`, `remove_and_drop_node`, `reparent_children`, `template_contents`, `last_child_id`, `add_attrs`, `root_node`. Wega adds a node budget (`admit`), incremental stepping (`Parser::step`) and diagnostics.
- `parse_fragment` (used by innerHTML/insertAdjacent) reuses **`blitz_html::DocumentHtmlParser`** (a 333-line sink of the same shape), and `HtmlDocument::from_html/from_xml` (`view.rs:360,631`) build the initial and XML documents, including stylesheet/title processing.
- **`html5ever`'s `TreeSink` already is the parser-to-document sink interface** and it is not a Blitz type. A Wega-owned builder trait would be a second copy of it with the same ~13 operations.
- The 13 operations are also the creation/removal operations `dom.rs` uses (`create_element` 8, `replace_children` 4, `remove_and_drop_node` 6, …): the parser and the DOM ops are **two real consumers of one mutation surface**, but there is still one implementation (Blitz).
- **Decision:** `wega-html` is *not* extracted. The prerequisite is a mutation contract (§8). Parsing is separable only in the sense that `Sink<M: Mutate>` could be generic over it; doing that without the contract would put `DocumentMutator` in a public API.

## 7. Rendering and layout dependencies (documented, nothing moved)

| Stage | Where | Boundary a future extraction needs |
|---|---|---|
| Style (Stylo) | inside `BaseDocument::resolve`; Wega touches Stylo only for media-query parsing in `dom.rs` (`style::…`, `stylist_device()`); `stylo_taffy` bridges to layout inside Blitz | a style contract must take a DOM element view (`selectors`/`TElement`) from the document; today only Blitz implements it |
| Layout (Taffy/Parley) | `Dom::resolve` (lazy, `layout_dirty`), `view.rs` (`resolve` before paint and geometry reads), `Node::final_layout`, scroll geometry, `StyleThreading` | geometry queries (`final_layout`, client rects, `has_boxes`, `offset_parent`: ≈34 sites) are the natural layout read interface |
| Paint | `blitz_paint::paint_scene(&mut scene, &doc, …)` in `view.rs` produces an `anyrender::Scene` (already backend-neutral) | `paint_scene` takes the whole `BaseDocument`; a paint extraction needs a paint-facing document view (nodes, styles, layout) |
| Skia raster | `wega-adapter-skia` consumes `anyrender::Scene` on its own thread | already isolated |
| Canvas / WebGL | engine-side `CanvasEngine`/`WebGLEngine`, reached through `NativeHost::call`; no document types | independent of the DOM boundary |
| Hit testing / scroll / a11y | `doc.hit`, `scroll_node_by`, `viewport_scroll`, `build_accessibility_tree` (≈14 sites, all in `view.rs`) | hit-test and a11y results already leave as ids and AccessKit trees |

## 8. Remaining blockers

1. **Mutation** (class 2/4 writes, ≈70 sites): `DocumentMutator` borrows the document mutably and processes deferred work on drop; a contract needs a GAT mutator with the same commit-on-drop semantics. Not started.
2. **`get_node` fields** still used directly in `dom.rs`/`view.rs` (`node.children`, `.parent`, `.data` pattern matches, `element_data()` mutation, form/checkbox/text-input state): ≈250 call sites remain on Blitz types.
3. **Events**: `handle_dom_event` and Blitz event types (`BlitzKeyEvent`, `BlitzImeEvent`, `DomEvent`) in `view.rs` and `document_host.rs`.
4. **Layout/paint** take `BaseDocument` wholesale.
5. `HostContext` and `Dom` still hold `Rc<RefCell<BaseDocument>>` (script runtime sees only `ScriptEnvironment`).
6. XML and fragment parsing depend on `blitz_html`.

## 9. Validation

- Full engines suite: **371 passed, 0 failed, 4 ignored** (360 + 4 `dom_model` tests + 4 `dom_identity` tests + 3 `worker_runtime` tests; the 4th ignored test is the new overhead measurement); 261 s wall on a quiet machine, 1.54 GB max RSS, 181 MB peak footprint.
- DOM, scripts, forms, custom elements, frames, documents, web, worker and Canvas/WebGL suites pass. Broker mode (security, forms, scripts, documents, history) passes; the required-worker test passes with a CI-style content helper.
- Root `cargo check --locked`, both `cargo metadata --locked`, `fmt --check`, Clippy `-D warnings` pass. Architecture checker: 0 violations. No lockfile changes.
- New tests: `dom_model` unit tests (parity, links, identity/dropped keys, contract has no Blitz types), `tests/dom_identity.rs` (wrapper identity across mutation/reparenting, node info, per-document identity spaces), `tests/worker_runtime.rs` (termination with pending timers/fetch/promises, nested async in classic and module workers including dynamic import, parallel and sequential worker churn). The worker tests cover the Round 3 tokio-context change; one observation: module workers that await at top level before installing `onmessage` miss earlier messages, identical before Round 3 (checked against `d081f98`).
- Not run: Windows/Linux builds and tests, MSVC Skia jobs, native-sandbox ignored tests.

## 10. Recommended Round 5 sequence

1. **Mutation contract, same method as the read view.** Define `DocumentMutate` (GAT `Mutator<'a>` with commit-on-drop) covering exactly the ~13 operations shared by `parser.rs` and the DOM ops; implement it for Blitz in `dom_model/blitz.rs`; move `parser.rs` and the creation/removal ops onto it; benchmark `innerHTML` and bulk creation. Success criteria: same as §4.
2. **Make the parser generic** over the mutation contract (`Sink<M>`); only then decide on `wega-html`. `blitz_html` stays for `from_html`/`from_xml`/fragments unless the sink replaces it (compare the 333-line Blitz sink with `parser.rs`).
3. **Move the read-heavy remainder** (`node.children`, `.parent`, `NodeData` matches, `element_data` reads in `dom.rs`/`view.rs`) onto `NodeView` where it reduces call sites without new methods.
4. **Events:** wrap Blitz's event types at the `view.rs`/`document_host.rs` edge in Wega-owned structs; keep `handle_dom_event` behind the same view.
5. **Only then** consider crate extraction: `wega-dom` (the contracts, depends on `std`) and `wega-adapter-blitz` (the mapping, depends on `blitz-*`), with the engine as the composition root. Layout/paint/style extraction stays out of scope until a paint-facing document view exists.
