# DevTools D3 readiness audit (read-only)

Audit of `feature/dom-rounds-9-12` at `f3d24cb` (D2 complete). **Nothing in runtime code was changed, D3 was not started, nothing was pushed, and this file is deliberately uncommitted.** The only measurement tooling was a temporary, untracked probe test (deleted) and a temporary release build (deleted). All measurements: macOS arm64 (Apple M4, 10 cores), release profile, load average 1.4–3.5.

Paths: D2 report `docs/architecture/devtools-d2-report.md`; the actual roadmap is `docs/architecture/devtools-implementation-roadmap.md` (D1 and D2 progress notes are already in it; the D3 section starts at "D3: DOM tree, selection, computed styles, CSS editing, layout highlighting").

## 1. Corrections to earlier documents (found by this audit)

| Claim | Reality | Consequence |
|---|---|---|
| D0 design / capabilities: "the engine has no CSSOM; matched rules and stylesheet listing are the largest gap" | The **vendored `blitz-dom` has a CSSOM layer** (`vendor/blitz/blitz-dom/src/cssom.rs`, 972 lines, Wega-modified): `stylesheet_owner_nodes`, `stylesheet_rule_count/info(path)`, `stylesheet_insert_rule/delete_rule`, `stylesheet_rule_style_{css_text,property_names,get,set,remove}_property`, `stylesheet_rule_set_selector_text`, `stylesheet_disabled`, and a **`stylesheet_generation()` change counter**. **No Wega code (adapter or engine) calls it** (grep of `wega/` and `adapters/`: zero hits). | Rule *listing and editing* is an adapter-mapping job, not new engine work. The genuinely missing piece is **matched rules per element** (cascade/specificity/origin): `Stylist` is `pub(crate)` in `BaseDocument` and no matching API exists. |
| D1/D2: `DocumentRevision` could drive DOM change events | `document_revision` changes only when the DOM/global is **replaced**. The only mutation counter is `Dom.revision` (`dom.rs:190`, bumped by `Dom::invalidate` on JS host-op mutations). Parser-driven changes, Blitz-internal changes (form state, focus/hover pseudo-state, stylesheet arrival) and WebDriver do not bump it. `MutationObserver` is implemented in JS (`semantics.js`) and sees only script mutations. | No usable change signal exists today for the Elements panel (see §3.4). |
| D2: origin policy is "scheme-based approximation" | The **actual principal** is stricter: `wega_network::principal(url)` returns `Origin::Tuple` only for `http`/`https`; **everything else, including `file:`, is `Opaque`**. D2 lets `file:` through without an override. | D2 is *looser* than the real principal for `file:` pages (§4). |
| D2 test `a_page_can_forge_its_own_console_entries_but_nothing_unbounded_survives` | **Vacuous.** It calls `Deno.core.ops.op_dom_read(...)` from the page and swallows the error with `.ok()`; `Deno` is **not defined in page scope** (`ReferenceError: Deno is not defined`, confirmed; the sandbox test also asserts `typeof Deno === 'undefined'`). The ring's validation of forged entries *is* covered by unit tests, so no security claim changes, but that integration test proves nothing. | Fix before D3 (§7). Good news: pages cannot reach host ops directly. |

## 2. Always-on console capture: measured profile

D2 measured +5 to +14 µs per console call (release, D1 → D2 A/B). This audit attributes it with a CPU sampler (`sample`, 1 ms, 8 s, DOM thread only, release build, stderr to `/dev/null`) plus whole-call variants and a Rust-side replica. Single runs; percentages are sampling shares of the DOM thread's busy time and are approximate (±2 points).

### 2.1 Whole-call variants (D2 build, µs per call)

| Call | µs |
|---|---|
| `console.log()` (no arguments) | 8.77 |
| `console.log(i)` | 10.78 |
| `console.log(i, i)` | 11.92 |
| 4 numeric arguments | 13.92 |
| 8 numeric arguments | 17.09 (≈ **1.04 µs per extra argument**) |
| `console.log('some message text')` | 9.93 |
| `console.log('x', nested object)` | 32.60 |
| `console.log(100-property object)` | 121.4 |
| `console.error('e', i)` | 16.61 |

The fixed cost (≈ 8.8 µs with no arguments) dominates simple calls; argument cost is small for primitives and large for objects.

### 2.2 Where the time goes (`console.log('x', i)`, 4,690 samples)

| Share of DOM-thread time | Component | Pre-existing or D2? |
|---|---|---|
| **37.7 %** | `std::io::_eprint` → `write(2)` for the host `log` op's `eprintln!("Wega JavaScript: …")` | **pre-existing** (D1 has it) |
| **18.8 %** | host op `op_current_user_call_site` (`v8::StackTrace::CurrentStackTrace` 10.9 %, `StackFrame::GetLineNumber` 4.3 %, column 0.9 %, string 0.9 %) | **D2** (call site for source location) |
| **16.3 %** | `ConsoleLog::ingest` (serde parse, validation, **two full `approx_bytes()` serialisations**, ring push) | **D2** |
| 2.6 % | `String::to_rust_cow_lossy` (op argument conversion, both ops) | shared |
| ≈ 24.6 % | JavaScript: Deno formatter, my wrapper, preview building, string assembly, GC; V8 `JsonStringify` (my `quote()`) is 2.5 % of it | formatter pre-existing; wrapper/preview D2 |

Nested argument (`console.log('x', nested)`, 5,171 samples): ingest 15.3 %, `eprint` 12.2 %, call-site op 8.1 %, JavaScript ≈ 68 % (formatter + preview; the split between them was **not** measured separately).

Cross-checks: capture's measured delta (5.3 µs of 11.4 µs for the simple call = 47 %) ≈ call-site 18.8 % + ingest 16.3 % + a share of conversion and wrapper/preview JS; a Rust replica of parse + validate + size costs 1.07 µs (2 previews) and 2.70 µs (nested) for *one* serialisation, while the real path serialises twice.

### 2.3 What is cheap to fix, with expected effect (estimates, not implemented)

1. **Ingest double serialisation**: compute the size once (or from the input length): saves roughly half of ingest, ≈ 8 % of a simple call.
2. **Call-site op**: 19 % of a simple call and the largest single D2 cost. Options: capture only for `warn`/`error`/`trace` and exceptions; or capture lazily on first read by DevTools is impossible (the stack is gone), so the real trade-off is *source location for every log vs ~2 µs per log*. Needs a decision, not a guess.
3. **One host call per entry** is already the case; batching would need a JS-side buffer flushed at task end (adds complexity and delays the first message).
4. The `eprintln!` (37.7 %) is **not D2's cost** and is the single biggest line; making it conditional (diagnostics only) would speed console calls by more than all D2 optimisations together, but changes existing stderr behaviour the browser logs from (`wega::diagnostics::drain`), so it is a product decision.

**Readiness conclusion:** the always-on cost is understood and bounded (≈ +5 µs/call release, 47 % of the simple call, 33 % of the nested one); it should be reduced *or explicitly accepted* before D3 adds more always-on hooks. D3's plan below adds none.

## 3. D3 contract inventory (verified in source)

### 3.1 Reusable as is (`engines/dom/core`, crate `wega-dom`, forbids unsafe, boundary-tested to name no Blitz type)

| Need | Existing interface |
|---|---|
| Tree reads | `DocumentView` (`node`, `element_by_id`, `root`, `base_url`), `NodeView` (`kind`, `element_name`, `qual_name`, `attr`, `attributes`, `parent`, `children`, `child_at`), helpers `sibling`, `tree_root`, `attribute`, `subtree`, `node_info` (nodeType/nodeName/localName/namespace/prefix) |
| Node kinds | `NodeKind::{Document, Element, AnonymousBlock, Text, Comment}`; **`AnonymousBlock` is layout-created and must be hidden** from the Elements tree |
| Identity | `NodeKey(u64)`: a slotmap key (index+version), so a dropped node never aliases within a document (tested: `dom_identity.rs`); **not unique across documents**, so it must always travel with a document stamp |
| Geometry | `GeometryAccess`: `has_boxes`, `bounding_rect`, `client_rects`, `client_box`, `offset_box`, `scroll_size`, `scroll_offset`, `content_box`, `viewport` (CSS px, DPR incl. zoom, scroll) |
| Hit test | `HitTest::element_at(point)` (viewport-relative, resolves text/anonymous boxes to the nearest element) |
| Layout flush | `LayoutControl::flush_layout` (the only operation that recalculates; readers never flush) |
| Style reads | `StyleAccess::{inline_style, inline_property, resolved_value(key, property), supports_*}` |
| Edits | `DocumentMutation`/`Mutator` (validate-then-apply, stale keys give `Stale`, never alias or panic), `StyleControl::set_inline_property` |
| Focus/hover | `FocusState::{focused, keyboard_target, hovered}` |
| Engine seams | `PagePort` (D2) as the engine-side port; `InspectorArgs` for parameters; `Preview` for bounded values; per-command deadlines; grant mechanism; paced, budgeted delivery pattern; `Dom` node/text budgets (`allocated`, `used`); `document_generation`/`document_revision`/`DocumentContext` |
| Vendored, unused | `blitz-dom::cssom` (§1) and `stylesheet_generation()` |
| Test infrastructure | in-process V8 tests, real-process harness (`inspector_worker.rs`), sandbox test, native `--inspector-test`, `console_bench`/`inspector_bench` |

### 3.2 Gaps (new interfaces or adapters required)

| # | Gap | Where | Notes |
|---|---|---|---|
| G1 | **Matched rules per element** (rule ref, selector, specificity, cascade origin/order, `!important`, source range) | new `wega_dom::inspect::StyleInspect` + a **vendor patch** to `blitz-dom` (and possibly `stylo`) | `Stylist` is `pub(crate)`; vendor changes need a patch file plus `engines/vendor/sources.json` entry and pass `scripts/verify-engine-vendor.py` (existing patches: `blitz-dom.patch`, `content-image-budgets.patch`, `document-id-scope.patch`, …). The largest single piece; prototype first. |
| G2 | **Bulk computed style** (all longhands) | `StyleInspect::computed_all` in the adapter (Stylo `LonghandId` iteration) | Today only one property per call (`resolved_value`); per-call cost measured 0.65 µs, so ≈ 0.3 ms for ~400 properties through the existing API is possible *without* a new contract; a bulk method mainly avoids 400 string round trips and gives a stable ordering. |
| G3 | **Stylesheet / rule listing and rule edits** | adapter methods mapping `blitz-dom::cssom` onto a `wega_dom::inspect` contract (`SheetInfo`, `RuleInfo`) | Reuse, don't rebuild. Rule paths are index paths: invalid after any sheet edit; tie them to `stylesheet_generation`. |
| G4 | **Box model** (margin, border, padding, content boxes) | adapter `box_model(key)` | `bounding_rect` is the border box; `client_box`/`content_box` give padding/content offsets; margins/borders can be assembled from `resolved_value` (`margin-*`, `border-*-width`) as a fallback, but the adapter has the layout node's margin/border directly. |
| G5 | **Document mutation epoch** | adapter newtype counter bumped at `mutate()`/`set_inline_property`/`refresh_style_element`, plus `stylesheet_generation` | Needed for coalesced `documentUpdated`. **Does not cover** Blitz-internal self-mutations (form values, focus/hover state, image/stylesheet arrival) — those need explicit hooks or are documented as "refresh on demand". |
| G6 | **Protocol additions**: `InspectorNodeId{stamp, NodeKey}`, `StaleNode`, DOM/CSS commands and events, paging cursors | `wega_protocol::inspector` | Same closed-enum discipline; no Blitz/Stylo/V8 types (the boundary test must extend to the new modules). |
| G7 | **Edit path that honours budgets** | engine | **Risk found:** node and text budgets (`dom_nodes`, `dom_text_bytes`) are enforced inside `Dom`'s host ops, **not** in the `Mutator` trait. A backend calling `Mutator` directly would bypass them. Edits must go through `Dom`'s accounting (or the budget must move into the contract). |
| G8 | **Overlay/picker plumbing** | UI + `InspectAt{x,y}` command | Overlay drawn by the UI from box-model data (no page DOM injection, no pixel reads); coordinates follow the Round 10 viewport conventions (zoom, scroll). |
| G9 | **Origin/capability model** | browser + protocol | §4; plus separate grants for read vs edit (D2's grant is evaluation-only). |

### 3.3 Constraints that shape the design

* Public inspector contracts must not name Blitz, Stylo or V8. Today `engines/wega/src/inspector/` is guarded by a text test for `blitz_`/`stylo`/`v8::`/`deno`/`servo`; the **D3 agent should be generic over the `wega-dom` traits** (`D: DocumentView + GeometryAccess + HitTest + StyleAccess`), reached through `PagePort` implemented on `View`, never naming `wega_adapter_blitz` in `inspector/`.
* `GeometryAccess`/`StyleAccess` readers require the caller to flush first; flushing can be expensive and, above 121 nested blocks, **never finishes** (roadmap B1: layout is not covered by the script watchdog). An inspection request must therefore not force a flush on a page flagged as deep, or must run under a layout budget.
* Child frames/iframes are not rendered or separate targets (F1); shadow trees are detached fragments (F3). Both must be labelled "not inspectable" in D3.

### 3.4 Change notification: what exists

`Dom.revision` (JS mutations only), `stylesheet_generation` (sheet set changes, vendored), `document_revision` (replacement only), JS `MutationObserver` (script mutations only). Recommendation: adapter-level epoch (G5) + `stylesheet_generation`, emit a coalesced `DocumentUpdated{epoch}` at most N times per second, and let the UI re-request visible subtrees; fine-grained attribute events only if profiling shows the coarse signal is insufficient.

## 4. Replacing the URL-scheme evaluation check with the real security principal

### 4.1 How origins are represented today

| Place | Representation | Authority |
|---|---|---|
| Browser, per `Page` | `FrameAuthority` in `Snapshot` (`authority.rs`): `bind(DocumentContext, Principal{origin: Origin::{Tuple(String), Opaque}, top_level_site})`, `resolve(ctx) -> Option<TrustedRequest{session, document, principal}>`. Bound at `Event::DocumentCommitted` from `broker.take_committed_principal(url)`, which only accepts a principal when the **broker observed** the committed response and its origin equals the engine's claimed URL origin; otherwise **`Opaque`**. | **Authoritative** (renderer values never select an origin or partition) |
| `wega_network::principal(url)` | `Tuple` iff scheme is `http`/`https`; everything else `Opaque`; plus `Cookies::site` for the partition | the rule that matters |
| Engine | no principal; `view.url`, `wega_security::secure_context::trustworthy(url)`, CSP, `StorageKey` | derived from the URL, not authoritative |
| D2 evaluation check | `matches!(url.scheme(), "http"\|"https"\|"file")` in `View::origin_allows_evaluation` (engine) and the panel (`devtools_console_ui.rs`, from the tab URL) | **weaker than the principal** (`file:` allowed; broker-observation mismatch ignored) |

### 4.2 Proposed replacement

1. **Browser decides, from the principal.** In `Page::inspector_grant`, resolve `state.authority.resolve(current DocumentContext)`:
   `Tuple` → allowed; `Opaque` → needs the user's override; `None` (unbound, stale, not committed) → refuse. Store the *principal* (not only the stamp) in the registry grant, and re-compare it with the current principal in `grant_for` (covers a re-bind of the same context to a different principal).
2. **Engine verifies consistency, not authority.** The engine has no principal, so `GrantEvaluation` should carry `origin: Option<String>` (the browser's tuple origin, or `None` + override). The engine accepts a tuple origin only if `url.origin().ascii_serialization()` of *its own* document equals it, and treats `None` as "override required". This catches a browser/engine disagreement (redirect/commit races, `about:`/`data:` inheritance) without trusting the engine with the decision. The stamp check stays.
3. **UI uses the same source.** The panel's "Enable anyway" decision must come from `Page::inspector_principal()` (new, read from `authority`), not from the tab URL.
4. **Behaviour changes to document:** `file:` documents will need the override (they are `Opaque` today); a document whose commit the broker could not corroborate becomes `Opaque` and needs the override; `wega://` pages have no engine and stay `NoPage`.
5. **Add a distinct error code** (`PolicyDenied`) instead of overloading `Unauthorized`, so the UI can explain.
6. **Tests**: tuple, opaque, `file:`, unbound/stale context, principal re-bind between grant and use, engine/browser origin mismatch, container/partition (grant is per `Page`, i.e. per container session, so cross-container reuse is impossible by construction; add a test that asserts it).
7. **Honest limit:** the engine cannot independently authenticate the browser's principal; the guarantee is *consistency with the engine's own URL* plus the browser's authority over the pipe.

## 5. Architectural risks for D3

| # | Risk | Evidence | Mitigation |
|---|---|---|---|
| R1 | Matched-rules extraction needs a **vendor patch** with `Stylist` access | `Stylist` is `pub(crate)`; patch process = patch file + `sources.json` + `verify-engine-vendor.py` | prototype in a throwaway branch; size estimate before committing the phase; keep the patch minimal (read-only accessor) |
| R2 | Edits can **bypass node/text budgets** | budgets live in `Dom` host ops, not in `Mutator` (§3.2 G7) | single edit entry that charges `Dom`'s counters; test that an edit loop hits `DOM node limit exceeded` |
| R3 | **Forced layout** can hang a tab (≥122 nested blocks) or cost 76 ms (300-cell page) per flush | roadmap B1; Round 12 `layout paths` | explicit flush policy: never from hover at frame rate; one flush per request; refuse above a depth flag; layout-time counters in tests |
| R4 | No reliable **change signal** | §3.4 | epoch + generation + on-demand refresh; document uncovered sources |
| R5 | **Stale ids across navigation/mutation** | `NodeKey` not unique across documents | `InspectorNodeId{stamp, key}`; `StaleNode` never resolves to another node; navigation test per command |
| R6 | **Large DOM** (900-node fragment 2 ms, 9,000 nodes 71 ms to parse) | Round 12 numbers | bounded paging (≤ 100 children, ≤ 64 KiB/response), depth limits, no full-tree serialisation |
| R7 | Always-on hooks add cost (D2 lesson) | §2 | D3 adds none: everything is on-demand/session-gated; A/B on the 14 hot-path metrics must show no change with DevTools closed |
| R8 | Edits as a **script-independent mutation path** | D0 §6 | separate `edit` grant (off by default, per document), CSP/active-content rules, undo, mutation budgets |
| R9 | Information exposure (password `value` attributes, form state, closed shadow content) | DOM reads return attributes | same-document, user-initiated only; redact `type=password` values by default with an explicit reveal; document |
| R10 | `PagePort` growth | one trait now carries status, console, evaluation | split into sub-traits (`DomPort`, `StylePort`, `ConsolePort`) before adding D3 methods |

## 6. Proposed D3 phases and performance budgets

Budgets are release-profile, engine-side, on the Round 12 fixtures unless noted; **none has been measured yet** for D3 (D3 code does not exist); they are targets to verify, derived from existing numbers (getComputedStyle 0.65 µs/property, 1.1 ms after a mutation, rect read after a write 0.31 ms, layout of 300 cells 76 ms, 900-node fragment 2 ms).

| Phase | Scope | Budgets / acceptance |
|---|---|---|
| **D3.0 prerequisites** | decide the capture-cost question (§2.3); fix test hygiene (§7); split `PagePort`; spike G1 (matched rules) and G5 (epoch) with size estimates; add `PolicyDenied` + principal-based grant (§4) | spike report; evaluation policy tests pass; no behaviour change otherwise |
| **D3.1 DOM read** | `getDocument{depth}`, `requestChildNodes`, `describeNode`, `querySelector(All)`, `InspectorNodeId`, `StaleNode`; hide anonymous boxes; shadow/frames labelled | tree page (≤ 100 nodes) p95 ≤ 2 ms **without layout**; response ≤ 64 KiB; zero retained copy of the DOM (test asserts only ids/counters retained); stale-id test after navigation and after removal |
| **D3.2 change notification** | adapter epoch + `stylesheet_generation`; coalesced `DocumentUpdated` | ≤ 10 events/s/session; ring bounded; **idle CPU 0.00 %** with the panel open and a static page (no polling) |
| **D3.3 geometry** | box model, hit test/picker (`InspectAt`), UI overlay | box model ≤ 0.5 ms (no flush when clean); picker coalesced ≤ 20 requests/s, **no flush when layout is clean**; coordinates correct under zoom/scroll (Round 10 fixtures) |
| **D3.4 CSS read** | computed (bulk), inline style, sheet/rule listing (reuse `cssom`), **matched rules** (G1) | computed all-longhands ≤ 2 ms; rule list page ≤ 2 ms; matched rules ≤ 5 ms for a 50-rule page (target; spike first); rule paths invalidated by `stylesheet_generation` |
| **D3.5 edits** | attribute/text/remove, inline style, rule text; `edit` grant; undo; budget-aware path | every edit hits the same node/text budgets as script (test); undo restores exactly; invalid text is ignored without side effects; edits denied without the grant and after navigation |
| **D3.6 Elements panel** | tree, styles, computed, box model; follow selection across `documentUpdated` | UI never blocks (the D1 stalled-engine tests extended); render-time validation of every displayed value (as `devtools_snapshot`) |
| **D3.7 hardening** | hostile DOM corpus (deep nesting, huge attributes, 10⁵ siblings), fuzz-style malformed payloads, sandboxed-worker run, A/B | A/B on the 14 hot-path metrics within spread with DevTools closed; all D3 commands run under the required worker; no unbounded queue |

## 7. Missing or weak test coverage (carry into D3)

1. `a_page_can_forge_its_own_console_entries…` is vacuous (swallowed `ReferenceError`): replace with a unit/in-process test that calls the ingest path with forged JSON (already unit-tested) and an integration assertion that `Deno` is absent.
2. Real-`Page` tab switch, tab close and process restart with the console/evaluation (view-level with a scripted port only).
3. The pinned detached **OS window** with the Console (shared code, never driven).
4. **Visual verification** of the Console panel (headless draw tests only; no screenshot).
5. Linux, Windows, MSVC Skia, native sandbox probes: never run for any inspector code.
6. Release-profile IPC benchmarks (release workers refuse the unsandboxed mode); only debug numbers exist.
7. The full UI under the **sandboxed** worker (only the protocol-level sandbox test exists).
8. `GetConsoleHistory` and `ExecutionContextDestroyed` are implemented and tested at the engine level but unused by the UI.
9. No fuzzing or golden-file tests for the inspector messages (D1 deferred).
10. No test pins the **budget bypass** risk (R2) or the layout-hang interaction (R3) for inspector-driven reads.
11. No hostile-DOM corpus for inspection (the roadmap lists it as missing infrastructure).
12. CI has not run any inspector test (local macOS only).

## 8. Security concerns for D3 (beyond §4)

* **Capability separation**: D2's grant means "evaluate". D3 needs `read` (default on with the panel), `edit` (explicit, per document), kept as distinct, independently revocable grants bound like D2's.
* **Edits inserting active content** (`<script>`, event-handler attributes, `javascript:` URLs): must obey the page's CSP and the same checks as script insertion, and be labelled in the UI.
* **Sensitive attribute values** (password `value`, autofill, tokens in URLs/attributes): default redaction with explicit reveal (R9); exports out of scope.
* **Node ids as capabilities**: ids are only meaningful with a live session and stamp; never accept an id without both (the D2 grant pattern); an id from another tab or document must fail closed.
* **Overlay safety**: highlights drawn natively; no pixel readback; no DOM injection that script could observe or tamper with (this is also why D0 rejected the old `inspector.js`).
* **Resource exhaustion**: bounded responses and pending requests (4), bounded stylesheet/rule enumeration, bounded selector complexity for `querySelectorAll`, request deadlines including layout.
* **Cross-origin stylesheets**: rule text from a cross-origin sheet is displayed to the *user* (not the page); keep it out of any page-observable channel.
* **WebDriver** remains an unauthenticated evaluation/inspection surface (D0 §6.2): unchanged and still out of scope, but any D3 data it could reach must be noted.

## 9. Dependencies

| Dependency | Needed by | Status |
|---|---|---|
| Matched-rules accessor in vendored `blitz-dom`/`stylo` (patch + `sources.json` + verification) | D3.4 | not started; spike in D3.0 |
| Adapter mutation epoch | D3.2 | not started |
| `wega_dom::inspect` contract module | D3.3–D3.4 | not started (design in D0 §5.3) |
| Principal-based evaluation policy | D3.0 | design in §4 |
| `PagePort` split | D3.0 | not started |
| B1 layout watchdog | D3.3 (forced layout safety) | open (`known_gap_nesting_of_122_blocks…`) |
| B7 idle repaint | D3.2 acceptance (idle CPU) | open (`known_gap_every_poll_marks_a_settled_page_dirty_again`) |
| F3 shadow DOM, F1 iframes | label-only in D3; full support later | open |
| Cross-platform CI for inspector tests | D3.7 | none |

## 10. Verdict

**D3 is feasible and the D2 foundation is sufficient** (session authority, correlation, deadlines, paged/paced delivery, `PagePort`, grants, test harnesses all carry over). It should **not start before D3.0**: the evaluation policy is weaker than the real principal, three test-quality gaps exist, the matched-rules gap needs a vendor patch whose size is unknown, there is no change signal, and edits would bypass the DOM budgets unless routed through `Dom`. The previously assumed "no CSSOM" gap is smaller than documented (§1), which lowers D3's schedule risk for rule listing and rule editing but not for matched rules. The always-on console cost is understood (§2) and should be explicitly accepted or reduced before D3 adds engine hooks.
