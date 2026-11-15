# D3-B — read-only CSS and style inspection

Design committed before production implementation. Base `ac6928e`; read D0,
D1/D2 designs/reports, D3 readiness/D3.0, D3-A design/report, roadmap and current
contracts. Source takes precedence over older feasibility claims.

## Executed feasibility (phase 1)

`wega-adapter-blitz --test css_inspection_spike`: **3 passed** before production
changes. Existing CSSOM enumerates author owners and nested rule lists; cached
computed width `42px !important` and inherited `rgb(1, 2, 3)` are correct after a
fixture-owned render. Inspection itself borrows `&self`; before rendering cached
style is absent. No inspector layout flush is needed or permitted.

Corrections to D3.0: actual vendored Stylo `StyleSource` stores **only an Arc to a
declaration block**, not `StyleSource::Rule`. The runtime spike deliberately gives
`.a` and nonmatching `.never` rules the same immutable block: the retained rule
tree maps to both, although `.never` does not match. Even without sharing, it
does not retain which selector in a list matched, specificity or source order.
**Stop matched-rule extraction in this slice.** Return explicit unsupported state,
never an empty or guessed matched list. Options: upstream provenance retained at
matching time (common style-engine path changes requiring a separate design and
render benchmarks), or cancellable/budgeted selector matching infrastructure. A
small Blitz accessor cannot recover information Stylo discarded. No such broad
patch or selector re-matching will be introduced here.

Existing `stylesheet_rule_info` serialized a 2,000-rule group into **144,016 bytes
in 1,953 us** in the debug spike. It recursively emits children and cannot accept
a bounded writer. Direct public `author_stylesheets` + `guard` + borrowed rule
lists permit shallow pagination without a vendor accessor. Declaration value
serialization is restricted to `String`; a tiny Stylo additive bounded-writer
entry point is justified to stop serialization before allocating oversized
values. Preserve the existing string entry point and its behavior.

The spike also confirms the known in-sheet generation gap. Close it narrowly in
Blitz CSSOM successful invalidation paths (insert/delete, declaration/selector
changes, disabled state). No editing API is added. This is required so old rule
paths cannot alias after a page or native CSSOM edit. Record both vendor changes
as exact patches in `sources.json`, verify archive integrity and test failures
leave the generation unchanged. No new dependencies.

## Wega-owned contracts and references

Add `wega_dom::inspect::StyleInspect`: borrowed authoritative CSS state mapped to
bounded snapshots. Plain Wega types: sheet metadata (owner key, source URL,
disabled, rule count, author origin); rule metadata (index path, kind, selector
list, child/declaration counts, optional start location); declarations (name,
value, importance, truncation); availability (ready/not-yet-computed/unsupported).
No adapter type escapes. No second CSS engine, stylesheet copy or always-on hook.

Protocol version 25 / inspector 4 with a styles capability and closed read query:
sheet pages, rule-list pages, rule declarations, inline declarations, computed
declarations and explicit unsupported matched styles. A rule reference includes
target (tab/frame/process), session, document generation+replacement revision,
mutation epoch, stylesheet generation, owner node key and index path. Validate
all fields at execution, not only when the reference was issued. Node requests
reuse document-scoped generation-bearing IDs and connected-light-DOM checks.

Source locations are parser start line/column and stylesheet URL where available;
no invented end ranges. Selector text is a **rule selector list**, not a claim that
it matched the selected element. Matched selector identity, specificity, cascade
winning/overridden status, layer/source-order ranking and inherited-rule chains
are unsupported. Author sheet listing excludes UA/user/imported child sheets;
their absence must be labelled rather than presented as no styles.

## Computed values and safety

Read cached primary styles only, never `resolve_undisplayed_style`, layout,
style traversal, matching, JS or geometry. Before first rendering return
not-yet-computed. After mutations/viewport changes report cached values with
`layout_pending`; normal rendering, not inspection, refreshes them. No guarantee
that values represent the newest animation/input state.

Initial computed coverage is an explicit bounded scalar-longhand set (colors,
display/position, dimensions, margins/padding, font size/weight/line height,
opacity and box sizing). Serialize borrowed values into a bounded writer; do not
clone arbitrary vector-valued computed properties or resolve custom properties.
Label other longhands, custom/pseudo/visited styles and used-value computation as
unsupported. Inheritance, specificity, importance, inline and media behavior are
verified through cached values after fixture rendering, not inferred metadata.

## Budgets and UI

32 entries/page, 512 UTF-8 bytes per string, 16 rule-path components, 512 author
owners scanned/listed, 128 ancestor edges, 10 ms cooperative read deadline,
64 KiB response, existing 2 s transport deadline. Bound all iteration, including
skipping to a page and attribute/property serialization. Never serialize an
entire group, clone declaration blocks or concatenate subtree CSS.

Styles/Computed controls live inside Elements. Retain one response page, one rule
reference and one in-flight style request, with timeout/retry pacing; DOM retains
its independent D3-A pagination cache and one pending read. Total remains within
the existing four-request inspector limit. Selection/document/session/process
changes clear style values; mutation/stylesheet epochs invalidate rule paths.
Computed refresh is paced at the existing one-second interval while visible, with
an explicit refresh control and cached/pending label. Detached tools use the same
renderer and paint-time authority checks. Clearly distinguish unsupported,
not-ready and actual empty pages. Plain egui text, bounded rendering, no editing,
picker, overlay or D3-C work.

## Delivery and validation

1. This design plus runtime spike, locally committed first.
2. Narrow vendor fixes, Wega contract/adapter, typed protocol and engine backend;
   test each before committing.
3. Browser/UI and lifecycle tests, separately committed.
4. Real-process, required-worker sandbox and native/detached-window tests; inheritance,
   specificity, importance, inline/media, CSSOM edits/stale refs, cross-tab/process/
   session negatives, huge values/groups/rule lists/pathological selectors; deep
   unrendered tree refuses computed reads without flushing. Full locked suites,
   WPT/CSS-WPT, architecture/boundaries/vendor, fmt/clippy, metadata/cycles and
   unsafe inventory. Benchmark reads/RSS and normal rendering; preserve console,
   native mutation and D3-A pagination/selection tests and benchmarks.
5. Exact report and evidence, logical local commits; no push.

D3-C still needs the existing first-flush depth-122 layout hang fixed. Matched
rules need a separate provenance/matching design; D3-B does not hide that gap.
