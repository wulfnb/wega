# Blitz retirement readiness (Round 12 inventory)

Purpose: an exact, evidence-based inventory of what still depends on Blitz and the other vendored engines, and what replacing each would take. This is a readiness map, **not a plan to remove Blitz now** (see `post-round12-roadmap.md`: missing browser features come first).

Method. Three different measures, because none alone shows independence:

1. **Direct imports** in `wega-engine` `src/`: the grep used since Round 6 (`get_node(`, `get_node_mut(`, `element_data()`, `NodeData::`, `.mutate()`, `blitz_dom::`, `blitz_html::`) = **81 lines** at this commit (Round 6: 149; Round 11: 81; unchanged in Round 12), plus named Blitz types (`blitz_traits::…`, `NodeId`).
2. **Reliance through `Deref`.** `BlitzDocument` still dereferences to Blitz's `BaseDocument`. To measure it exactly, `Deref`/`DerefMut` were removed temporarily and the engine compiled: **149 errors in four files (`dom.rs` 72, `view.rs` 56, `document_host.rs` 12, `dom_host.rs` 9), over 26 distinct `BaseDocument` members** (table below). The change was reverted; `Deref` stays. This is the real remaining coupling, and it is larger than the grep suggests.
3. **Indirect reliance**: types and behaviour that cross the boundary without a method call: Blitz's `NodeId` (used by the engine in 78 places: `dom.rs` 42, `view.rs` 27, `document_host.rs` 7, `script.rs` 2), Blitz's event types in the public engine API, Blitz's `NetProvider`/`NavigationProvider` traits implemented by the engine, and the vendored Blitz sources themselves (patches below).

A low number in measure 1 is not evidence of independence: the contracts are exercised and tested, but each is implemented by exactly one backend (`BlitzDocument`), so none has been proven against a second implementation.

## What exists

| Item | Size / state |
|---|---|
| Vendored `blitz-dom` | 29,334 lines; 5 Wega patches (`blitz-dom.patch`, `content-image-budgets`, `document-id-scope`, `detached-style-elements` (R9), `own-scroll-bounding-rect` (R10)) |
| Vendored `blitz-paint` | 6,877 lines; 1 patch |
| Vendored `stylo` 0.22 | 1 patch; the `Stylist`, lock, device and all sheets are fields of `BaseDocument` (Stylo is not separable from Blitz today) |
| Vendored `anyrender_skia`, `skia-bindings` | 1 patch each |
| Registry crates Blitz brings | `blitz-html`, `blitz-traits`, `stylo_taffy`, `taffy` 0.15 (layout), `parley` 0.12 + `fontique`/`harfrust` (text), `selectors`, `html5ever`/`xml5ever`/`markup5ever`, `accesskit` 0.25, `anyrender` 0.14, `kurbo`, `peniko`, `usvg`/`image` |
| Direct engine dependencies on the stack | `blitz-dom` (features: system-fonts, svg, woff, autofocus, scrollbars, floats, error-diagnostics, accessibility, file-input), `blitz-traits`, `style` (stylo), `selectors`, `style_dom`, `accesskit`, `html5ever`, `keyboard-types`, `skia-safe` (canvas/WebGL). Removed this round (unused): `blitz-html`, `cssparser`, `style_traits`, `cookie_store`, `publicsuffix`. |
| Duplicate versions in the lockfile | `cssparser` 0.36 (via `deno_web`, V8 side) and 0.38 (Stylo side); `selectors` 0.41 (Blitz/Stylo) and 0.42 (engine, adapter); `servo_arc` 0.5/0.6. Cosmetic until Stylo moves; the engine's `selectors = 0.42` is not the version Blitz uses. |

## Wega contracts (all in `wega-dom` unless noted)

| Contract | Round | Implemented by | Exercised by |
|---|---|---|---|
| `DocumentView`/`NodeView` (read) | 6–7 | `BlitzDocument` | engine DOM paths, `dom.rs` tests, adapter parity tests |
| `DocumentMutation`/`Mutator` | 6–7 | `BlitzMutator` | parser (`wega-html`), engine DOM ops |
| HTML tree building (`wega-html`, own crate) | 7 | html5ever `TreeSink` over `Mutator` | engine `set_html`, fragments |
| `StyleAccess`/`StyleControl` | 9 | `BlitzDocument` | engine host ops, 24 style tests |
| `FocusState`/`InputControl` | 8 | `BlitzDocument` | input/events tests |
| `GeometryAccess`/`HitTest`/`LayoutControl` | 10 | `BlitzDocument` | engine host ops, 23 layout tests |
| `PaintSource` | 11 | `BlitzDocument` | `View::paint`, 17 render tests, 42-frame pixel parity |
| `DocumentSetup` + constructors (`wega-adapter-blitz`) | 8 | adapter | lifecycle tests |

## Inventory by domain

Columns: **Where** (source) · **Consumer** · **Why Blitz is still required** · **Contract** (exists / exercised) · **Replace** (difficulty) · **Tests to carry over** · **Perf risk**.

### DOM
- **Where.** `Deref` calls: `get_node` (39 sites in `dom.rs`/`view.rs`/`document_host.rs`), `get_node_mut` (5), `create_node` (1), `root_node`/`root_element`/`try_root_element` (5), `get_element_by_id` (1), `snapshot_node_and` (1). `TreeTraverser` (`dom.rs` ×4, `view.rs` node count). `NodeData::` matches in `dom.rs` (text/comment/document reads and writes), `NodeData`/`SpecialElementData` in `view.rs`.
- **Consumer.** The JS DOM host ops: `outerHTML`, `text`, `setText` budget accounting, `attr`, `clone` budget, `inputValue`, `checked`, `node count`.
- **Why.** Node payloads (text, comment, element data) and serialisation (`outer_html`, `text_content`) are only reachable on Blitz's `Node`.
- **Contract.** `DocumentView`/`NodeView` exist and are exercised for names, traversal, attributes, liveness. **Missing:** node data (text/comment content read/write), serialisation, `TreeTraverser`-style subtree iteration, node creation outside the mutator.
- **Replace.** Medium. Mostly adding `NodeView::{text, outer_html}` and a text-write op to the mutator, then deleting `NodeData` use. Identity (`NodeId` → `NodeKey` in the engine's own signatures) is a mechanical but wide edit (78 sites).
- **Tests.** `dom.rs` (30), `dom_identity.rs` (4), WPT DOM subset (35 files, 1685 subtests), adapter parity tests.
- **Perf risk.** Low (borrowed reads); serialisation cost is proportional to size either way.

### HTML
- **Where.** `wega-html` (html5ever `TreeSink` into `Mutator`) is the parser for incremental parsing and fragments. `BlitzDocument::from_html` (initial blank/static documents) calls `blitz_html::HtmlDocument` inside the adapter only; the engine has no `blitz-html` dependency any more.
- **Why.** Whole-document one-pass parsing for the initial page still goes through Blitz's HTML wrapper; script/style hooks are parser callbacks into Blitz.
- **Contract.** Exists and exercised (the parser crate is Wega's, with a node budget and fragment-context support).
- **Replace.** Low–medium: route `from_html` through `wega-html` as well. Difficulty is parity of parser-end hooks (`<style>` refresh), already behind `DocumentMutation`.
- **Tests.** Parser parity (23 fragments × 17 contexts, Round 6), `pages.rs`, budget tests.
- **Perf risk.** Low (9000-node parse benchmark ≈ 70 ms release).

### CSS / style
- **Where.** Contract methods implemented in `wega-adapter-blitz` over `BaseDocument::stylist`. Direct: `style_dom::ElementState::CHECKED` (`dom.rs:1292`), `primary_styles().clone_visibility()` (`view.rs`, iframe visibility check), `selectors` and `style` dependencies of the engine crate.
- **Consumer.** Inline style, computed style, `CSS.supports`, `matchMedia`, `:checked` snapshots, selector queries.
- **Why.** Stylo is hosted by Blitz: the `Stylist`, lock, device, sheets and the cascade traversal over Blitz nodes (`TElement` implemented in `blitz-dom/src/stylo.rs`). Replacing Blitz means re-hosting Stylo.
- **Contract.** Exists and exercised (Round 9). **Missing:** selector matching/query (`query_selector*`, `matches_selector`, `closest`: 11 `Deref` sites), element-state changes, stylesheet/CSSOM objects (not exposed to script at all).
- **Replace.** High. A Stylo-hosting adapter (`wega-adapter-stylo`) is not independent of the DOM it traverses; the realistic path is to keep Stylo and replace its host.
- **Tests.** `style.rs` (16), adapter `style.rs` (8), CSS WPT (7), `css.rs` reftests (21).
- **Perf risk.** High if the cascade host changes (restyle of 300 elements + paint ≈ 3.6 ms; computed-after-write ≈ 1.08 ms/query are dominated by the traversal).

### Layout
- **Where.** `flush_layout`/`prepare_frame` in the adapter call `BaseDocument::resolve`: style → box construction → Taffy block/flex/grid → Parley inline text. `view.rs` still calls `scroll_node_by` (2 sites).
- **Consumer.** Every geometry read, hit-test, paint.
- **Why.** Layout algorithm, text shaping, line breaking and the scroll model live in Blitz/Taffy/Parley.
- **Contract.** `GeometryAccess`/`HitTest`/`LayoutControl` (Round 10), exercised; it **hides** the engine, it does not abstract the algorithm: results are Blitz's.
- **Replace.** Very high (an entire layout engine). Known defects of the current one: transforms ignored by geometry, **a hang for ≥122 nested blocks** (see the report), a quadratic JS traversal.
- **Tests.** `layout.rs` (15), adapter `layout.rs` (8), `css.rs` reftests, `reliability.rs`.
- **Perf risk.** Very high; layout dominates document load (≈ 77 ms for 300 cells + paint).

### Geometry
- Contract complete for current consumers (rects, client/offset/scroll boxes, viewport, hit-test). Remaining Blitz knowledge: the 1/64 px snapping and inline-fragment rules (behaviour, not API). Replace with layout.

### Events and default actions
- **Where.** `handle_dom_event` (5 sites) drives text editing, selection, click synthesis; `active_node`/`set_mousedown_node_id`/`unactive_node`, `get_cursor`; Blitz event types `BlitzPointerEvent`, `BlitzKeyEvent`, `BlitzImeEvent`, `DomEvent*` in `view.rs`.
- **Consumer.** `View::pointer/key/ime/wheel`, default actions after script dispatch.
- **Why.** Blitz owns the editing and selection model and the default action sequence; dispatch itself is deno_web's (not Blitz).
- **Contract.** Focus/hover (Round 8) exists and is exercised. **Missing:** default-action/click-synthesis contract, text editing/selection, cursor, and **Wega-owned input event types**: `View::key(BlitzKeyEvent)`, `View::ime(BlitzImeEvent)` and `pub use blitz_traits::events` are Blitz types in the engine's public API.
- **Replace.** High (editing/selection model). The type leak is medium and mechanical.
- **Tests.** `events.rs` (20), `input.rs` (8), forms tests.
- **Perf risk.** Low–medium (native click ≈ 1.7 ms, dominated by hit-test and restyle).

### Forms
- **Where.** `submit_form` (3 `Deref` sites), `BaseDocument::toggle_radio`, `SpecialElementData::FileInput`, `element_data().checkbox_input_checked()`, input value reads, `form_entries` built from Blitz element data in the engine.
- **Why.** Form control state (values, checked, file lists, selection) is stored in Blitz nodes.
- **Contract.** **None.** Form state is the largest uncovered area after layout.
- **Replace.** Medium–high; needs a form-state contract before anything else can move (input editing depends on it).
- **Tests.** `forms.rs` (4), `input.rs`, WPT subset (no forms files), framework fixtures.
- **Perf risk.** Low.

### Paint
- **Where.** `blitz_paint::paint_scene` inside `wega-adapter-blitz` (since Round 11); `anyrender::Scene` is the display list shared with Skia; the image cache (`release_image_cache`) and decoding are Blitz's.
- **Contract.** `PaintSource` exists and is exercised; Skia side is `wega-adapter-skia`.
- **Replace.** High for the painter; the rasteriser is already behind a typed boundary. The engine has **0** `blitz_paint`/`anyrender` references.
- **Tests.** `render.rs` (17), pixel-parity corpus (42 frames), reftests.
- **Perf risk.** Medium (record ≈ 0.09 ms, rasterise ≈ 0.24 ms at 800×600 on a light page).

### XML
- **Where.** `BlitzDocument::from_xml` (`blitz_html::HtmlDocument::from_xml`, xml5ever), selected by `view.rs` for XML responses.
- **Contract.** None (`wega-html` handles HTML only).
- **Replace.** Medium: an xml5ever `TreeSink` over `Mutator` like the HTML one. Known gap: WPT files needing XML/XHTML documents are excluded from the pinned subset.
- **Tests.** Few; add XML parity tests before moving.
- **Perf risk.** Low.

### Document lifecycle
- **Where.** `url()` (**49** `Deref` sites, the single largest group), `set_base_url` (2), `handle_messages` (resource-load pump), `release_image_cache`; `DocumentSetup` + constructors are behind the adapter (Round 8). `view.rs` implements Blitz's `NetProvider` (`Resources`) and `NavigationProvider` (`Navigation`) so Blitz drives subresource loading and link navigation through Wega's network stack; `install_broker_fonts` (`network.rs`) installs fonts into Blitz.
- **Why.** Subresources (images, stylesheets, fonts) are requested by Blitz itself and returned through its provider traits.
- **Contract.** Construction only. **Missing:** document URL/base-URL access (trivial: one `DocumentView` method would remove ~51 sites), a resource-delivery interface, font installation.
- **Replace.** Low for URL; medium–high for the resource pipeline (it is where CSP/CORS/broker policy meets the document).
- **Tests.** `documents.rs`, `pages.rs`, `security.rs`, `image_limits.rs`.
- **Perf risk.** Low.

### Accessibility
- **Where.** `build_accessibility_tree` (2 sites in `view.rs`) returns an `accesskit::TreeUpdate` built by the vendored `blitz-dom/src/accessibility.rs`, published as `PageEvent::Accessibility`.
- **Contract.** None. No engine-level test of the page tree or ARIA mapping exists.
- **Replace.** Medium (a tree walker over `NodeView` + computed style + ARIA rules) but **untested today**, so behaviour parity cannot be demonstrated without writing tests first.
- **Perf risk.** Low (built per frame only when accessibility is enabled).

## Reliance through `Deref`, exact (149 compile errors, 26 members)

| Group | Members (errors) |
|---|---|
| Lifecycle | `url` (49), `set_base_url` (2), `handle_messages` (1), `release_image_cache` (1) |
| DOM nodes | `get_node` (39), `get_node_mut` (5), `root_node` (3), `create_node` (1), `get_element_by_id` (1), `try_root_element` (1), `root_element` (1), `snapshot_node_and` (1) |
| Selectors | `query_selector` (6), `query_selector_all` (5), `query_selector_in` (1), `query_selector_all_in` (1), `matches_selector` (1), `closest` (1) |
| Events / default actions | `handle_dom_event` (5), `scroll_node_by` (2), `active_node` (1), `set_mousedown_node_id` (1), `unactive_node` (1), `get_cursor` (1) |
| Forms | `submit_form` (3) |
| Accessibility | `build_accessibility_tree` (2) |
| Type mismatches (functions that take `&BaseDocument`) | 8 |

Order by effort-to-benefit for removing `Deref`: URL access (−51), node data + traversal (−55), selectors (−15), accessibility (−2), forms (−3 plus node data), events/default actions (the rest, hardest).

## Readiness summary

| Domain | Contract exists | Exercised | Independent backend proven | Replacement difficulty |
|---|---|---|---|---|
| DOM | partial | yes | no | medium |
| HTML | yes | yes | partly (parser is Wega's; hosted by Blitz) | low–medium |
| CSS/style | yes (no selectors) | yes | no (Stylo hosted by Blitz) | high |
| Layout | yes | yes | no | very high |
| Geometry | yes | yes | no | follows layout |
| Events/default actions | focus/hover only | yes | no | high |
| Forms | no | n/a | no | medium–high |
| Paint | yes | yes | no | high (painter), low (rasteriser) |
| XML | no | n/a | no | medium |
| Lifecycle | construction only | yes | no | low (URL) to high (resources) |
| Accessibility | no | n/a | no | medium, untested |

Conclusion: the DOM, HTML, style, geometry, paint and focus boundaries are real and exercised, so the engine can no longer be *accidentally* coupled in those areas. Blitz cannot be retired yet: forms, selectors, default actions, accessibility, XML and the resource pipeline have no contract; layout and Stylo hosting have no alternative; and the 149-error `Deref` surface plus the `NodeId` identity type and Blitz event types in the public API still bind the engine to Blitz's types.
