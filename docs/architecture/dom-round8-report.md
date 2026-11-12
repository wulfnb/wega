# Round 8 report: event routing, focus, input and document lifecycle

Scope: Wega-owned boundaries for native input routing, focus/hover state and document construction, preserving behaviour; concrete correctness bugs found on the way are fixed and listed separately. No push. No CSS/layout/render work, no JavaScript event-system rewrite. The design and audit were committed before implementation: [`dom-round8-design.md`](./dom-round8-design.md).

**Outcome:** a real input path (pointer, keyboard, accessibility focus, `element.focus()`/`blur()`) now runs through Wega-owned boundaries; focus has one explicit owner; eight pre-existing bugs are fixed with tests; document construction moved into `wega-adapter-blitz`. No new crate, no new unsafe, no external dependency change, no graph change.

## 1. Event architecture: before and after

### Who actually owned what (audit, exact locations)

| Concern | Owner | Evidence |
|---|---|---|
| Listener registration, capture/target/bubble, `once`, `passive`, `stopPropagation`, `stopImmediatePropagation`, `preventDefault`, `composedPath`, retargeting, `isTrusted` storage, composed/shadow-including paths | **Deno `deno_web` `02_event.js`** (`Event`, `EventTarget`, `dispatch`). Wega's `Node extends EventTarget` supplies `parentNode`, `getRootNode`, shadow host and calls `setEventTargetData`. | `semantics.js:18` selects `__wega_web`; `:578`; `:4736` |
| JS fallback dispatcher (`FallbackEvent`, `FallbackEventTarget`, ~120 lines) | Wega, **dead code**: only used when `__wega_web` is absent, which no engine configuration produces | `semantics.js:141-265`, chosen at `:266` |
| Creating events for native input | Wega JS `__wega_native_event` (`observers.js`) | |
| Rust → JS routing | Wega, previously **11 hand-formatted strings** in `view.rs` (+2 `__wega_submit_event`) | |
| Hit-testing, hover, focus state, active/mousedown node, selection, caret | **Blitz** (`set_hover_to`, `focus_node_id`, `active_node`, …) | `view.rs`, `dom.rs` |
| Focus/blur events | Blitz generated them for click/Tab; JS `focus()`/`blur()` dispatched partial events of their own | probe |
| Default actions | Mixed: Wega `default_click`, `key` (submit/reset/file/new window/fragment, implicit submission, clipboard, checkbox toggle), then Blitz `handle_dom_event` (text editing, selection, click synthesis) | `view.rs`, `document_host.rs` |
| Shadow DOM metadata | Wega `components.rs` + `dom.rs` ops | |
| Document replacement | Wega `view.rs::set_document` | |

Call counts (engine sources, before → after): `__wega_native_event` format sites 11 → 0 (one `NativeEvent::script`); `get_hover_node_id`/`set_hover_to`/`clear_hover` 4 → 0; `get_focussed_node_id` 3 → 0; `set_focus_to` 2 → 0; `node_chain` 1 → 0; direct `DocumentConfig`/`HtmlDocument`/`BaseDocument::new` 3 → 0. Blitz's `handle_dom_event` (5 sites) and `active_node`/`unactive_node`/`set_mousedown_node_id` (3) remain by design (default-action machinery).

### Separation of the three concerns (as designed)

1. **Dispatch semantics:** unchanged, owned by deno_web. No Rust re-implementation, no contract.
2. **Native input target selection and focus/hover state:** new `wega_dom::input` contract (below).
3. **Default actions:** unchanged and engine-side (plus Blitz's editing defaults); documented, deliberately not abstracted.

## 2. Contracts introduced

- **`wega_dom::input`** (in `wega-dom`; **no new crate** — dependents are exactly the engine and the Blitz adapter, both already depending on `wega-dom`, so a `wega-events` crate would only add a manifest and an edge):
  - `FocusState`: `focused()` (explicit focus only), `keyboard_target()` (focused, else the root element), `hovered()`.
  - `InputControl`: `set_focus(key) -> Result<bool, MutationError>`, `clear_focus()`, `pointer_moved(x, y) -> HoverChange`, `pointer_left() -> HoverChange`.
  - `element_chain(doc, key)` (hover/enter/leave chain, replaces Blitz's `node_chain`).
  - `NodeKey` only; stale keys rejected; no Blitz event type; no allocation except the chain `Vec`.
  - Implemented once, by `BlitzDocument`. Blitz's root-element fallback is exposed as `keyboard_target`, not as focus.
- **`NativeEvent`** (engine, `native_event.rs`): `kind`, `NodeKey` target, init, `related`; one `script()` builder and `script_all`. Engine-only because its only consumer is the engine's JS bridge.
- **Focus transition** (engine): after any operation that can move focus (pointer, key, accessibility focus, `element.click()` activation) the engine compares `focused()` before/after and calls the **single** JS definition `__wega_focus_events(from, to)`; `element.focus()`/`blur()` call the same function. Blitz's own focus events are no longer forwarded, so nothing is dispatched twice.
- **Ownership of focus:** the document (Blitz). Wega reads/writes it only through the contract; the JS-side copy `_active` was removed; no second authoritative state exists. Hover, active/mousedown and selection stay Blitz-owned.
- **Deferred, with reasons:** an `EventPath`/`EventRoute` type (dispatch is deno_web's; a Rust path would duplicate it); `Window`/`Document` as native event targets (no Rust native input targets them today); pointer capture (the API does not exist); moving `active_node`/click synthesis into the contract (single implementation, Blitz-internal).

## 3. Behaviour migrated and bugs fixed

Migrated without observable change (probe output identical before/after): mouse click targeting, hover enter/leave chain, hover change detection, keyboard target selection, accessibility focus, `element.focus()` state change.

**Pre-existing bugs found and fixed (each tested in `tests/events.rs`):**

| # | Bug (existing before this round) | Fix |
|---|---|---|
| 1 | Every native event had `isTrusted === false` | marked trusted via Deno's `setIsTrusted`; script-created events stay untrusted (commit `221a0fa`) |
| 2 | Focus events had `relatedTarget === null` | related nodes set |
| 3 | `element.focus()` dispatched only `focus`: no `blur`/`focusout` on the old element, no `focusin` | one shared transition; nothing when already focused |
| 4 | `element.blur()` left `document.activeElement` on the element | clears focus, emits `blur`/`focusout` |
| 5 | `document.activeElement` was `<html>` with nothing focused | `<body>` (spec) |
| 6 | Key events with nothing focused targeted `<html>` | target `<body>` (Blitz's editing defaults keep using the root fallback) |
| 7 | `document.activeElement` returned the inner element for focus inside a shadow root; `shadowRoot.activeElement` did not exist | spec retargeting: host from the document, inner node from the root |
| 8 | A native event aimed at a dropped node threw `Node no longer exists` (script error, event lost) | `__wega_node` returns null for that error; the event is ignored |

Bugs 2–8 are in commit `c777548`. Pinned, not changed: clicking a non-focusable element clears focus (Blitz) and is reported as `blur`/`focusout` (test `clicking_a_non_focusable_element_blurs_the_focused_one`). An earlier draft of the design doc listed this as a gap; that came from a mis-aimed probe click and was **wrong**, so the design doc was corrected. Whether the blur was reported before this round was not checked against the old build.

Not changed (kept observable behaviour): `element.focus()` still focuses any element (the spec makes non-focusable elements a no-op); Blitz default actions and ordering for click/Tab are as before (mousedown, then focus events, then click).

## 4. Shadow DOM findings

Verified by tests that pass (`tests/events.rs`): composed events cross the shadow boundary with the host as retargeted target (at-target phase at the host); non-composed events stop at the shadow root; the composed path includes the slot and shadow nodes for open roots and hides them for closed roots; slotted elements' events pass through the slot; `host.shadowRoot`, `root.host`, `getRootNode()` identity; `focusin` crosses the boundary with correct targets; `document.activeElement`/`shadowRoot.activeElement` retargeting; detached trees dispatch only within themselves.

**Not claimed / known gaps (checked in source):** `delegatesFocus` is stored and reported but no code acts on it (no focus delegation); there is no `slotchange` event; no pointer-capture API (`setPointerCapture` and friends do not exist); `focus()` on non-focusable elements is not a no-op; `tabindex`-order Tab navigation beyond Blitz's default focus order is untested; Shadow DOM event conformance in general is **not** claimed beyond the listed tests. These belong on the browser feature checklist.

## 5. Document lifecycle

- Construction moved into the adapter: `DocumentSetup` (base URL, brokered fonts, abort signal, net/navigation providers) and `BlitzDocument::{from_html, from_xml, empty}`. The engine no longer builds a `DocumentConfig` or constructs `HtmlDocument`/`BaseDocument` (3 → 0 sites). The adapter now depends on `blitz-html` and `blitz-traits` (both already locked); one-document ownership is unchanged. Viewport and shell providers are not part of construction: the engine never set them in the document config, so they keep Blitz's defaults.
- Replacement semantics pinned by tests: a new document starts with focus reset (`activeElement` is `<body>`), no listeners or script state from the old document, input routing works on the new one; navigation, reload (a fresh document, one keydown per press, not two) and error-page replacement; replacing a document mid-parse drops the parser, its pending script and blocking state.
- `Deref`/`DerefMut` stays: layout, paint, style and form code still reach `BaseDocument` through it. No further narrowing was justified this round.
- Round 6's recursion guard for an over-budget error page still holds (existing pages test).

## 6. Direct Blitz references (same grep as Rounds 6 and 7)

Pattern: `get_node(`, `get_node_mut(`, `element_data()`, `NodeData::`, `.mutate()`, `blitz_dom::`, `blitz_html::`, engine `src/` lines: **Round 6: 149, Round 7: 100, Round 8: 96** (`dom.rs` 58, `view.rs` 31, `document_host.rs` 5, `network.rs` 1, `script.rs` 1). The drop is small because this round moved *state* access (hover, focus, construction), not the layout/style/form reads that dominate the remaining lines. `BaseDocument`/`DocumentConfig`/`HtmlDocument` are named in 1 engine line (was 3 construction sites plus type mentions).

## 7. Dependency graph

Before (Round 7) and after: **identical** — 13 members, 24 internal edges, no cycles, architecture checker 0 violations (`baseline/architecture-check-after-round8.md`, `baseline/dependency-graph-after-round8.md`). `engines/Cargo.lock`: +1 line (the internal edge `wega-adapter-blitz → blitz-traits`); no external package added, removed or re-versioned; no new unsafe (`#![forbid(unsafe_code)]` on the three crates; the engine's new `native_event.rs` has none). The boundary tests of the three crates pass with the updated adapter allow-list.

## 8. Performance

Debug builds, baseline = Round 7 end (`4ae5c65`) built in an isolated worktree and target directory (removed afterwards), current = this branch; identical benchmark file (`tests/hot_path_bench.rs`, new `event_paths` benchmark added). **ABBA ordering (new, old, old, new), 6 samples per build, median; machine load average ≈ 10 (other processes), so this is not a quiet-machine measurement.** Last column: spread (max−min)/median of the old / new samples.

| Event, input and lifecycle path | Old | New | Change | Spread old / new |
|---|---|---|---|---|
| dispatch, no listeners (µs/event) | 568.8 | 570.7 | +0.3% | 2% / 11% |
| dispatch, 6 listeners over 3 targets (µs) | 744.6 | 754.0 | +1.3% | 2% / 14% |
| dispatch through a 30-deep tree, 60 listeners (µs) | 20,175 | 19,828 | −1.7% | 6% / 21% |
| native click: move+down+up, 2 targets (µs) | 14,525 | 13,538 | −6.8% | 2% / 15% |
| native pointer move between 2 targets (µs) | 7,988 | 7,959 | −0.4% | 4% / 16% |
| native keydown to the focused input (µs) | 710.8 | 677.1 | −4.7% | 3% / 16% |
| `element.focus()` transitions, script (µs) | 1,094.6 | 3,593.2 | **+228%** | 7% / 17% |
| native click focus transitions (µs) | 15,129 | 15,559 | +2.8% | 4% / 17% |
| `set_html` document replacement (ms) | 124.5 | 124.5 | 0.0% | 1% / 10% |
| `create_view` construction (ms) | 83.7 | 83.6 | −0.1% | 9% / 11% |

The one real difference is script `element.focus()`: it now dispatches the correct `blur`, `focusout`, `focus`, `focusin` (four events with `relatedTarget`) where it used to dispatch a single `focus`; the cost tracks the event count, not the contract. Native click focus transitions, which already produced four events through Blitz, are unchanged. Dispatch itself is unchanged (deno_web) and is slow in a debug build for deep trees (each path step crosses the host bridge); that is pre-existing and was not optimised.

Regression guard on the untouched DOM paths (`hot_paths`, same method, 6 samples): every difference is within the noise floor (−6.5% … +5.9%, spread 13–26% under this load), and `evaluate_json(1+1)` is +1.0% (14.03 → 14.16 µs).

**A regression found and fixed during measurement.** The first A/B run showed `evaluate_json(1+1)` +14–18% on both of two data sets with a 3–4% spread. Cause: `drain_requests` sampled the focused node on every call to support `element.click()` activation. Fixed in `fa1a5c5` (sample only when there are activations); the figure above is after the fix. The same first run also showed +7…+37% on other DOM paths from only two samples with the new build always first; the swapped-order re-run showed that was an ordering/load artifact (−3%…+4%).

Allocations: not measured (no allocator instrumentation in this repository's test setup). By inspection the new code allocates one `String` per native event (the same as the formatted strings it replaced) and one `Vec` for the hover chain (which replaced Blitz's `node_chain` `Vec`); no per-event allocation was added to dispatch.

## 9. Validation

- Full engines workspace suite: **433 passed, 0 failed, 7 ignored** (406 before; +27: 20 event/lifecycle tests, 5 input-contract tests, 2 routing tests; the extra ignored test is the new `event_paths` benchmark).
- Broker mode (`WEGA_TEST_BROKER=1`): `security`, `forms`, `scripts`, `documents`, `history`, `events`, `input` (`--test-threads=1`) plus `lib`, `dom`, `dom_identity`, `pages`: all pass. Required-worker test (`inherited_broker::a_required_worker…`) with a CI-style content helper from `wega-content-bundle`: passes.
- DOM, JavaScript event, input/focus, Shadow DOM and lifecycle tests: `dom`, `dom_identity`, `input`, `forms`, `events` (propagation, stop*, once, passive, cancelable, native flags, click/key targets, focus order, Shadow DOM, identity, replacement, navigation, reload, error page, parser teardown), adapter `input` and `contract`, `native_event` unit tests.
- Root `cargo check --locked`; `cargo metadata --locked` for the root, engines and tools workspaces; `cargo fmt --check`; `clippy -D warnings` for the engines workspace and the tools workspace; architecture checker (0 violations) and its 8 tests; boundary tests of `wega-dom`, `wega-html`, `wega-adapter-blitz`; tools workspace tests (14).
- No dependency cycles (graph identical to Round 7). No external dependency change: `engines/Cargo.lock` +1 line (internal edge `wega-adapter-blitz → blitz-traits`). No new unsafe. Generated bindings not stale (`build.rs` verifies them on every build; only hand-written `semantics.js`/`observers.js` changed). No removed or renamed public items in the engine or the three crates (diff scan).
- **Not run:** Windows and Linux builds, MSVC Skia jobs, native-sandbox ignored tests. All results are macOS only.


## 10. Existing browser-spec gaps (not addressed)

Pointer capture; `delegatesFocus`; `slotchange`; `focus()` no-op for non-focusable elements; `tabIndex` semantics; the dead JS fallback dispatcher (`semantics.js:141-265`) should be deleted once confirmed unused by workers; `Window`/`Document` as native input targets; pointer events beyond the mouse path (touch/pen); event dispatch through a deep tree is slow (each path step crosses the host bridge: 24 ms for a 30-deep tree in a debug build, below).

## 11. Remaining blockers and next steps

1. Click synthesis (`active_node`, `set_mousedown_node_id`, `handle_dom_event`) and the default actions still go through Blitz; they need a default-action contract before the engine can drop `blitz-traits` event types.
2. `view.rs` still imports Blitz event types (`BlitzPointerEvent`, `DomEvent`, `BlitzKeyEvent`, `BlitzImeEvent`); an engine-owned input event type would remove them.
3. Layout geometry, style blocks and form state (the bulk of the remaining 96 lines) are the next boundaries, in that order of consumer count.
4. Re-run the benchmarks on an idle machine.
5. Environment: `target/wega-engine/debug/deps` had grown to 35 GB of stale test binaries (disk was at 100% in Round 7); this round pruned only this project's own gitignored workspace artifacts (`cargo clean -p wega-engine -p wega-adapter-blitz -p wega-html -p wega-dom`, 31 GB). Third-party build caches and other worktrees were not touched.

## 12. Commits (local, not pushed)

`0406e51` audit and design; `9db9112` `wega_dom::input` contract and Blitz implementation; `3f55e74` native event routing type; `6162279` engine reads/changes focus and hover through the contract; `221a0fa` trusted native events; `c777548` focus transitions and the listed bugs; `22dbc5a` document construction in the adapter; `6cab08f` benchmarks; `fa1a5c5` activation-path focus sampling fix; (this commit) report.
