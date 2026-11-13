# Round 6 report: parser safety, DOM migration and crate readiness

Scope: fix the node-budget panic, finish the mutation migration, move fragment parsing onto the generic tree sink, migrate the useful read paths, and decide on crate extraction. No push. No CSS/layout work.

## 1. Node-budget panic: root cause and fix

**Reproduction.** `<body>` + 400 × `<p>x</p>` with `Limits::dom_nodes = 100` panicked in html5ever (`tree_builder/mod.rs`, "no current element") through the engine path as well as the unit test.

**Root cause.** Past the budget, the sink returned *one shared placeholder* handle (a detached `<template>`) for every new node. html5ever identifies open elements by handle equality (`same_node`), so after the budget the stack of open elements, the active-formatting list and the "in scope" searches conflated unrelated entries. `</p>` popped "the" placeholder, emptied the stack, and the next insertion called `current_node()` on an empty stack. Everything after the crossing within the chunk ran against a corrupt builder. A page could crash the content process.

**Options considered.**

| Option | Verdict |
|---|---|
| Keep placeholders, patch `same_node` | Rejected: placeholders violate tree-builder invariants (distinct nodes must have distinct handles). |
| Fresh detached real nodes past the budget, discard them | Rejected: still allocates without bound until the chunk ends. |
| Stop the tokenizer by a script pause | Rejected: html5ever only allows `TokenSinkResult::Script` for tag tokens (it asserts otherwise), so comment/text floods would not stop. |
| **Count nodes, never substitute; drain the unread input when the budget is crossed; never resume** | Chosen. |

**Fix (`57a195f`).**
- `Sink` only counts created nodes (elements, comments, PIs *and* text nodes) and sets an overflow flag at `created > limit`. Every handle given to the tree builder is a real, distinct node.
- `BudgetedBuilder` wraps the `TreeBuilder` as the tokenizer's `TokenSink`. After each token, if the budget is crossed it replaces the shared input queue with an empty one, so the tokenizer runs dry after that token for every token kind. `Parser::step` reports `Step::Overflow` before it looks at any script result, never calls `end()`, and the parser is not resumable. `document.write` after termination is ignored.
- **Policy** (unchanged for users): parsing aborts, the view shows the "DOM node limit exceeded" page. The document keeps a valid, consistent partial tree until the engine replaces it.
- **Bounds.** Overshoot is at most one token's tree-construction work (implied elements, re-opened formatting elements), never proportional to the input. Tests assert ≤ budget + 64.
- No script runs after overflow, including a script whose own end tag crosses the budget.

**Second bug found while testing tiny budgets.** When the limit page itself exceeds the budget, `load_error → set_html → overflow → load_error` recursed until the stack overflowed (pre-existing; reachable only with budgets below ~15 nodes). `set_document` cleared `showing_error` before parsing, so the guard never held. A `building_error` flag now covers the error page's own parse, in both the overflow and the post-parse count checks.

## 2. Regression tests (all enabled)

Parser level (`parser::tests`): exactly at the limit; one node over; the original 400/100 case; 200,000 paragraphs with a 1,000 budget; nested `<div>`×5000; misnested formatting re-opened on later text; tables, foster parenting and templates at five budgets; script closing after and exactly at the crossing; `document.write` chunk crossing the budget and writes after termination; floods of comments, text+comments and PIs; **every budget from 0 to total+1 for every construction case** with a tree-consistency check (parent/child links agree, no node reachable twice); recovery for the next document.

Engine level (`tests/pages.rs`): nine overflow shapes (400/100, 100k, nested, formatting, table, template, comment flood, scripts around the limit, `document.write` chunk) each show the limit page, paint, accept a mutation and recover for a normal page; a budget found by search loads and one node less does not; budgets of 1, 3 and 8 no longer recurse.

Run in normal and broker (`WEGA_TEST_BROKER=1`) mode: `lib`, `dom`, `dom_identity`, `pages` pass in both.

## 3. Mutation migration (Task 2)

Remaining direct `DocumentMutator` use outside the contract: **21 sites → 0** (the only others are the test oracle and Blitz's own `Document` node creation, which is not a tree node of this contract).

Two operations were added to `Mutator`, both reusable: `clone_node(key, deep)` (backs `cloneNode`/`importNode`; shallow clone of element/text/comment, deep clone of any subtree, rejects stale and unclonable nodes) and `remove_style_property` (`element.style`). Migrated: `createElement`/`createElementNS`/`createComment`/`createFragment`, custom-element construction, shadow roots, `cloneNode`, `setAttributeNS`/`removeAttributeNS`, style writes, template contents, node release, form value attributes (`view.rs`) and fragment staging (Task 3). Not added to the contract on purpose: form-control editor state and `Document` node creation (Blitz widget state / not a tree node); recorded as deliberate residue. Preserved: `NodeKey` identity and stale rejection, immediate semantics, deferred finalization, invalidation inside each operation, and the JavaScript-visible error strings ("Node no longer exists", "Cannot create a DOM cycle", "This node cannot be cloned").

## 4. Fragment parsing (Task 3)

`parse_fragment<D: DocumentMutation>` builds `innerHTML`, `insertAdjacentHTML`, `createContextualFragment` and template-content fragments through the same `Sink`/`BudgetedBuilder`. The tree is built in place in the document; the temporary parser root's children are moved under the staging element with `append_children` and the root is dropped: no second DOM, no copy. `&mut D` implements `DocumentView`/`DocumentMutation`, so the caller's existing borrow is used with no `Rc`. Fragments now stop at the *remaining* node budget instead of building an unbounded staging tree first, and parse errors carry their line.

**Parity.** 23 fragments × 17 contexts (div, body, table, tbody, tr, td, select, colgroup, textarea, script, style, title, template, p, svg, foreignObject, math) serialise identically to the old `blitz_html` fragment sink, which stays in the test module as an oracle. Also checked: in-place identity of parsed nodes, scripts stay inert and do not pause, budgets 0..21 for every case, and an engine-level test (table contexts, `insertAdjacentHTML`, template content, SVG/foreign content namespaces, misnested markup, held wrappers keep identity, range fragments, a 100k-element fragment rejected with the page untouched).

`blitz_html` remains for XML documents (`view.rs`) and the test oracle.

## 5. Read paths (Task 4)

`NodeView` gained `attributes()` and `child_at()`, `children()` is now double-ended, and two engine-neutral helpers `sibling` and `attribute_ns` were added. Migrated in `dom.rs`/`view.rs`: liveness, element-name reads, parent/first/last/next/previous/child traversal, ancestor walks (append/insert/component root/connected), subtree and slot assignment, attribute reads (`attr`, `attributes`, `attrNS`, `setAttrNS`/`removeAttrNS` lookups, custom-element attribute replay, `windowNamed`). A parity test compares every helper with direct Blitz access on every node of a fixture. Deliberately untouched (per scope): layout (`box`, `clientRects`, `scroll`), style blocks, form state, `outerHTML`/`text_content` serialisation, events, focus and paint.

**Remaining Blitz references outside `dom_model`** (same grep before and after: `get_node(`, `get_node_mut(`, `element_data()`, `NodeData::`, `.mutate()`, `blitz_dom::`, `blitz_html::` per line): **198 → 149** (`dom.rs` 117 → 58, `view.rs` 39 → 35, `document_host.rs` 5, `parser.rs` 8 → 13 because the oracle lives there). Round 5's figure of 514 used a different counting method and is not comparable; the same method was not re-run.

## 6. Crate extraction readiness (Task 5): deferred

Dependency graph before and after: **identical** (10 members, 18 internal edges, no cycles, 0 checker violations; `baseline/dependency-graph-after-round6.md`, `baseline/architecture-check-after-round6.md`; edges identical to `after-workers`). No manifest or lockfile changed.

| Candidate | Finding |
|---|---|
| `wega-dom` (the contract) | Production code is Blitz-free and depends only on `html5ever` and `url`. Real consumers exist today (`dom.rs`, `parser.rs`), but it can only be extracted *together with* a Blitz mapping. |
| `wega-adapter-blitz` | **Blocked by the orphan rule.** It must implement `wega-dom`'s traits for `blitz_dom::BaseDocument`, a foreign trait on a foreign type. The only fix is a newtype around `BaseDocument`, which touches ~500 uses of `Rc<RefCell<BaseDocument>>`. Alternatively the contract and the Blitz impl stay in one crate. |
| `wega-html` (the parser) | Depends on the contract (fine) but also on the engine's `diagnostics` module (`Diagnostic`/`Category`), and its tests need Blitz. Needs a diagnostics callback or trait first. |

Decision: no crate is forced into existence. Prerequisites for a later round: (1) decide newtype vs. co-location for the Blitz impl; (2) replace the parser's direct `crate::diagnostics` use with a small reporting interface; (3) move the remaining ~150 direct Blitz references (layout/style/form/events) behind the view, otherwise the engine still needs `blitz-dom` regardless.

## 7. Performance (Task 6)

Debug builds, baseline = Round 5 end (`057f03d`) built in an isolated worktree and target directory, current = this branch; three interleaved runs each, median of runs. **The machine was heavily loaded (load average ~16), and run-to-run spread of the *old* build alone is 10–40%, so differences below that are not meaningful.**

| Benchmark | Old | New | Change |
|---|---|---|---|
| evaluate_json(1+1) | 14.27 µs | 14.61 µs | +2.4% |
| dom ops (create+setAttribute+append+getAttribute) | 539 µs | 598 µs | +11.0% (old spread 520–578) |
| lookup + nodeType + nodeName + parentNode | 34.2 µs | 37.1 µs | +8.4% (old spread 33.5–39.9) |
| deep clone, 51 nodes | 366 µs | 398 µs | +8.7% (old spread 356–559) |
| appendChild + removeChild | 562 µs | 521 µs | −7.4% |
| setAttribute + removeAttribute | 77.6 µs | 72.0 µs | −7.1% |
| text node data update | 34.8 µs | 36.0 µs | +3.2% |
| innerHTML small fragment | 250 µs | 260 µs | +4.1% |
| innerHTML large fragment (900 nodes) | 60.6 ms | 59.3 ms | −2.2% |
| createElementNS | 88.9 µs | 73.8 µs | −17% (old spread 59–97) |
| cloneNode(false) | 150 µs | 139 µs | −7.9% |
| style set+remove | 121 µs | 97.5 µs | −19% |
| parse 9,000-node document | 530 ms | 538 ms | +1.6% |
| create_view + set_html | 237 ms | 248 ms | +4.5% |

Reading: parsing, fragment parsing and lifecycle are within noise. `dom ops`, lookups and deep clone show +8–11% medians that sit at the edge of the baseline spread; the read-path migration adds a key conversion and a validated lookup per call, so a small real cost there is plausible. This run cannot confirm or exclude it. Re-measure on a quiet machine before relying on the figure (Round 5 measured +0.8–1.5% on a quiet machine).

## 8. Validation

- Full engines suite: **401 passed, 0 failed, 6 ignored** (379 baseline + 22 new; the ignored node-budget test became an enabled test).
- Broker mode: `security`, `forms`, `scripts`, `documents`, `history` (`--test-threads=1`), plus `lib`, `dom`, `dom_identity`, `pages`: all pass.
- Required-worker test (`inherited_broker::a_required_worker_runs_jit_storage_cookies_and_frames_through_grants -- --ignored`) with a content helper built by `wega-content-bundle`: passes.
- Root `cargo check --locked`, `cargo metadata --locked` for both workspaces, `cargo fmt --check`, workspace `clippy -D warnings`: pass. Architecture checker: 0 violations. No manifest or lockfile changes.
- **Not run:** Windows and Linux builds, MSVC Skia jobs, native-sandbox ignored tests, the tools workspace's own test suite (no tool code changed).

## 9. Commits (not pushed)

`57a195f` node-budget fix; `156b5f2` mutation migration (+`clone_node`, `remove_style_property`); `1359d6d` benchmarks; `f7c4a6b` generic fragment parsing; `132cdac` read-path migration; `b585410` benchmark fix; (this commit) report.

## 10. Remaining blockers and next steps

1. Orphan rule for a separate Blitz adapter crate (newtype vs. co-location).
2. Parser reporting is engine-owned (`diagnostics`).
3. ~150 direct Blitz references: layout, style blocks, form state, serialisation, events, focus, paint.
4. Overshoot of one token's work past the node budget is bounded but not zero.
5. Orphaned parser-discarded nodes (formatting elements removed by the tree builder) stay in Blitz's arena until the document drops; unchanged from before.
6. Re-run the micro-benchmarks on a quiet machine.
