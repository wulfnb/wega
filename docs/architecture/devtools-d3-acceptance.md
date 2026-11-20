# D3 acceptance

What is implemented, what is only partly supported, what is explicitly unsupported, and under
which conditions each claim was verified. Counts, timings and commands are in
[`devtools-d3-final-report.md`](./devtools-d3-final-report.md); decisions in
[`devtools-d3-final-design.md`](./devtools-d3-final-design.md). Evidence files are under
[`baseline/`](./baseline/).

Verification platform: **macOS 27.2 arm64 only**, rustc from `rust-toolchain.toml`. Linux and
Windows were not tested. Native acceptance ran with the **signed, App-Sandboxed content worker**
(`WegaContent.app` bundled by `wega-content-bundle`, ad-hoc signed) and also with the development
worker.

## 1. Fully implemented and tested

| Capability | How it is verified |
|---|---|
| **Layout safety guard** (`MAX_LAYOUT_DEPTH = 256`) | Killable child processes on the engine's 8 MiB stack, debug **and release**, depths 120, 122, 128, 200, 256, 257, 520, 600, 1,000 through script flush, paint, paint-then-flush and a misc route (computed style, geometry reads, resize, hidpi/dark repaint, pointer); recovery with an ordinary page on the same view; real `wega-engine` worker with a 1,000-deep document. Unguarded release cliff measured between 3,200 and 4,800 levels (8 MiB); debug 520 to 600. |
| **Cached geometry**: content, padding, border and margin boxes, bounding rect, viewport size, scroll, zoom and device scale | Adapter tests (scale 1/2, zoom 1/1.5, viewport and nested scroll, hit-test agreement), engine tests, real-process tests, native test. CSS pixels, viewport-relative; DPR does not change them. |
| **Freshness states**: Current, Stale, Pending, NoBoxes, Refused, Invalid | Each produced and asserted without any flush (adapter and real worker). |
| **Inspect Element picker**: toolbar control, hover, click-to-select, Escape, button toggle, restoration of page input | Native test with real injected pointer events through `wega_ui.rs`; the page's own capture-phase listeners saw zero events (`IN:{}`). Session tests for tab switch, navigation, process replacement, crash, leaving Elements, close, detached pinned window. |
| **Highlight overlay** (hover and selected; margin/border/padding/content bands; label) | Pure `plan()` tests (exclusive bands tile the margin box exactly, clip and viewport cutting, stale marking, zoom mapping) plus native evidence images with the overlay composited on the engine's page frame; the engine's page frame contains **0** overlay-palette pixels. |
| **Box Model panel** (read-only) | Status text for every state asserted; drawn in the native run. |
| **Separate editing grant** | Protocol, session and real-process tests: an evaluation token never opens editing and the reverse; forged token, wrong session, wrong document, revoked, navigated, replaced process all refused; generic UI entry refuses `Mutate` and `GrantMutation`; sandboxed-worker test. |
| **DOM editing**: set, add and remove attribute; edit text and comment node data; remove node | Real-process tests read the result back through page script; budgets shared with script (text budget exhaustion refuses the edit). |
| **Inline CSS editing**: add, change, remove declaration, `!important` | Real-process tests; native test measures the rendered change on engine pixels (0 to 152,598 pure-red pixels). |
| **Selection refresh after edits** | The edit's epoch is adopted; pages refresh through the normal bounded path; removal forgets only the removed node (no tree rebuild). |
| **Read-only DOM, CSS and console inspection** | Unchanged and re-run (see the report's matrix). |

## 2. Partially supported

| Area | What works | What does not |
|---|---|---|
| Highlight accuracy | Exact for untransformed elements; ancestor `overflow` clips (padding boxes) are intersected | Elements under any transform, `translate`, `rotate`, `scale` or `perspective` are **not highlighted** (explicit notice). A positioned (`absolute`/`fixed`) element under a clipping ancestor is not highlighted. `html`/`body` overflow is assumed to propagate to the viewport. `clip-path`, masks and `filter` are not modelled. |
| Stacking and occlusion | The overlay draws above the page for the chosen element | An element hidden under another element is still highlighted (it is chosen by hit test, not by painted visibility). |
| Label | Tag, id, up to three classes, size; drawn in the app | Not present in the evidence PNGs (they composite rectangles only). |
| Code-running edits | `on*`, `srcdoc`, `javascript:` URLs and `<script>` text need the **evaluation grant too** (verified both ways) | Other attributes that start network loads (`src`, `href` on `img`, `iframe`, `link`) are allowed with the mutation grant alone and load through the normal broker and policy, like page script. |
| Page observers | State is shared with script | Native edits do not produce `MutationObserver` records (script-side wrappers produce them). |
| Removed nodes | Removed from the tree; reported stale afterwards | The detached node stays allocated and counted until the page drops it (bounded by the node budget). |
| Text editing | Text and comment nodes (an element's text is edited through its text child) | A value or text whose preview was truncated (over 256 bytes) is not editable in the UI, to avoid writing back a shortened value. |
| Opaque origins | Allowed only with the user's separate override, like evaluation | Same limits as evaluation. |

## 3. Explicitly unsupported

* **Matched CSS rules, specificity and cascade winners.** D3-B showed retained declaration-block identity
  cannot recover the matching selector; a provenance patch inside Stylo/Blitz was not attempted. The
  panel keeps showing "Matched (unsupported)". Nothing is guessed.
* Stylesheet-rule editing (insert, delete, change in a sheet).
* Arbitrary HTML replacement, subtree insertion, node move or reorder.
* **Undo and redo.** Not implemented and not advertised.
* Shadow DOM, iframes, template contents as editing targets.
* Box-model editing by dragging the diagram.
* Highlighting under transforms (see above).

## 4. Security limitations

* The browser can only mint a mutation grant from a user action in trusted UI; there is no unauthenticated
  endpoint, and no remote-debugging transport was added.
* The content process cannot authenticate the principal; it checks the browser's claim for consistency
  with its own document (same rule as evaluation).
* An edit is bound to the mutation epoch the user saw; a stale epoch is retried **once** after a status
  refresh, then reported. A page that mutates continuously can therefore starve DevTools edits.
* A hostile page can make geometry `Pending`, `Stale` or `Refused`; DevTools then shows that state and
  never forces layout.
* Native edits spend the same text budget as script; a page that has used its budget blocks edits.
* `MutationObserver` blindness (above) means a page cannot detect DevTools edits that way.
* Release-build sandbox signing used an ad-hoc signature; a production developer-ID signature path was
  not exercised.

## 5. Rendering and layout limitations

* Documents nested deeper than **256 tree levels** are not laid out (blank page); geometry says `Refused`.
  Chrome renders them; this is a safety trade-off pending an engine fix.
* The limit is validated for the 8 MiB engine thread on macOS arm64. Any thread hosting a view must have
  at least that stack.
* HTML parsing is O(depth²) (html5ever scope scans): 5,000 levels about 2 s, 20,000 levels did not finish
  in 15 s (debug). Tracked as an engine defect; the fix is a parser nesting cap like Chrome's 512.
* Layout of `display:none`/`contents` nodes has no boxes (reported as `NoBoxes`).

## 6. Platform coverage

| Platform | Status |
|---|---|
| macOS arm64, debug, development worker | tested |
| macOS arm64, debug, signed App-Sandboxed worker | tested (native flow, `inspector_sandbox`, `inherited_broker`, `native_sandbox`) |
| macOS arm64, release engine | layout guard, benchmarks tested; the real-worker pipe harness cannot run (release refuses the development launch) |
| Linux, Windows | **not tested** (no host); stack cliff and overflow behaviour unmeasured there |
| Docker | not available on the test host |

## 7. Remaining defects and deferred work

1. Parser nesting cap (O(depth²) parse, deeper-than-256 documents render blank).
2. Matched-rule provenance (separate Stylo/Blitz design).
3. Transform-aware highlighting (needs the painted matrix from the paint pipeline).
4. Undo with epoch checks.
5. Linux and Windows measurements of the stack cliff; a dedicated-host performance gate.
6. Native-edit `MutationObserver` records, if wanted.
7. The unexplained spin-instead-of-abort of a native stack overflow on macOS (an abort would be a safer
   failure mode on any platform).
