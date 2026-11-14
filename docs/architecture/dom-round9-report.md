# Round 9 report: CSS and styling ownership

Scope: Wega-owned style interfaces over the existing pipeline, preserving behaviour; concrete bugs found on the way are fixed and listed separately. No Stylo rewrite, no CSS parser/cascade/layout change, no second style state. Local only, not pushed. The audit and design were committed before implementation: [`dom-round9-design.md`](./dom-round9-design.md) (the operation-by-operation ownership table, with files, state and gaps, lives there and is not repeated).

**Outcome:** the seven style host ops in the engine (`inlineCSS`, `inlineProperty`, `styleSet`, `computed`, `supports`, `supportsProperty`, `media`) no longer name Blitz or Stylo types; they go through `wega_dom::style`, implemented once by `BlitzDocument`. Four pre-existing bugs fixed with tests. One new patch to vendored Blitz. No new crate, no new unsafe, no cycle, no change to the JavaScript API.

## 1. Architecture: before and after

Before: Stylo owns semantics, Blitz owns all style state (`Stylist`, lock, sheets, declaration blocks, computed values), and the Wega engine reached into `ElementData::style_attribute`, `PropertyId`, `ParserContext`, `MediaList`, the Stylo lock guard and `resolved_style_value` directly from `dom.rs`.

After: same owners, same state, one narrow seam.

| Operation | Owner after | Through |
|---|---|---|
| Inline style read/write, `setProperty`/`removeProperty` | Blitz/Stylo state | `StyleAccess::inline_style/inline_property`, `StyleControl::set_inline_property` |
| `getComputedStyle` value | Blitz (`resolved_style_value`) | `StyleAccess::resolved_value` (engine still flushes style/layout first) |
| `CSS.supports` (both forms) | Stylo | `supports_condition`, `supports_declaration` |
| `matchMedia` | Stylo `MediaList` over Blitz's device | `StyleControl::media_matches` |
| Stylesheet discovery, insertion, removal, text changes | Blitz | unchanged: `<style>`/`<link>` mutations, `refresh_style_element`; tested, not re-wrapped |
| Selector matching, cascade, inheritance, invalidation, layout/paint dependencies | Stylo/Blitz | unchanged |

## 2. Decisions

- **Contract in `wega-dom` (`engines/dom/core/src/style.rs`), no `wega-style` crate.** Consumers are the engine and the Blitz adapter, exactly the two existing dependents; the contract needs `NodeKey`, `DocumentView`, `MutationError`. A new crate would add a manifest and two edges and isolate nothing.
- **No `wega-adapter-stylo`.** Stylo is not separable from Blitz: the `Stylist`, lock, device and sheets are fields of `BaseDocument` and the cascade runs over Blitz's `TElement`. Implementation is in `wega-adapter-blitz`, which gained direct dependencies on `stylo`, `stylo_traits`, `cssparser` (already locked). **Limitation:** replacing Blitz also means re-hosting Stylo.
- **Not added, no consumer:** `StylesheetAccess` (CSSOM is not exposed to script), `StyleInvalidation` (Blitz invalidates inside each mutation), `ComputedStyleView` (callers need one property string at a time).
- `Mutator::remove_style_property` (Round 6) was removed from the contract; `set_inline_property` supersedes it.

## 3. Contracts

`StyleAccess: DocumentView` (`inline_style`, `inline_property`, `resolved_value`, `supports_condition`, `supports_declaration`) and `StyleControl: StyleAccess + DocumentMutation` (`set_inline_property`, `media_matches`). `NodeKey` only; stale keys and non-elements give `StyleError::{Stale, NotElement}` (`MutationError` for writes) instead of the former `unwrap()`. No Blitz/Stylo type in any signature. Reads append to a caller-provided `String` (inline reads serialize straight from the borrowed declaration block). No cache, no copied sheet or computed-style tree. `set_inline_property` is all-or-nothing and returns `Ok(false)` for an unknown property, an unparseable value, or an unchanged block.

## 4. Bugs fixed (pre-existing, each tested)

| # | Bug at `54b452f` | Fix |
|---|---|---|
| 1 | `setProperty('color','notacolor')` wrote `color: notacolor;` into the `style` attribute and queued a MutationObserver record | invalid declaration ignored (CSSOM); no mutation |
| 2 | The `style` attribute after `setProperty` was string-concatenated (`width: 5px;height: 7px;`) | canonical serialization |
| 3 | Computed `cssText`/`length`/`item` leaked the element's inline style | computed declarations answer empty |
| 4 | A `<style>` outside the document (never inserted, in a detached tree, or in a shadow root) applied its rules to the document | vendored patch `detached-style-elements.patch`; the element is processed on insertion |

Consequence to note: a value containing `!important` is now rejected (as in CSSOM); priority goes through the third argument. Writing the same value again is now a no-op (no mutation, no restyle).

## 5. Compatibility gaps (recorded, not fixed)

- `<style media>` is ignored (`MediaList::empty()` in Blitz's `make_stylesheet`); `HTMLStyleElement.disabled/.sheet/.media` absent.
- No CSSOM objects: `document.styleSheets`, `CSSStyleSheet`, `insertRule/deleteRule`, `adoptedStyleSheets` are `undefined`. Blitz has a 972-line backend (`cssom.rs`) nobody calls.
- Computed style ignores the pseudo-element argument.
- Author sheets are ordered by node id (creation order), not tree order: a script-created `<style>` inserted before existing sheets still cascades last (pinned by `known_gap_…`, correct value is documented in the test).
- `length`/`item`/`getPropertyPriority` in the JS proxy are still derived from `cssText` text (not changed this round).
- Layout flush before `getComputedStyle` stays an engine call through `Deref`; it moves with layout in Round 10.

## 6. Shadow DOM CSS audit

Shadow styling is **not implemented**; this is pinned, not claimed. A shadow root is a detached `#document-fragment`; its subtree gets no style, layout or paint. Tests (`engines/wega/tests/style.rs`) pin, for a host with a shadow `<style>` using `:host`, `:host-context(body)`, `::slotted(span)` and ids:

- shadow rules do not leak into the document (`shadow_styles_do_not_leak_into_the_document`);
- shadow elements have inline style but empty computed style and a 0×0 box; `:host`, `:host-context`, `::slotted` have no effect; light children render unflattened with document styles; `adoptedStyleSheets` is `undefined` on root and document; dynamic style insertion/inline writes inside a shadow tree change inline state only, still without leaking; the engine reports `"Shadow DOM scoped style and flattened layout are not implemented by Wega"`.

Missing: scoped selector matching, `:host`/`:host-context`/`::slotted`/`::part`, inheritance across the boundary into the shadow tree, shadow stylesheet ordering, adopted stylesheets, flattened-tree layout. Root cause is structural (the shadow subtree is outside the document tree Stylo traverses), so this is a feature, not a contract change.

## 7. Tests

Full engines workspace (`cargo test --locked --workspace`): **462 passed, 0 failed, 8 ignored** (Round 8: 433/0/7; +29 tests, +1 ignored benchmark). New: `wega-adapter-blitz/tests/style.rs` (8) and `wega-engine/tests/style.rs` (16), covering inline styles and invalid values, custom properties, `!important`, specificity/source order/inheritance, stylesheet insertion/removal/text change, computed style after DOM mutation, detached nodes, document replacement, invalid CSS recovery, `CSS.supports`/`matchMedia`, stale keys and the shadow behaviour above. Pre-existing bugs vs migration regressions: no migration regression was found; the four differences from `54b452f` are the intentional fixes in §4.

Broker mode (`WEGA_TEST_BROKER=1`, `--test-threads=1`): security 9, forms 4, scripts 8, documents 5, history 3, events 20, input 8, style 16, lib 27, dom 30, dom_identity 4, pages 16: all pass. Required-worker test (`a_required_worker_runs_jit_storage_cookies_and_frames_through_grants`, content helper from `wega-content-bundle`): passes. Also: `cargo fmt --check`; `clippy --locked --workspace --all-targets -D warnings` (engines workspace and tools workspace); `cargo check --locked` at the root; `cargo metadata --locked` for root, engines and tools; `scripts/check-engine-boundary.py`; architecture checker (0 violations, 14 informational) and its tests; `wega-dom` and `wega-adapter-blitz` boundary tests.

## 8. Performance (Round 8 `54b452f` vs Round 9 `d67317f`)

Method: release test binaries for both commits built from the same benchmark file (the Round 9 `hot_path_bench.rs`, which only uses the JS API), run interleaved r8,r9,r8,r9,r8,r9 ×3 each, median of 3 process runs (each already a median of 7). **Machine load average was 12–14 on 10 cores** (other processes running), so absolute numbers carry noise; the max column of a few runs shows load spikes (e.g. 3745 µs against a 1031 µs median). Allocations were not measured.

| Path | Round 8 | Round 9 | Δ |
|---|---|---|---|
| inline property read | 0.70 µs | 0.67 µs | −4% |
| `cssText` read | 1.63 | 1.64 | +1% |
| `setProperty` | 4.81 | 5.02 | +4% |
| set+removeProperty | 5.26 | 5.41 | +3% |
| `getComputedStyle`, no mutation | 0.65 | 0.68 | +5% |
| computed after inline write | 1081.6 | 1083.2 | 0% |
| computed after an unchanged write | 1031.2 | 25.5 | −98% (by design: no-op write, no restyle) |
| selector matching | 10.04 | 9.87 | −2% |
| stylesheet insert+query+remove | 1230.6 | 1247.8 | +1% |
| class toggle invalidation+query | 1117.4 | 1102.8 | −1% |
| restyle 300 elements + paint | 3.58 ms | 3.60 ms | +1% |
| styled document creation (300 items) | 78.7 ms | 78.8 ms | 0% |
| `CSS.supports`/`matchMedia` | 1.81 | 1.81 | 0% |

Real regression: the older `style.setProperty+removeProperty` pair in `hot_paths` is **+13%** (6.85 → 7.76 µs/pair); `setProperty` in the new bench is +4%. This is the cost of validating, applying and canonically serializing a declaration (previously unvalidated string concatenation); accepted as the price of fixes 1 and 2, not optimized. Everything else is within noise (≤ ±7%; the other Round 8 benchmarks, run for the same comparison, are within about ±3% except `createElementNS` −7%). `create_view+set_html` is +3%, within observed variance.

## 9. Dependency graph, references, lockfile

- Engines workspace: 13 members, 24 internal edges, no cycles, 0 violations (unchanged from Round 8). `wega-adapter-blitz` gained direct external dependencies `stylo`, `stylo_traits`, `cssparser`.
- `engines/Cargo.lock`: +3 lines (those three entries); root and tools lockfiles unchanged.
- Direct Blitz references, same grep as Rounds 6–8 (`get_node(`, `get_node_mut(`, `element_data()`, `NodeData::`, `.mutate()`, `blitz_dom::`, `blitz_html::` in engine `src/`): **96 → 93** (`dom.rs` 58 → 55; `view.rs` 31, `document_host.rs` 5, `network.rs` 1, `script.rs` 1). Direct Stylo-type lines in `dom.rs`: 2 → 1.
- No new `unsafe` in the diff (grep over non-vendor changes). No generated bindings changed; `dom/semantics.js` is hand-written and its public API is unchanged.
- No second style state: every contract call reads Blitz/Stylo state; nothing is cached on the Wega side.

## 10. Not executed, blockers

- Only macOS (Apple M4) was run. Linux and Windows builds, and the native-sandbox tests (Linux seccomp/namespaces, Windows AppContainer/job objects), were not executed; they are unaffected by this change but unverified here.
- Benchmarks ran under heavy load (see §8); allocation counts not measured.
- The vendored Blitz patch is a local patch: it must be re-applied on a Blitz update (listed in `engines/vendor/README.md` and `sources.json`).
- Disk was at 99% after release builds (7.6 GB free); no artifacts outside this project's `engines/target` were created.

## 11. Commits

`90aa968` audit and design, `d67317f` implementation, tests and benchmark ("step 9"), plus the report commit.

Next (Round 10, not started): layout geometry contract (including the pre-`getComputedStyle` flush), then CSSOM exposure and Shadow DOM styling as features.
