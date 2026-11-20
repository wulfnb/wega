# Post-Round-12 roadmap

Dependency-aware, evidence-based; every item cites the audit that motivates it
([`browser-feature-completion-audit.md`](./browser-feature-completion-audit.md),
[`blitz-retirement-readiness.md`](./blitz-retirement-readiness.md),
[`dom-round12-report.md`](./dom-round12-report.md)). Nothing here is started. It is a proposal for the next implementation phase, not a commitment, and it does not promise Chrome compatibility.

**Ordering principles.** (1) Close security and reliability holes that a hostile page can hit today. (2) Build the infrastructure that makes later work measurable. (3) Deliver the user-visible features whose absence blocks the most sites. (4) Standards coverage grows with (2) and (3). (5) Vendor replacement last: Blitz is not retired before the features it hosts exist, because replacing an engine that is missing iframes, forms contracts and a layout watchdog only moves the gaps.

Effort scale: S ≤ 1 week, M 1–4 weeks, L 1–3 months, XL > 3 months (one engineer, rough). Perf risk: low / medium / high.

## Critical blockers (do first)

| # | Item | Why (evidence) | Depends on | Effort | Perf risk | Acceptance criteria |
|---|---|---|---|---|---|---|
| B1 | **Layout watchdog and the nested-block hang** | ≥ 122 nested blocks never finish layout (121: 22 ms); the script watchdog does not cover layout/paint, so a ~1 KB page hangs a tab's engine process. | none | M | low | (a) Root-cause the Taffy/Blitz recursion (cache miss at depth ≈ 122, vendored `blitz-dom/src/layout`); (b) `reliability.rs::known_gap_nesting_of_122_blocks…` flips to "finishes"; nesting of 1000 lays out in < 200 ms or fails closed with an error; (c) an engine-process liveness watchdog covering layout and paint, with a parent-side kill and a test where a hung layout is terminated within a stated deadline; (d) a negative-test corpus of hostile structures (deep nesting, huge tables, pathological flex/grid) with time budgets. |
| B2 | **Frame-level and navigation CSP** *(`frame-src`, `frame-ancestors` and `X-Frame-Options` delivered by the iframe milestone; `form-action`, `base-uri`, `sandbox` directive, reporting remain)* | `form-action`, `base-uri`, `frame-ancestors`, `frame-src`, `sandbox`, reporting are absent (no mitigation for form hijack or `<base>` injection). | none for `form-action`/`base-uri`/`upgrade-insecure-requests`; frames for `frame-ancestors`/`frame-src` | M | low | Directives parsed and enforced at the navigation/form/base sites with tests per directive and per fallback rule; `report-uri`/`report-to` delivery with a bounded queue; `docs/pending-features.md` updated; unknown-extension subresource classification replaced by request destination. |
| B3 | **Cross-origin isolation and embedding headers** | No COOP/COEP/CORP/X-Frame-Options handling; `crossOriginIsolated` hard-coded false. | B2 for XFO parity; frames for COOP browsing-context groups | M | low | CORP enforced on subresource fetches; XFO honoured with the same code path as `frame-ancestors`; COOP/COEP parsed and exposed as `crossOriginIsolated`; `SharedArrayBuffer` gated on it; WPT-style tests for each. |
| B4 | **Third-party cookie / storage partitioning** | The jar is per profile; `StorageKey.top_level_site` exists with no caller. | none | M–L | low | `StorageKey` consumed by cookies, storage, cache and workers; third-party cookie policy (block by default or partition) chosen and documented; negative tests for cross-site reads; no regression in containers tests. |
| B5 | **Unsafe-code and sandbox evidence** | Windows FFI files carry 55 + 19 + 16 `unsafe` with no SAFETY comments (some parse cross-process data); native sandbox probes have never run in this audit. | a Windows and a Linux host | M | none | Every `unsafe` outside vendor has a SAFETY justification or is removed; a `#![deny(clippy::undocumented_unsafe_blocks)]` gate on the platform and UI crates; the native denial probe, required-worker test and broker tests run and recorded on Linux and Windows (see "Cross-platform"). |
| B6 | **Bound the worker queues** | Worker command channels (`mpsc::channel()`) and `MessagePort` queues are unbounded. | none | S | low | Bounded channels with a defined overflow behaviour; a flood test (`postMessage` ×10⁶ within the deadline) shows bounded memory. |
| B7 | **Idle repaint** | Every `poll()` re-dirties the view; settled pages repaint at the frame cap (Round 11). | Canvas/WebGL/media/resource producers must invalidate explicitly | M | low (improves) | The pinned test `known_gap_every_poll_marks_a_settled_page_dirty_again` flips; canvas, WebGL, video, image load and stylesheet arrival still repaint (a test each); idle CPU of a static page measured at ≈ 0. |

## High-value missing features

Ordered by what blocks the most real sites, then by dependency.

| # | Item | Why | Depends on | Effort | Perf risk | Acceptance criteria |
|---|---|---|---|---|---|---|
| F1 | **Iframe rendering and frame contexts** *(partially delivered by the iframe milestone: see `iframe-implementation-report.md`; same-origin DOM bridge, process isolation, z-order, joint history and host UI remain)* | `contentWindow`/`contentDocument` are null and nothing renders; many sites embed content; B2/B3 enforcement and most WPT frame tests need it. | B1 (a second layout tree widens the hang surface), the existing protocol frame tree | XL | high | A same-origin and a cross-origin iframe load, lay out, paint and receive events; sandbox flags enforced (forms, scripts, popups, same-origin); `postMessage` between frames; `frame-ancestors`/`frame-src` enforced; one engine realm per frame with the same budgets; the `frames.rs` "no document" assertions are replaced by behavioural tests. |
| F2 | **IndexedDB JavaScript binding** | The storage service and its crash-recovery tests exist; pages cannot use it. | the existing service | M | low | `indexedDB` exposed with `IDBFactory`/`IDBDatabase`/`IDBTransaction`/`IDBObjectStore`/cursors/indices; origin/partition isolation reusing the storage key; WPT IndexedDB subset passes; quota errors surfaced. |
| F3 | **Shadow DOM style, layout and paint** | Shadow trees are detached fragments; web components render nothing in the shadow tree. | Style/selector contract extension (selectors), flattened-tree support in the document host | L | medium | `:host`, `:host-context`, `::slotted`, scoped selectors, inheritance into the tree, adopted stylesheets, flattened layout and paint; the known-gap tests in `style.rs`, `layout.rs`, `render.rs` flip; a conformance subset (CSS Scoping) passes. |
| F4 | **Media decoding and audio output** | Playback is a clock model; no decode, no sound. | platform media backend (the packaging already pins GStreamer for Windows in the UI process); permission/broker policy | XL | high | MP4/H.264, WebM/VP9/Opus, AAC, MP3 decode; audio output; `<video>` frames painted; A/V sync within a stated tolerance; MediaSource as a separate milestone; media read through the broker, not local files. |
| F5 | **Editing and form controls** | `execCommand` has 4 commands; no dialog/details/select tests; customized built-ins and `ElementInternals` missing. | the forms contract (A3) | L | low | `contenteditable` editing, `execCommand` core set, `beforeinput`/`input` events with `inputType`, selection/Range tests, `<dialog>` (`showModal`, top layer), `<details>`, `<select>`, `ElementInternals`; WPT editing/forms subsets. |
| F6 | **WebGL real rendering / WebGPU** | WebGL is a state machine, WebGPU absent. | GPU path decision (CPU Skia only in the engine today) | XL | high | Shaders executed (software or GPU) with conformance tests; explicitly staged: WebGL1 before WebGL2 before WebGPU. |
| F7 | **Device APIs: camera/microphone, WebRTC, Web Audio** | Permission prompts front simulated devices. | platform capture backends, F4 audio output | XL | medium | Real capture behind the existing permission flow, `enumerateDevices` truthful; until then prompts must not claim a device that does not exist. |
| F8 | **Browser services**: passwords/autofill, passkeys, printing, extensions, sync, DRM | All category 4. | secure credential store, platform integrations, B5 | XL each | low | Each needs its own design document and threat model first; none should start before B1–B6. |
| F9 | **Service Worker and shared-worker completeness** | `Clients.navigate`, push/sync missing; one shared-worker test. | F1 for client navigation | M | low | Spec features chosen explicitly; WPT service-worker subset; shared-worker lifecycle tests. |

## Standards and test infrastructure

| # | Item | Evidence | Effort | Acceptance criteria |
|---|---|---|---|---|
| S1 | **Expand the WPT runner** | Pinned subset: 35 DOM files (1685 subtests), 2 CSS files + 1 reftest; no `promise_test` use, no `step_timeout`, no iframes, no workers, no testdriver, no generic reftest runner, no expectations manifest; the upstream checkout is absent. | M | A vendored or fetched-by-hash WPT checkout; an **expectations manifest** (per-test expected status with a reason) so failures are recorded, not excluded; `promise_test`, `step_timeout`, reftest (`link rel=match`) support; per-area counts reported; a ratchet that fails on a new failure. |
| S2 | **Grow coverage by area** | Not covered by any WPT: navigation/history, storage, security, accessibility, fetch, workers, layout. | L | Each area gets a pinned subset with recorded pass/fail counts; failing tests stay in the manifest with reasons. |
| S3 | **Cross-platform runs of the suite** | Only macOS was run in this audit; CI is described but its results were not examined. | M | The engine suite, broker mode, required-worker and native sandbox probes executed on Linux and Windows with logs archived; any platform not run is labelled so. |
| S4 | **Accessibility tests** | No engine-level test of the page tree or ARIA mapping. | M | Tree snapshots for a fixture corpus; ARIA role/name tests; precedes any accessibility contract work (A5). |

## Architecture and vendor replacement (after the above)

Ordered by effort-to-benefit; each step removes `Deref` surface (149 errors today) and adds a contract that is exercised.

| # | Item | Removes | Effort | Acceptance criteria |
|---|---|---|---|---|
| A1 | `DocumentView::{url, base_url}` and document lifecycle accessors | ≈ 51 `Deref` sites | S | `url`/`set_base_url` behind the contract; `Deref` errors drop by ≥ 50. |
| A2 | Node data and serialisation (`text`, `outer_html`, text writes, subtree iteration); engine switches to `NodeKey` | ≈ 55 sites + `NodeData` | M | No `NodeData`/`TreeTraverser` in the engine; DOM and WPT subsets unchanged. |
| A3 | **Forms contract** (value, checked, files, selection, submit, radio groups) | `submit_form`, `toggle_radio`, `SpecialElementData` | M–L | Form state behind a trait with tests; prerequisite for F5. |
| A4 | Selector contract (query, matches, closest) | 15 sites | M | Selector queries behind a trait; needed by F3. |
| A5 | Accessibility contract with tests (S4 first) | `build_accessibility_tree` | M | Page tree built from `NodeView` plus computed style, parity with Blitz's tree on the corpus. |
| A6 | Wega-owned input event types and a default-action contract | the Blitz event types in `View::key/ime` and `pub use blitz_traits::events`; `handle_dom_event` | L | No `blitz_traits` type in the public engine API; editing/selection default actions behind a contract. |
| A7 | Resource-delivery interface (images, stylesheets, fonts) replacing the Blitz `NetProvider`/`NavigationProvider` implementations in `view.rs` | `blitz_traits::net`/`navigation`, `install_broker_fonts` | L | Subresources flow through a Wega interface where CSP/CORS/broker policy is applied once. |
| A8 | XML tree building via `Mutator` | `from_xml` | M | XML parity tests (with the WPT XHTML files currently excluded). |
| A9 | Remove `Deref` from `BlitzDocument` | everything above | S (once A1–A8 land) | The engine compiles with `Deref` removed (it fails with 149 errors today). |
| A10 | Decide Blitz retirement **with data** | — | — | Only after A9, F1/F3 and B1: a second backend (or a Stylo-hosting adapter) passes the contract tests (`adapters/blitz/tests/*`) and the 42-frame pixel corpus before any swap is considered. |

Also tracked, lower priority: pooled or resizable Skia renderer (resize ≈ 20× the steady cost, Round 11), scene reuse and damage tracking, worker message latency (≈ 6 ms round trip in release, consistent with the 4 ms poll tick in `workers/runtime.rs`), deduplicating `selectors`/`cssparser`/`servo_arc` versions when Stylo moves, the stale `docs/containers.md`.

## Required infrastructure

- **Benchmarks in CI with a gate.** `scripts/engine-bench.py` (added this round) records environment, repeats and spread; no gate exists yet. Acceptance: a nightly job on a pinned machine class compares against `docs/architecture/baseline/engine-bench-round12.json` and fails on significant regressions (beyond spread).
- **Startup, navigation, JavaScript throughput and multi-tab interaction benchmarks.** Missing today (only a call-overhead and an idle-resource benchmark exist; `engines/tools/memory-benchmark` and `build-benchmark` are unimplemented READMEs).
- **Fuzzing.** No fuzz targets were found; the HTML parser, CSS parser (via Stylo), IPC packets and the CSP parser are the first candidates.
- **A hostile-page corpus** (deep nesting, huge tables, event/message floods, memory growth) run with time and memory budgets: the successor of `reliability.rs`.
- **Crash recovery tests** that kill the content process during load, layout, paint and worker execution and assert the browser recovers.

## Cross-platform gaps

| Area | State | Needed |
|---|---|---|
| macOS | The only platform run in this audit (Apple M4). | — |
| Linux | CI job described (`native` job; Docker integration); results not examined; native sandbox needs Landlock ABI ≥ 6. | Run and archive the suite, broker, required-worker and sandbox probe on a supported kernel; record behaviour on older kernels (fail-closed). |
| Windows | CI job described; MSVC Skia cached build; AppContainer + job object. 74 `unsafe` occurrences without SAFETY comments in the platform Windows files (`windows_spawn.rs` 55, `ipc_windows.rs` 19), plus 16 in the UI crate. | Run and archive; SAFETY review (B5). |
| GPU rendering | No GPU path exists in the engine (CPU Skia only). | Decide the GPU strategy before F6; the UI process surface is outside the engines workspace. |

## Suggested sequencing

1. **Phase A (security and reliability):** B1, B6, B7, B2 (non-frame directives), B5, S3 in parallel with S1.
2. **Phase B (isolation and storage):** B4, B3, F2, A1–A2.
3. **Phase C (frames and components):** F1 (with B2/B3 frame parts), F3 with A4, F9, S2.
4. **Phase D (rich content):** F5 with A3, F4, then F6/F7 as separate programmes; F8 items each after a design review.
5. **Phase E (vendor):** A5–A9 interleaved with the above as capacity allows; A10 only after Phase C.

Nothing in Phase A requires replacing Blitz, and Phase E should not start before iframes (F1) and the layout watchdog (B1) exist.
