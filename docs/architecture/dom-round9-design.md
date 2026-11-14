# Round 9 design: CSS and styling ownership (audit and decision)

Committed before implementation. Scope: Wega-owned style interfaces for the operations the engine actually performs; no Stylo rewrite, no second style state, no change to the rendering pipeline. Branch `feature/dom-rounds-9-12` (created from `main` at `54b452f`; `feature/browser-cache-accessibility` named in the task text is already merged into `main` by PR #63).

## 0. Baseline (re-established, not assumed)

- HEAD `54b452f`, working tree clean, 33 GiB free.
- Engines workspace: 13 members, 24 internal edges, 0 checker violations (same as Round 8).
- Direct Blitz-reference lines in engine `src/` (same grep as Rounds 6–8): **96** (`dom.rs` 58, `view.rs` 31, `document_host.rs` 5, `network.rs` 1, `script.rs` 1).
- Test counts: recorded in the Round 9 report from the full-suite run started at this HEAD.
- Platform: macOS (Apple M4, 10 cores, 16 GB) only.

## 1. Who owns the CSS pipeline today

"Blitz" below is the vendored, patched `engines/vendor/blitz/blitz-dom`; "Stylo" is `stylo 0.22.0` (vendored, patched). Wega engine paths are under `engines/wega/src`.

| # | Operation | Owner | Where | State | Tests before this round | Gaps found (probe at `54b452f`) |
|---|---|---|---|---|---|---|
| 1 | Inline `style` attribute: parse, read, mutate | Parse/storage: Blitz + Stylo (`node/element.rs:671 flush_style_attribute`, `PropertyDeclarationBlock` in `ElementData::style_attribute`). Read/write glue: **Wega engine reaching into Blitz and Stylo types** (`dom.rs:1278-1285` `inlineCSS`, `inlineProperty`, `styleSet`) | one block per element, in the Blitz node; the `style` attribute text is a second representation kept in step by Blitz on attribute writes | incidental (`css.rs` transition test, `dom.rs`) | `setProperty('color','notacolor')` writes `color: notacolor;` into the attribute (CSSOM: ignore); attribute text is built by string concatenation (`width: 5px;height: 7px;`), so it is not the canonical serialization; a rejected value still queues a MutationObserver record |
| 2 | `CSSStyleDeclaration` bindings | Wega JS (`dom/semantics.js:1156-1234`): a `Proxy` over host ops; `length`/`item` split `cssText` on `;`; `getPropertyPriority` is a regex over `cssText` | none in JS (no cache) | none dedicated | computed declarations answer `cssText`/`length`/`item` from the **inline** style; `getComputedStyle(el, '::before')` ignores the pseudo-element argument |
| 3 | Stylesheet discovery (`<style>`, `<link rel=stylesheet>`) | Blitz (`mutator.rs:1086 load_linked_stylesheet`, `:1136 unload_stylesheet`, `document.rs:1157 process_style_element`); the parser's end-of-`<style>` hook goes through `wega_dom::DocumentMutation::refresh_style_element` | `BaseDocument::nodes_to_stylesheet` (owner node → sheet) | `css.rs`, `pages.rs` (indirect) | the `media` attribute of `<style>` is ignored: `<style media=print>` applies on screen (`MediaList::empty()` in `make_stylesheet`); `HTMLStyleElement.disabled`, `.sheet`, `.media` do not exist |
| 4 | CSS parsing, stylesheet ownership | Stylo parses; Blitz owns the `Stylist`, the `SharedRwLock` and every sheet | `BaseDocument::{stylist, guard, nodes_to_stylesheet}` | `css_wpt.rs` (pinned subset) | none found; invalid CSS recovery verified by probe |
| 5 | Selector matching, specificity | Stylo (`selectors`), over Blitz nodes (`blitz-dom/src/stylo.rs` implements `TElement`/`TNode`) | — | `css_wpt.rs`, `dom.rs` (`querySelector`) | not audited beyond the probe |
| 6 | Cascade, inheritance, computed values | Stylo traversal driven by Blitz (`stylo.rs:64 resolve_stylist`, `resolve.rs:40 resolve`) | `ComputedValues` in each node's Stylo `ElementData` | `css.rs` reftests | — |
| 7 | `getComputedStyle` | Value: Blitz (`resolved_style.rs:343 resolved_style_value`, used values from layout). Flush: Wega (`dom.rs:190 resolve`, `:1305 computed`) | none in Wega; the engine's `layout_dirty` flag decides whether to flush | `css.rs` (indirect), `compat.rs` | no pseudo-elements; empty for detached nodes (spec) |
| 8 | `CSS.supports` | Blitz/Stylo (`resolved_style.rs:294 css_supports_condition`); the two-argument form is a Wega hack: `style_attr_set_property("", name, value, false)` non-empty (`dom.rs:1277`) | — | none dedicated | — |
| 9 | CSSOM stylesheets and rules | **Not exposed.** Blitz carries a full backend (`blitz-dom/src/cssom.rs`, 972 lines: rule lists, `insertRule`, `deleteRule`, disabled sheets) that no Wega code calls | — | none | `document.styleSheets`, `CSSStyleSheet`, `sheet`, `adoptedStyleSheets` are all `undefined` |
| 10 | Style invalidation after DOM changes | Blitz: restyle hints and snapshots set inside each mutator operation (`mutator.rs:275…715`, `document.rs:772/784/1460`). Wega only bumps its own `revision`/`layout_dirty` (`dom.rs:186`) so the next read or paint flushes | hints/damage in Blitz nodes; one dirty flag in the engine | indirect | none found by probe (class, attribute, sheet insert/remove/text change all re-cascade) |
| 11 | Shadow DOM styling | **Not implemented.** A shadow root is a detached `#document-fragment` element (`dom.rs:1132 shadowAttach`); its subtree is not in the document, so it gets no style, layout or paint; light-DOM children render unflattened. The engine reports this (`"Shadow DOM scoped style and flattened layout are not implemented by Wega"`) | shadow metadata in Wega `components.rs` | events only (`events.rs`) | see §4 |
| 12 | Style → layout, style → paint | Blitz (`resolve.rs`: style, then layout construction/damage, then Taffy; `blitz-paint` reads `primary_styles()`); one engine read: iframe visibility (`view.rs:2354`) | — | reftests | — |

Other engine-side Stylo use: `matchMedia` builds a Stylo `ParserContext` and `MediaList` in `dom.rs:1186-1191`; `setChecked` passes `style_dom::ElementState::CHECKED` (form state, not this round); `view.rs` sets the style threading mode before `resolve`.

So: **Stylo owns the semantics, Blitz owns all style state, and Wega owns only the JS surface plus seven host ops that currently name Blitz and Stylo types directly.**

## 2. Decision

### Contract: `wega_dom::style`, implemented by `BlitzDocument`

Two small traits, shaped by the existing consumers only (the host ops above):

```rust
pub trait StyleAccess: DocumentView {
    /// Appends the element's inline declaration block, canonically serialized.
    fn inline_style(&self, key: NodeKey, out: &mut String) -> Result<(), StyleError>;
    /// Appends one inline property value (shorthands included); nothing if absent/unknown.
    fn inline_property(&self, key: NodeKey, property: &str, out: &mut String) -> Result<(), StyleError>;
    /// Appends the resolved (getComputedStyle) value. The caller flushes style/layout first.
    fn resolved_value(&self, key: NodeKey, property: &str, out: &mut String) -> Result<(), StyleError>;
    fn supports_condition(&self, condition: &str) -> bool;
    fn supports_declaration(&self, property: &str, value: &str) -> bool;
}
pub trait StyleControl: StyleAccess + DocumentMutation {
    /// CSSOM setProperty/removeProperty on the inline style. `Ok(false)`: nothing changed
    /// (invalid declaration, unknown property, or the same block).
    fn set_inline_property(&mut self, key: NodeKey, property: &str, value: &str, important: bool)
        -> Result<bool, MutationError>;
    /// Evaluates a media query list against the document's device.
    fn media_matches(&mut self, query: &str) -> bool;
}
```

- `NodeKey` only; stale keys and non-elements are rejected (`StyleError::{Stale, NotElement}`, `MutationError` for writes) instead of the current `unwrap()`.
- No Blitz or Stylo type in the signatures. Reads append to a caller-provided `String`: inline reads serialize straight from the borrowed declaration block (no intermediate allocation); `resolved_value` forwards Blitz's `String` (one allocation, as today).
- No cache, no copied sheet or computed-style tree: every call reads the Blitz/Stylo state.
- `media_matches` takes `&mut` only because Blitz flushes pending device changes lazily.
- `Mutator::remove_style_property` (Round 6) is superseded by `set_inline_property` and is removed from the contract rather than left as a second way to do the same thing.
- **Not added** (no consumer exists): `StylesheetAccess` (CSSOM is not exposed to script), `StyleInvalidation` (Blitz invalidates inside each mutation; the engine has nothing to tell it), a `ComputedStyleView` handle (the only consumer asks for one property string at a time). Stylesheet insertion/removal already runs through the mutation contract (`<style>`/`<link>` insertion, `refresh_style_element`); that path is tested, not re-wrapped.
- The layout flush before `getComputedStyle` stays the engine's `resolve` (through `Deref`) this round; it is a layout concern and moves in Round 10.

### Crate placement

- **No `wega-style` crate.** The consumers are the engine and the Blitz adapter, exactly the two dependents `wega-dom` already has; the contract needs `NodeKey`, `DocumentView` and `MutationError` from `wega-dom`. A new crate would add a manifest and two edges and isolate nothing.
- **No `wega-adapter-stylo`.** Stylo is not separable from Blitz here: the `Stylist`, the lock, the device and all sheets are fields of `BaseDocument`, and the cascade runs over Blitz nodes through Blitz's `TElement` implementation. An adapter over Stylo alone would have nothing to adapt. The implementation therefore lives in `wega-adapter-blitz`, which gains direct dependencies on `stylo`, `stylo_traits` and `cssparser` (already locked; needed for `PropertyId` and media-query parsing that the engine does today). **Limitation recorded:** replacing Blitz also means re-hosting Stylo.

### Vertical slice to migrate

`inlineCSS`, `inlineProperty`, `styleSet`, `computed`, `supports`, `supportsProperty`, `media` host ops in `dom.rs`; the JS `CSSStyleDeclaration` keeps its API.

### Bugs to fix on the way (pre-existing; each gets a test)

1. `setProperty`/property assignment with an invalid value corrupts the `style` attribute and reports a mutation.
2. The attribute written by `setProperty` is not the canonical serialization.
3. Computed `cssText`/`length`/`item` leak the inline style.

Recorded, not fixed (need vendor or feature work): `<style media>` ignored; no CSSOM; no pseudo-element computed style; Shadow DOM styling.

## 3. Risks

- `set_inline_property` changes the attribute text to the canonical serialization; tests that pin the concatenated form will show it. Treated as fix #2.
- Values carrying `!important` inside the value string are now rejected (CSSOM); the priority argument is the supported route.

## 4. Shadow DOM CSS (audit; details and tests in the report)

Probe at `54b452f`: an element inside a shadow root has an empty computed style and a zero-size box; `:host`, `::slotted` and shadow `<style>` rules have no effect; a shadow `<style>` does not leak into the document; light children of a host keep rendering with document styles; `adoptedStyleSheets` does not exist. This round pins that behaviour with tests and lists the missing semantics; it does not implement Shadow DOM styling.
