# Round 12 report: architecture consolidation and browser compatibility audit

Scope: consolidate and audit; not a promise of Chrome compatibility or of Blitz removal. Local only, not pushed. Branch `feature/dom-rounds-9-12` (the branch named in the task text was already merged into `main`). Rounds 9, 10 and 11 were verified complete and committed before starting (`git log`: `90aa968`…`811baf8`; their reports exist and their suites passed at the time).

Companion documents:
[`blitz-retirement-readiness.md`](./blitz-retirement-readiness.md) (exact Blitz inventory),
[`browser-feature-completion-audit.md`](./browser-feature-completion-audit.md) (feature and security classification),
[`post-round12-roadmap.md`](./post-round12-roadmap.md) (prioritised next phase).

**How to read this report.** It separates five things the earlier rounds sometimes blurred: **architecture completed**, **behaviour implemented**, **standards conformance verified**, **platform acceptance completed**, **still unknown or untested** (§1). Evidence kinds are labelled: *run* (executed in this round), *read* (source review, including a delegated read-only review whose key claims were re-verified by the author, marked ✔), *not run*.

## 0. Baseline (recorded at the start)

- Branch `feature/dom-rounds-9-12`, HEAD `811baf8`, working tree clean, 11 GiB free (the disk was repeatedly near full; see §7), load average 3 at start and 8–10 during benchmarks.
- Engines workspace: 13 members, 24 internal edges, 0 checker violations (unchanged since Round 8). No dependency cycles.
- Tests at start (Round 11): 506 passed, 0 failed, 11 ignored.
- Platform: macOS only (Apple M4, 10 cores, 16 GiB, release/dev profiles).

## 1. Status at a glance

| Question | Status |
|---|---|
| **Architecture completed** | DOM read/mutation, HTML tree building, style, input focus/hover, layout/geometry/hit-test and paint recording are behind Wega-owned contracts, implemented once (by `BlitzDocument`) and exercised. Not completed: forms, selectors, default actions, accessibility, XML, resource delivery and the `Deref` to Blitz's document (149 uses, see §3). No contract has a second backend. |
| **Behaviour implemented** | See the feature audit: 5 areas in category 1 (dedicated workers, Cache API, Canvas 2D, storage, permissions/notifications, DOM-level Shadow DOM, CSP script/connect/eval); many partial; **category 4 (absent)**: iframe rendering, IndexedDB for pages, Shadow DOM style/layout, WebGPU, WebRTC, Web Audio, password manager, passkeys, printing, extensions, sync, DRM, frame-level CSP, cross-origin isolation. **Category 3 (simulation)**: WebGL rendering, camera/microphone, media frames. |
| **Standards conformance verified** | Only the pinned subset: 35 DOM WPT files **1685/1685** subtests, 2 CSS WPT files **7/7** + 1 reftest, 7 framework fixtures, 6 harness fixtures (all *run*, all pass). No navigation, storage, security, accessibility, fetch, worker or layout WPT area exists. The upstream WPT checkout is absent; there is no expectations manifest. |
| **Platform acceptance completed** | macOS only: engine suite, broker mode, required-worker test. **Not tested here:** Linux, Windows, MSVC Skia, the native-sandbox probes on any OS, packaged builds, GPU paths (none exist in the engine), the browser UI suites. |
| **Still unknown** | Whether CI passes on Linux/Windows (workflows were *read*, not run); the browser UI crate's own tests; engine-side `javascript:` URL handling; impact of the unbounded worker queues; Web Crypto and Intl completeness; accessibility tree correctness; startup, navigation and multi-tab behaviour (no benchmark). |

## 2. Consolidation findings (Task 1)

Audited every `wega-*` crate (13 workspace members) and the vendored engines.

| Check | Result |
|---|---|
| Dependency cycles | None (`cargo metadata` + checker, run). 24 internal edges. |
| Unused dependencies | **Five found and removed** from `wega-engine`: `blitz-html`, `cssparser`, `style_traits`, `cookie_store`, `publicsuffix` (no references in its sources, tests or build script; all still used by other crates). `engines/Cargo.lock`: −5 edges, **no package added or removed**. The other 12 crates have none (heuristic: dependency name absent from `src/`, `tests/`, `build.rs`). |
| Dead code | One dead file removed: `engines/wega/src/public_suffix_list.dat` (230 KB, byte-identical to `services/cookies/src/`; no reference in any source or manifest, as noted in the earlier crate plan). Round 9 and 11 already removed `remove_style_property` and the engine's `blitz-paint`/`anyrender` use. No compiler or clippy warnings remain (`-D warnings`, run). |
| Placeholder directories | About **60** directories under `engines/` contain only a README (`css/*`, `foundation/*`, `graphics/*`, `layout/*`, `javascript/*`, `runtime/*`, `webapi/*`, `services/{bookmarks,downloads,history,http,navigation}`, `adapters/stylo`, and so on). They are planned-structure documentation, referenced by `module-map.md`; **not deleted** (not dead code, and removing them would be an unrelated refactor). `adapters/stylo` is correctly absent as a crate (Stylo is not separable from Blitz). `tools/memory-benchmark` and `tools/build-benchmark` are unimplemented READMEs. |
| Unused adapters | None: `wega-adapter-{blitz,skia,v8}` all have consumers. |
| Contract leaks | **Blitz types in the public engine API:** `View::key(BlitzKeyEvent)`, `View::ime(BlitzImeEvent)`, `pub use blitz_traits::events`, `pub use blitz_traits::shell::{ColorScheme, Viewport}` (kept for compatibility). Blitz's `NodeId` appears in 78 engine places. Blitz's `NetProvider`/`NavigationProvider` traits are implemented in `view.rs`. `wega-dom` names no Blitz type (boundary test, run). |
| Duplicate state ownership | None found: focus, hover, style, layout and paint state each have one owner (Rounds 8–11). The engine's `layout_dirty` gate and the `FrameState` flag are gates, not copies of Blitz's damage state. |
| Excessive abstraction / dynamic dispatch | The contract traits use static dispatch and `wega-dom` has **0** `dyn`. Remaining `dyn` sites (counts, *read* not individually audited): engine 25, V8 adapter 34, platform 6, Blitz adapter 2, Skia adapter 1, network 1 — wake callbacks, handler registries and third-party trait objects, none on a measured hot path. No trait was found with a single implementation and no consumer that would justify removal; the contracts' single implementation is deliberate (§3). |
| Unnecessary allocations | Per frame: one `Scene` (moved, not copied) and, for owned frames, one pixel buffer; the shared target writes in place (Round 11). Known cost: the Skia renderer is rebuilt on every size change (≈ 20× steady frame cost). Not changed. |
| Unbounded recursion / traversal | **Found:** layout of ≥ 122 nested blocks does not finish (§5). A JS `firstElementChild` walk is quadratic in depth (600 deep: 190 ms). Parser has a node budget; DOM budgets exist; no explicit depth limit in serialisers (not exercised beyond depth 600). |
| Panic risk | Non-test `unwrap/expect/panic` counts: engine 26 + 2, V8 adapter 28, network 7 + 2, platform 3, protocol 3, parser 1, cookies 1 (script over crate sources, test modules excluded; *run*). The seven `get_node(node_id()?).unwrap()` DOM-host sites that review flagged as page-reachable are guarded by `node_id()`'s liveness check ✔ (not defects). Five `policy.read().unwrap()` sites panic on a poisoned lock (low). |
| Unsafe code boundaries | No new `unsafe` this round or in Rounds 9–12 (diff grep: 0). Outside vendor there are 221 occurrences in 35 files by grep (including false positives such as the string `'unsafe-inline'`): `windows_spawn.rs` 55, `ipc_windows.rs` 19 and the platform Windows files carry **no SAFETY comments**; the UI crate's FFI files (`native_*.rs`, `windows_instance.rs` (cross-process `COPYDATASTRUCT` parsing), `mac_application.rs`) mostly uncommented; `wega-security` forbids unsafe. Recorded as roadmap item B5; not edited (reviewing FFI on a host that cannot build it would be guesswork). |
| Duplicate crate versions | `cssparser` 0.36/0.38, `selectors` 0.41/0.42, `servo_arc` 0.5/0.6 (Stylo/Blitz side vs Deno side vs the engine's own pins). The engine pins `selectors = 0.42`, not the version Blitz uses. Recorded, not changed. |

Removal policy followed: only demonstrably dead or redundant items were removed; fallback implementations were not touched; no renames.

## 3. Blitz retirement readiness (Task 2)

Full inventory in [`blitz-retirement-readiness.md`](./blitz-retirement-readiness.md). Key facts:

- **Three measures, deliberately.** Direct-import grep: **81** lines (Round 6: 149). **Reliance through `Deref`, measured by removing it and compiling: 149 errors in 4 files over 26 `BaseDocument` members** (`url` 49, `get_node` 39, selectors 15, events/default actions 11, `get_node_mut` 5, forms 3, accessibility 2, lifecycle and root accessors the rest). The low grep count understates the coupling by almost half.
- Vendored: `blitz-dom` 29,334 lines (5 patches), `blitz-paint` 6,877 (1), Stylo (1), `anyrender_skia` (1), `skia-bindings` (1).
- **No domain has an independently proven backend.** Replacement difficulty: lifecycle URL access low; HTML/DOM/XML/forms/accessibility medium; events/default actions and style hosting high; layout very high.
- **Conclusion:** Blitz cannot be retired yet, and a low grep count must not be read as independence. Next removals by value: URL access (≈ −51 `Deref` uses), node data and serialisation (≈ −55), selectors (−15).

## 4. Feature and security audit (Task 3)

Full tables with source paths and tests: [`browser-feature-completion-audit.md`](./browser-feature-completion-audit.md). Classification by the five categories, no percentages. Method: delegated read-only source review (no execution) plus author re-verification of the highest-impact claims; **the review is static, so "tested" means a test exists in source**, and engine tests were run (§6). The browser UI crate's tests and the Python/JS tests were not run.

Highest-impact gaps (each *verified* ✔ unless noted): iframes never render and expose no document; IndexedDB has no JS binding although the Rust store exists; Shadow DOM content is never styled, laid out or painted; WebGL rendering is simulated; media has no decoder or audio output; camera/microphone are placeholders behind real permission prompts; WebGPU, WebRTC and Web Audio are absent; CSP frame directives (`frame-ancestors`, `frame-src`, `form-action`, `base-uri`, `sandbox`) and reporting are absent; no COOP/COEP/CORP/X-Frame-Options; cookies are partitioned by profile only and `StorageKey` has no consumer; the iframe sandbox flags are modelled but only top-navigation is enforced; macOS and Windows sandboxes depend on signing/packaging and the native probes were not run; browser services (passwords, passkeys, printing, extensions, sync, DRM) are all category 4.

## 5. Reliability and security review (Task 7)

Negative tests added (`engines/wega/tests/reliability.rs`, run, 3 tests, also under broker mode):

1. **Page-event flooding is bounded** (verified): 50 000 `window.open`, 50 000 `clipboard.writeText`, 20 000 `Notification`, 5 000 `pushState` calls leave ≤ 1 queued browser event (producers are gated by activation and permission).
2. **Deep nesting within the supported depth works** (20, 100, 121 nested blocks lay out and paint).
3. **Known hang, pinned:** **122 or more nested block elements never finish layout** (121: ≈ 22 ms; 125/150/200: no result in minutes; sampling shows all time in Taffy's `compute_block_layout` recursion inside vendored Blitz). The script watchdog covers script only, so a ~1 KB page can hang its tab's engine process. Pre-existing; not fixed (vendor layout; roadmap B1 with acceptance criteria). Whether the browser UI detects and recovers from an unresponsive content process was not verified.

Other checks: untrusted URL handling, redirects, CORS and CSP script/connect are covered by existing tests (`security.rs`, `network/tests/security.rs`, `web.rs`; *run*); IPC messages are capped at 1 MiB before allocation, bodies at 8 MiB, redirects at 8 hops, scripts at 1024, work channel at 32 (*read*); unbounded: worker command channels and MessagePort queues (*read*, not exercised); the `PageEvent` queue has no cap but its producers are gated (test 1). Crash recovery, navigation teardown and worker termination have functional tests (`web_workers.rs`, `worker_runtime.rs`, `documents.rs`; *run*) but no kill-the-process tests. No permissions were added.

## 6. Standards and compatibility (Task 4)

Infrastructure *read* (delegated review, key figures re-run): `engines/wega/tests/wpt.rs` runs 35 pinned upstream files (commit `68d964a5…`, SHA-256 checked) in fresh engines with the real `testharness.js`; `css_wpt.rs` runs 2 CSS files and one hand-written reftest; `support/harness.rs` is a process-level worker harness, not WPT. **Supported:** inline scripts, `async_test` (4 files), polling to completion. **Not supported or not used:** `promise_test` (no pinned case), `step_timeout`, iframes, worker tests, testdriver, a generic reftest runner, expectations/skip lists (unsupported files are excluded from the subset and listed in prose in `docs/step-08-dom-events-web-apis.md`: per-tag element interfaces, XML documents, `:has()`/`:invalid`, iframes).

**Run this round** (exact counts):

| Suite | Result |
|---|---|
| DOM WPT subset (`wpt.rs`) | **35 files, 1685/1685 subtests passed**, 0 failed, 0 skipped |
| CSS WPT subset (`css_wpt.rs`) | **7/7 subtests** (2 files) + the clip reftest (0 differing bytes) |
| Framework fixtures (`compat.rs`) | 7/7 (React, Vue, Preact, jQuery, Angular, inline SVG, jQuery 3.6.3 bundle) |
| Process-level harness (`harness_fixtures.rs`) | 6/6 |
| Reftests (`css.rs`) | 21/21; pixel parity with Round 10 on 42 frames (Round 11) |
| Entire engines workspace | see §7 |

Coverage by area: DOM, events and Location only (events 10 files, Element 7, Node 3, ChildNode 3, Document 2, MutationObserver 2, Location 4, others 1 each). **No area exists for navigation, storage, security, accessibility, fetch, workers or layout.** Passing 1685/1685 is therefore a regression ratchet on a hand-picked subset, not a measure of standards conformance; nothing here supports a conformance percentage. Nothing was excluded silently: no test failed, and the exclusion list is the one in the repository documentation.

## 7. Validation (Task 9) and platform coverage (Task 5)

All *run* on macOS arm64 unless stated.

| Check | Result |
|---|---|
| Engines workspace suite (one target at a time because of disk space) | **509 passed, 0 failed, 12 ignored** (Round 11: 506/0/11; +3 `reliability.rs` tests, +1 ignored `worker_bench`). The 12 ignored: 7 engine benchmark functions (`hot_path_bench` ×5, `render_bench`, `worker_bench`), 2 adapter overhead benchmarks (`adapters/blitz/tests/contract.rs`), the native-sandbox probe, the required-worker test (run separately below, passes) and one further `#[ignore]`d test that was not identified. |
| Broker mode (`WEGA_TEST_BROKER=1`) | security 9, forms 4, scripts 8, documents 5, history 3, events 20, input 8, style 16, layout 15, render 17, reliability 3, dom 30, dom_identity 4, pages 16, lib 31: all pass |
| Required-worker test (content helper from `wega-content-bundle`) | passes |
| `cargo fmt --check`, clippy `-D warnings` (engines and tools workspaces), root `cargo check --locked`, `cargo metadata --locked` (root, engines, tools) | pass (fmt required one formatting fix of this round's new test files) |
| Architecture checker and its tests (8), tools tests (6) | 13 members, 24 edges, **0 violations**, 11 informational (14 at Round 10, 12 at Round 11: `blitz-html` is gone from the engine's direct dependencies) |
| Boundary tests (`wega-dom`, `wega-adapter-blitz`), `scripts/check-engine-boundary.py` | pass |
| Python tests for the new benchmark runner (3) and the existing browser bench (existing) | pass |
| Lockfiles | `engines/Cargo.lock` −5 internal dependency edges; root and tools lockfiles unchanged; no external package added or removed |
| New `unsafe` | 0 |
| Public API | unchanged this round (consolidation touched manifests, one dead data file and tests) |

**Not tested:** Linux; Windows (including MSVC Skia and the AppContainer path); the native sandbox probes (`native_sandbox`, `#[ignore]`) on every OS including macOS; packaging/CD; hosted CI results (the workflows were read: the `native` job runs the platform tests, native probe, required-worker test, the full engine suite with `--test-threads=1` and broker mode for five files on Linux, macOS and Windows, but no logs were examined); GPU/CPU rendering variants (the engine has CPU Skia only); the root browser UI crate's tests (`src/*_tests.rs`) and the Python/JS smoke suites. Successful macOS results make no claim about the other operating systems.

## 8. Performance and stability (Task 6)

New infrastructure: `scripts/engine-bench.py` (repeatable runner: release profile, repeats, per-metric median/min/max, hardware, load before/after, git revision; compare mode flags changes beyond run-to-run spread; unit-tested) and a worker benchmark (`tests/worker_bench.rs`). Baseline: [`baseline/engine-bench-round12.json`](./baseline/engine-bench-round12.json) — 64 metrics, 3 repeats, release, Apple M4 / 10 cores / 16 GiB, commit `87e1f81` (plus the benchmark files), **load average ≈ 9 before and after (not a quiet machine; the load before the first repeat was higher)**, so values are indicative and ranges are wide for a few metrics.

| Area | Metric (median [min–max], 3 repeats) |
|---|---|
| Document load | load + first paint 800×600: 61.9 ms [61.5–64.2]; `create_view`: 27.2 ms [26.7–27.8] |
| HTML parsing | 9000-node `set_html`: 70.6 ms [69.8–71.2] |
| DOM | `createElement+setAttribute+appendChild+getAttribute`: 55.8 µs [55.4–56.1] |
| JavaScript | `evaluate_json(1+1)` call: 3.54 µs [3.47–3.55] (no throughput benchmark exists) |
| CSS | computed style (no mutation) 0.65 µs; after an inline write 1.08 ms; selector match 9.95 µs |
| Layout | rect read 3.2 µs; document layout (300 cells) + paint 75.8 ms [75.3–116] |
| Paint | 800×600 unchanged page 0.62 ms/frame; 1600×1200@2× 1.31 ms |
| Events | dispatch (no listeners) 71.4 µs; native click 1.67 ms |
| Memory | +2 MiB over 300 frames (Round 11); no peak or multi-tab measurement |
| Workers | create + first reply + terminate **31.3 ms** [30.9–31.4]; message round trip **5.8 ms** [5.77–5.78]; 8 parallel workers create + reply 64.5 ms |

**Regression check vs Round 11** (34 metrics common to both runs): median change −1.3 %; 31 of 34 within ±5 %; the largest differences (−5.8 % to −8.3 % on document load/parse/lifecycle) are improvements within the noise of runs taken at different load; nothing regressed. This is expected: Round 12 changed manifests and tests only, not runtime code.

**Findings:** (1) the worker round trip of ≈ 5.8 ms is consistent with the 4 ms poll tick in `workers/runtime.rs:293` but the cause was not isolated; (2) the pre-existing items from Rounds 10–11 (resize ≈ 20× steady paint, idle repaint, layout hang above) remain; (3) the unbuilt gaps: no startup, navigation, JavaScript-throughput, multi-tab-interaction or CI-gated benchmark (`browser-bench.py` covers idle multi-tab resources and is in CI without a threshold; not run here, it needs the full browser binary).

## 9. Roadmap (Task 8)

[`post-round12-roadmap.md`](./post-round12-roadmap.md): critical blockers B1–B7 (layout watchdog and nested-block hang, frame-level CSP, cross-origin isolation, cookie partitioning, unsafe/sandbox evidence, worker queue bounds, idle repaint), high-value features F1–F9 (iframes first, then IndexedDB, Shadow DOM rendering, media, editing/forms, WebGL/WebGPU, device APIs, browser services, worker completeness), standards infrastructure S1–S4, vendor-replacement steps A1–A10 ordered by `Deref` removal, required infrastructure (CI benchmark gate, fuzzing, hostile-page corpus, crash-recovery tests), cross-platform gaps, and a suggested phase order. Each item has dependencies, effort, performance risk and acceptance criteria. Blitz replacement is explicitly last.

## 10. Commits

`87e1f81` consolidation (unused dependencies, dead data file) and reliability tests; `e2a9cc9` benchmark runner, worker benchmark and baseline; `21f56a1` readiness, feature audit and roadmap documents; plus this report and the progress update.

## 11. Blockers and limits of this audit

- Only macOS was available; no Linux or Windows host, and no hosted CI results.
- The feature and security tables come from a source review that did not execute code; the claims with the largest consequence were re-verified (§4, audit §5), the rest should be treated as *read, not run*.
- The benchmark baseline was recorded at load average ≈ 9; it is a repeatable reference, not a quiet-machine measurement.
- Disk space was near exhausted (as low as 1.2 GB); one build failure (link error) occurred from it; this project's stale `engines/target` test binaries were pruned (`cargo clean -p wega-engine`, ≈ 13 GB) and the suites were run one target at a time. No other project's artifacts were touched.
- The layout hang was characterised (threshold, location of the time) but not root-caused.
