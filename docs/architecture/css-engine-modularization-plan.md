# Wega CSS engine: modularization and Blitz/Stylo-referenced port

Status: IN PROGRESS. Phase 0 is implemented (2026-11-21). Phases 0–2 are done. Phase 3 step 1 (original stylesheet text) is re-landed; steps 2–3 are open.

## Why

The native engine (`engines/native`, crate `wega-native`) became the only renderer in
`71938eb`. Real sites exposed a long tail of CSS defects:
- Rules were dropped by one unknown pseudo-class.
- `@layer`, `@supports` and `@property` were ignored.
- Color spaces, logical properties, range media queries and units were missing.
- `border-color` and `border-width` shorthands were silently ignored.
- `z-index: calc()` was ignored.

Most were fixed one at a time while debugging Google, Hacker News, Wikipedia and
edudia.in. The causes are structural, and they will keep producing bugs:

1. **One crate does everything.** `css.rs` (3,973 lines) and `syntax.rs` (1,425 lines)
   mix values, parsing, selectors, cascade, the UA sheet and property application.
   `layout.rs` is 2,873 lines. This contradicts `module-map.md`, which already plans
   independent `css/*`, `layout/*` and `graphics/*` crates.
2. **Declarations stay strings until the cascade.** `apply_declaration` re-parses every
   value when it is applied, and on failure drops it with no diagnostic. Shorthands are
   half-expanded. Stylo's model avoids this whole class of bugs: it parses every
   property into a typed specified value at parse time and expands shorthands into
   longhands.
3. **The CSS source is a Stylo round-trip.** `native_stylesheet_text` re-serializes
   Stylo's CSSOM, so anything Stylo rejects (for example `:has()`) never reaches Wega.
   Stylo's serialization quirks also leak in: `color-mix()` loses `in oklab`, and
   shorthands are merged.
4. **No conformance gate.** The "real-world" tests use hand-written fixtures. Nothing
   measures Wega against WPT the way Blitz does (`blitz/wpt/runner`).

Decision (owner): keep a Wega-owned CSS engine. Use Stylo (`engines/vendor/blitz/stylo`,
MPL-2.0, the same license as Wega) and Blitz (`/home/wolverine/my/blitz`, MIT/Apache-2.0)
as the behavioral and structural reference. Do not use Stylo for style resolution.

## Target crates (per `module-map.md`)

| Crate (path) | Owns | Ported from / reference |
|---|---|---|
| `wega-css-values` (`css/values`) | Typed values: length, percentage, `calc()`/`min()`/`max()`/`clamp()`, number, angle, color (all color spaces, `color-mix`), image (url, gradients), shadow, transform, grid track. Specified-to-computed conversion. | `stylo/values/{specified,computed}/*`, `cssparser-color` |
| `wega-css-parser` (`css/parser`) | Tokenizer glue, stylesheet, rule and at-rule structures (`@media`, `@layer`, `@supports`, `@property`, `@font-face`, `@import`, `@container`, nesting). **Property table:** one typed `parse` per longhand, shorthand to longhand expansion, CSS-wide keywords. | `stylo/properties/*` (longhand/shorthand definitions), `stylo/stylesheets/*`, `stylo/media_queries`, `stylo/queries` |
| `wega-css-selectors` (`css/selectors`) | Selector AST, parser, specificity and matching through an `ElementView` trait (no DOM dependency). Includes pseudo-elements and `:has` with relative selectors. | `selectors` crate (`parser.rs`, `matching.rs`, `bloom.rs`) |
| `wega-css-style` (`css/style-engine`) | Cascade (origins, layers, importance, scoping), UA stylesheet, presentational hints, custom properties and `var()`, `@property` registrations, inheritance, `ComputedStyle` built from per-family structs. | `stylo/rule_tree`, `stylo/applicable_declarations.rs`, `stylo/custom_properties.rs`. Blitz: `blitz-dom/src/stylo/{presentational_hints.rs,user_agent.rs}` and `assets/default.css` |
| `wega-css-cache` (`css/cache`) | Rule hashing by id, class and tag (selector map), descendant bloom filter, style sharing. | `stylo/selector_map.rs`, `stylo/sharing/*`, `selectors/bloom.rs` |
| `wega-css-invalidation` (`css/invalidation`) | Restyle hints for DOM and attribute changes and stylesheet changes. | `stylo/invalidation/*` |
| `wega-layout-*` (`layout/{core,block,inline,flex,grid,text}`) | Layout from **plain input types** (`LayoutStyle`), split by formatting context: block and floats, inline and text, flex, grid, table, intrinsic sizing. | Taffy algorithms (block, flex, grid), Blitz `blitz-dom/src/layout/*` (inline, table, replaced) |
| `wega-display-list` (`graphics/display-list`) | Display items, stacking contexts, painting order. | `blitz-paint` |
| `wega-adapter-style-layout` (`adapters/`) | `ComputedStyle` to `LayoutStyle` conversion. This is the only crate that depends on both. | `stylo_taffy/convert.rs` |
| `wega-adapter-dom-css` (`adapters/`) | `ElementView` implemented for `WegaDocument` (state, attributes, siblings). | `blitz-dom/src/stylo/dom.rs`, `wega/src/document/selectors.rs` |

### Rules
- Contract crates (`css/*`, `layout/*`, `graphics/display-list`) depend only on `std`,
  `cssparser`, `parley`, `kurbo` and `peniko`, plus lower crates in their own group.
  Integration happens only in `adapters/*`.
- **Size budgets:** source files of 600 lines or fewer, and crates of about 3k lines or
  fewer. Property handling is split by family (`box`, `border`, `background`, `text`,
  `font`, `flex`, `grid`, `position`, `effects`), one module each.
- **No silent drops.** A declaration that fails to parse reports a CSS diagnostic
  (property, value, location) through the existing `WEGA_DIAGNOSTIC` channel.
- Every ported module names its reference source in its module doc comment, and
  `engines/vendor/sources.json` / `NOTICE` record what was ported (Stylo is MPL-2.0,
  Blitz is MIT/Apache-2.0).

## Phases

Each phase ends with:
- `cargo test` passing for the native suites.
- No drop in the WPT pass rate.
- Google (legacy page), Hacker News, Wikipedia and edudia.in rendering at least as well
  as before, compared against Chrome screenshots with `examples/render_url`.

### Phase 0: Conformance and reference gates (before any refactor)
1. Check out WPT at Blitz's pin (`blitz/wpt/WPT_COMMIT` = `92d37954…`) into
   `~/my/wpt`. This needs a git clone of web-platform-tests, about 1 GB.
2. Add `engines/tools/wpt-runner`, modeled on `blitz/wpt/runner`: reftests and
   testharness tests, run in-process through `wega_engine::View` (as `render_url`
   does). It records pass/fail per test to JSON.
3. Baseline these suites: `css/selectors`, `css/css-cascade`, `css/css-color`,
   `css/css-values`, `css/mediaqueries`, `css/css-conditional`,
   `css/css-variables`, `css/css-logical`, `css/css-flexbox`, `css/css-grid`,
   `css/CSS2/floats`, `css/CSS2/tables`, `css/css-backgrounds`.
4. Add a real-site corpus test (saved HTML and CSS snapshots of the four sites plus
   Chrome reference PNGs). Fix the Chrome path in `tests/support/native_visual.rs`
   (macOS only today) so the comparisons also run on Linux.

**Phase 0 status (2026-11-21):**
- WPT: a sparse checkout of about 140 MB at `~/my/wpt`, pinned to Blitz's commit.
- Runner: `engines/tests/compatibility/wpt-runner` (crate `wega-wpt-runner`). It runs child
  processes with crash and hang isolation and gates on `baseline.json`.
- Site corpus: `wega/tests/native_site_corpus.rs` with `fixtures/site_corpus/`. Frozen snapshots
  of Google (the consent page Chrome receives), Hacker News, Wikipedia, edudia.in and example.com,
  plus Chrome references captured by `tests/support/site_snapshot.mjs`. Geometry, size and pixel
  scores are recorded in `scores.json`.
- The Chrome comparison tests find Chrome on Linux through `native_visual::chrome_binary()`.
- Findings already fixed:
  - Percentage heights against an auto-height container now behave as `auto` (CSS 2.1 §10.5).
  - The root element resolves percentage heights against the viewport.
  - Overflow on `body` and `html` propagates to the viewport.
  - Fixed and absolute boxes escape overflow clips outside their containing-block chain.
  - UA input and textarea borders and padding now match Chrome.

  Together these made the Google consent dialog render.
- Findings still open:
  - Stylo's `AtomicRefCell` panics with "already immutably borrowed" after many Views in one
    process. This is state leaking across Views, and more support for Phase 3.
  - SVG children report empty client rects.
  - Font fallback for Verdana is wider than Chrome's.
  - `::after` is not generated.
  - The viewport ignores `overflow:hidden`.
- WPT baseline (`baseline.json`), 7,628 tests:
  - 1,858 pass (24.4%), and 11,544 of 28,711 subtests pass (40.2%).
  - Of the rest, 4,628 fail, 867 time out, 270 are errors and 5 crash.
  - The weakest suites are CSS2 tables (4%), css-grid (17%) and mediaqueries (29%).
- Still left to close Phase 0:
  - [ ] The full `cargo test -p wega-native -p wega-engine` run. Don't run it alongside the WPT
    runner: together they ran out of memory. Deferred to the end of Phase 1.
  - [x] `native_accessibility.rs:76`: option selectedness lived only in JS (`_selected`, since
    round 13), so native readers saw the `selected` attribute. It now lives in `WegaDocument`
    (`option_selected`), like checkedness. The next assertion (input width) now expects
    Chrome's 218 px.
  - [ ] Re-record the site scores with `WEGA_UPDATE_SITE_SCORES=1`. Deferred to the end of Phase 1.
  - [x] Clippy with `-D warnings` (engines workspace) and `wega-architecture-check` (0 violations
    before Phase 1).
  - [x] Commit (`969f183`).

### Phase 1: Mechanical split (no behavior change)
1. Create the crates above and move code:
   - `values` ← the value types in `css.rs`/`syntax.rs`
   - `parser` ← `syntax.rs` plus the media and at-rule parts of `css.rs`
   - `selectors` ← selector types and matching
   - `style` ← cascade, UA sheet, hints, `apply_declaration`
   - layout split by formatting context
   - `display-list` ← `display_list.rs`
2. Keep `wega-native` as a thin re-export facade until `wega-engine` is moved over, then
   delete it.
3. Gate: all tests and WPT results identical.

Done (2026-11-21), test gate pending:

| Crate | Directory | From |
|---|---|---|
| `wega-css-values` | `css/values` | value types and `calc` from `css.rs`/`syntax.rs`; value parts of `graphics.rs` |
| `wega-css-selectors` | `css/selectors` | selector types, matching, selector parsing |
| `wega-css-parser` | `css/parser` | rules and at-rules, declarations, media, shorthands, `var()` |
| `wega-style` | `css/style-engine` | `ComputedStyle`, cascade, `apply_declaration`, UA sheet, hints, `computed.rs` |
| `wega-layout` | `layout/core` | `layout.rs` as modules per formatting context; `text.rs`, `controls.rs` |
| `wega-display-list` | `graphics/display-list` | `display_list.rs`; `hit_test_node` |
| `wega-image` | `graphics/image` | `images/`; SVG parts of `graphics.rs` |

- `wega-native` is deleted:
  - `wega-engine` and `wega-adapter-blitz` import the new crates directly.
  - `wega_engine::native_engine::css` groups the four CSS crates for embedders.
  - `NativeResources` moved to `wega_layout::resources`.
  - The vendored `blitz-dom` imports image payloads from `wega-image`.
    `wega-owned-images.patch` is updated, and `verify-engine-vendor.py` passes.
  - `wega-native`'s tests moved: `images.rs` to `wega-image`; `owned_pipeline.rs` and
    `benchmark.rs` to `wega-display-list`. The Blitz-independence test now checks
    `wega-display-list`, which depends on every pipeline crate.
- Moved items are identical after normalizing whitespace and visibility, except four
  deliberate edits:
  - `ClipPath::path` takes a `kurbo::Rect`, so values do not depend on `wega-dom`.
  - The cascade sets the viewport through `set_viewport()`.
  - `hit_test_node` moved to display-list, which breaks the layout ↔ display-list cycle.
  - `paint_context` builds the `kurbo::Rect` for `ClipPath::path`.
- Clippy with `-D warnings` is clean.
- Known deviations, now exit criteria for later phases:
  - **Architecture check ARCH-003 violations:** `wega-layout -> wega-dom`,
    `wega-display-list -> wega-layout` and `wega-display-list -> wega-dom`.
    `adapters/style-layout` and `adapters/dom-css` remove these.
  - **The checker's contract list still names a single `wega-css` crate**, so
    `wega-css-selectors -> wega-dom` and `wega-style -> wega-dom` are not reported yet.
  - **Formatting contexts are mutually recursive through `layout_box`**, so they are modules,
    not crates, until a dispatch trait exists.
  - **Two files exceed 600 lines**, each a single function: `apply.rs` (687) and
    `block.rs` (653).

### Phase 2: Typed property model (the structural fix)
1. In `css/parser`, add a property table modeled on Stylo's longhand/shorthand
   definitions. Each entry has a typed `parse(&mut Parser) -> Result<SpecifiedValue>`,
   and shorthands expand to longhands at parse time.
2. Declarations become `PropertyDeclaration` enums, not strings. `var()`-dependent
   declarations stay token streams, substituted at compute time (as in Stylo's
   `custom_properties.rs`).
3. The cascade applies typed values, so `apply_declaration`'s string re-parsing goes
   away.
4. Port property families in priority order of real-site impact:
   1. box model and position
   2. color and background
   3. border and outline
   4. font and text
   5. flex and grid
   6. transforms and effects
   7. animations and transitions (computed only)
5. Gate: the WPT `css-cascade`, `css-values`, `css-color` and `css-variables` pass
   rates rise. Unsupported properties appear in diagnostics.

Progress (2026-11-21), step 1 of the port order (box and position), test gate pending:
- **Types:** `wega_css_parser::PropertyDeclaration` (`typed.rs`). `Declaration::new`
  parses the value once, at stylesheet parse time, into `Declaration::typed`:
  - `Parsed::Typed` for ported longhands.
  - `Parsed::Invalid` for ported longhands with bad values.
  - `Parsed::Deferred` for everything still on the string path: unported families,
    shorthands, logical properties, CSS-wide keywords, `var()` and `currentcolor`.
- **Ported longhands:**
  - `display`, `position`, `box-sizing`, `visibility`, `pointer-events`
  - widths and heights with their min/max variants
  - margin and padding longhands, `top`, `right`, `bottom`, `left`
  - `z-index`, `float`, `clear`, `overflow-x`, `overflow-y`
- **Cascade:** applies `Typed` declarations with `apply_typed` (no re-parse), skips
  `Invalid` ones, and sends `Deferred` ones to `apply_declaration` as before.
  Presentational hints are parsed when matched.
- **Behavior change (spec fix):** an invalid `float`, `clear` or `z-index` used to reset
  the property to its initial value. It is now dropped, so the previous cascaded value
  wins (CSS Syntax §5.4.4 / CSS Cascade §4). All other ported properties keep their
  exact previous acceptance rules: the typed parsers call the same value parsers.
- **Diagnostics:** `StyleSheet::unsupported` was collected but never reported. `wega-engine`
  now emits each entry once per document as a `css` warning `WEGA_DIAGNOSTIC`:
  - unsupported properties
  - invalid values (`invalid value:<property>: <value>`)
  - unsupported at-rules and selectors
  - viewport-relative typography
- **Steps 2–6 (2026-11-22):** color and background, border, font and text, flex and grid,
  transforms and effects.
  - 64 longhands are typed. Values that depend on the element (`em` borders, gaps,
    `border-spacing`, `line-height` lengths) stay specified and are resolved in
    `apply_typed`.
  - Math functions with viewport units are deferred at parse time;
    `PropertyDeclaration::parse_value` types them once the viewport is known.
  - `apply_declaration` maps logical properties and shorthands, then calls
    `parse_value`. Its 64 string arms for ported properties are deleted, so `apply.rs`
    went from 687 to 293 lines.
  - Still strings (each needs a real parser first):
    - shorthands: `margin`, `padding`, `border*`, `background`, `mask`, `overflow`,
      `grid-template`, `grid-area`
    - `border-radius`
    - `background-image` (gradients need `currentcolor`)
    - `font-size`, which the cascade special-cases
    - `grid-row`/`grid-column` and their start/end longhands, which depend on each other
  - `clip-path`, `transform`, `box-shadow` and `font-style` keep their exact previous
    behavior: their parsers return `None` for valid values that aren't painted yet, so
    "invalid" can't be told apart from "unsupported".
  - Site corpus scores are identical before and after.
- **Step 7 (animations):** nothing to port. The native engine has no animation or
  transition properties yet.

### Phase 3: Own stylesheet source and CSSOM
1. Parse stylesheets from their original source text in `css/parser`. Drop the Stylo
   re-serialization (`native_stylesheet_text`).
2. Back the CSSOM (`insertRule`, `cssText` and so on) with Wega's stylesheet model.
3. Retire Stylo from the worker, and with it the duplicated parse and memory cost.
4. Gate: CSSOM tests (`round15`, `inspector_styles`, `css`) pass. `:has()` and
   anything else Stylo rejected now apply.

Progress (2026-11-22): **tried and reverted.**
- **What was tried:** feeding the native engine the original stylesheet text (`b473723`).
- **Results:** `:has()` worked and the site corpus didn't change. But the full WPT run found
  37 baseline regressions; all 37 pass again with Stylo's text.
- **Cause:** Stylo used to validate the CSS before the native engine saw it, and the native
  parsers are more lenient than Stylo. Examples:
  - mixed-unit `rgb()`/`hsl()`
  - invalid media types
  - `@supports` edge cases
  - `revert`/`revert-layer`
  - negative `background-size`
  - some selectors
- **Reverted in:** the commit after `ad3f036`.
- **Re-landing condition:** the native parsers must reject these first. With the source text
  re-enabled, every one of these tests must pass:
  - `css/css-backgrounds/background-size-with-negative-value.html`
  - `css/css-cascade/revert-layer-001.html`
  - `css/css-cascade/revert-layer-002.html`
  - `css/css-cascade/revert-layer-006.html`
  - `css/css-cascade/revert-layer-007.html`
  - `css/css-cascade/revert-rule-layer.html`
  - `css/css-cascade/revert-val-001.html`
  - `css/css-color/relative-currentcolor-rec2020-02.html`
  - `css/css-color/t421-rgb-func-no-mixed-f.xht`
  - `css/css-color/t422-rgba-func-no-mixed-f.xht`
  - `css/css-color/t424-hsl-parsing-f.xht`
  - `css/css-color/t425-hsla-parsing-f.xht`
  - `css/css-conditional/at-supports-020.html`
  - `css/css-conditional/at-supports-026.html`
  - `css/css-conditional/css-supports-013.xht`
  - `css/css-conditional/css-supports-014.xht`
  - `css/css-conditional/css-supports-039.xht`
  - `css/css-flexbox/balance/balance-002.html`
  - `css/css-flexbox/flex-basis-004.html`
  - `css/css-flexbox/relayout-image-load.html`
  - `css/css-grid/alignment/grid-gutters-002.html`
  - `css/css-grid/alignment/grid-gutters-004.html`
  - `css/css-grid/alignment/grid-gutters-006.html`
  - `css/css-values/angle-units-001.html`
  - `css/css-values/attr-IACVT.html`
  - `css/css-values/calc-rgb-percent-001.html`
  - `css/css-variables/revert-rule-to-var.html`
  - `css/css-variables/variable-declaration-11.html`
  - `css/css-variables/variable-declaration-12.html`
  - `css/css-variables/variable-declaration-13.html`
  - `css/mediaqueries/mq-invalid-media-type-001.html`
  - `css/mediaqueries/mq-invalid-media-type-002.html`
  - `css/mediaqueries/mq-invalid-media-type-004.html`
  - `css/mediaqueries/mq-invalid-media-type-layer-001.html`
  - `css/selectors/dir-selector-ltr-002.html`
  - `css/selectors/selectors-attr-white-space-001.html`
  - `css/selectors/x-pseudo-element.html`

Progress (2026-11-26): **step 1 re-landed.** The native engine parses the original
stylesheet text again (`b473723`), after the native parsers were made to reject what the
spec rejects:
- **Colors:** CSS Color 4 grammar for `rgb()`/`hsl()`/`hwb()` (no mixed legacy numbers and
  percentages, hue units), `calc()`/`min()`/`max()`/`clamp()` channels, unclamped `hsl()`,
  `color()` with the predefined spaces (`rec2020` uses the 2.4 gamma), relative
  `color(from …)`, and `color-mix(in srgb, …)` mixing encoded values, rounded once.
- **Media queries:** `only`, `not`, `and`, `or` and `layer` are not media types; invalid
  queries never match.
- **`@supports`:** syntax errors drop the rule (leftover tokens, `and`/`or` mixed without
  parentheses, `or(`); declaration tests check the value, allowing `!important`.
- **Selectors:** unknown pseudo-elements invalidate the list (`-webkit-` ones are kept, as
  Chrome does); `:dir()` takes only `ltr`/`rtl`; attribute selectors accept `[|a]` and
  `[*|a]`, not whitespace around `|`.
- **Values:** negative `flex-basis`, gaps and `background-size` are invalid; `flex-wrap`
  keywords; gradient angle units are validated at parse time; `grid-gap` aliases;
  `flex-flow` is now supported.
- **Cascade:** `revert`, `revert-layer` and `revert-rule`; `var()` fallbacks reject a
  top-level `;` or `!`; `attr()` substitution with IACVT; `currentcolor` inside values, and
  a `currentcolor`-relative `background-color` re-resolves when inherited.

Results (Linux, comparing the source text with Stylo's text at `2384b98`):
- All 37 acceptance tests pass.
- `flex-flow` now taking effect exposed missing flex layout, which is ported after Taffy
  (the layout engine Blitz uses): multi-line `align-content` (`normal` stretches lines),
  `flex-wrap: balance` (Taffy's balancing dynamic program, without its zero-sized-item
  rule) and `wrap-reverse`, and flex axes that follow `writing-mode` and `direction`.
  Logical properties map by writing mode and direction, percentage gaps resolve at layout,
  and a vertical-writing-mode flex container with `width: auto` fits its content.
- No regressions: the 1,858 baseline-passing tests, the full `css-flexbox` suite (362 →
  441 passing) and `css-logical` + `css-grid` (464 → 478) were compared test by test.
- `relayout-image-load` is timing-dependent: it fails when run alone with either text.
- The CSSOM gates (`round15_acceptance`, `inspector_styles`, `native_real_css_gaps`) pass,
  and the ignored `css` tests fail identically with either text.

Progress (2026-11-26): **steps 2–3, part 1 of 4: DOM selector queries.**
Steps 2–3 run in four parts, each compared test by test with the previous commit:
1. `querySelector`/`matches`/`closest` on `wega-css-selectors` (done).
2. Inline styles (`element.style`) with a native serializer, together with `CSS.supports`.
   Moving `CSS.supports` alone broke feature detection (container-query tests stop at
   `CSS.supports("container-type: size")`) and would disagree with `element.style`;
   `supports_text`/`supports_property_value` are ready in `css/parser`.
3. The CSSOM rule tree (`cssRules`, `insertRule`, `cssText`).
4. Stylesheet loading and DevTools inspection, then removing Stylo from the worker.

Part 1 adds to `wega-css-selectors`: a real `:scope`, `:nth-child(An+B of S)`, type
selectors that keep their case (case-insensitive only on HTML elements) and attribute names
likewise, HTML's case-insensitive attribute values, `*|E`/`|E`/`ns|E` with `@namespace`
(default namespace rules for `:is()`/`:where()`/`:not()`, top of sheet only), `:empty`
ignoring comments, parentless elements as only children, and the pseudo-classes and
pseudo-elements Stylo accepted. Invalid selectors (and other `SyntaxError:` native errors)
throw a `SyntaxError` `DOMException`.

Results: selector API, `css/selectors` and `css/cssom` files 316 → 395 passing, subtests
4,813 → 5,536, no regressions. The 1,858 baseline tests and `css/css-conditional` show none
either; `css-cascade/parsing/supports-import-parsing.html` times out only under parallel load.

Part 2, first step (2026-11-26): `element.style` runs on a native CSSOM declaration block
(`css/parser/src/block.rs`): parsing with `!important` precedence, `setProperty`/
`removeProperty` with Stylo's logical-group reordering, `cssText` with shorthand
recombination (including `all`), `var()` shorthands with pending longhands, CSS-wide
keywords, and `length`/`item()`/`getPropertyPriority()` from the block (they were guessed
from `cssText`). The property table (names, aliases, shorthands, keywords, inheritance,
logical groups) is generated from Stylo's metadata by `tools/css-properties/generate.py`.
Values go through `NativeFirst`: keyword properties are native, other value types still use
Stylo (`StyloValues`) until ported. `parsing/` suites and `css/cssom`: 68 more subtests
pass, none lost against the previous commit.

### Phase 4: Layout fidelity, ported per formatting context
Driven by WPT flexbox, grid, floats and tables plus the site corpus. Replace
approximations made during the bug-fix rounds:
- float line shortening (currently a narrowed containing block)
- per-corner `border-radius`
- inset shadows
- generated content for `::before`/`::after`
- `filter: blur()`
- gradient text through `background-clip: text`

Progress (2026-11-22):
- **Done: per-corner `border-radius`.**
  - `ComputedStyle::border_radii` holds four `(px, %)` corners from the shorthand
    (1–4 values) or the corner longhands. The old precedence hack, which ignored a
    longhand after the shorthand, is gone; the cascade decides.
  - `LayoutBox::radii()` resolves percentages and scales overlapping radii down
    (CSS Backgrounds 3 §5.5).
  - Display items carry `kurbo::RoundedRectRadii`, and padding-box clips subtract each
    corner's adjacent borders.
  - Still approximated: elliptical radii use their horizontal value, and blurred
    shadows use the largest corner.
- **Done in Phase 3 (2026-11-26):** multi-line `align-content`, `flex-wrap: balance`,
  `wrap-reverse`, and writing-mode-aware flex axes. Block and inline layout still ignore
  vertical writing modes.
- **Open, from the list above:** tables, grid, float line shortening, inset shadows,
  `::before`/`::after` content, `filter: blur()`, gradient text, SVG child rects, the
  Google card width, Verdana fallback width and viewport `overflow:hidden`.

### Phase 5: Performance
1. Selector map and bloom filter (`css/cache`).
2. Style sharing.
3. Restyle invalidation (`css/invalidation`) in place of full restyles.
4. Gate: `native_benchmark`, `hot_path_bench` and `render_bench` do not regress.
   Restyle on large pages (Wikipedia) is faster.

Progress (2026-11-22):
- **Done: selector map** (`css/cache`, crate `wega-css-cache`).
  - Rules are bucketed by the id, class or tag of their subject compound, and the
    cascade tests only those candidates, in source order.
  - The matched declarations and their order are unchanged. CSS tests and site
    corpus scores are identical.
  - The site corpus test took 56 s against 74 s before. That's one wall-clock sample,
    not a benchmark.
- **Not done:**
  - an ancestor bloom filter for descendant combinators
  - style sharing between siblings with the same id, classes and attributes
  - restyle invalidation; every mutation still restyles the document

## Decisions
- WPT lives at `~/my/wpt` as a sparse checkout of the CSS suites, at the pinned commit.
- Retiring Stylo (Phase 3) comes after Phase 2.
- The User-Agent string is unchanged for now (Google serves its legacy page to
  `Wega/0.1 (Native/V8)`). This is still open for the owner.
