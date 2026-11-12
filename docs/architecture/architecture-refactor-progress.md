# Architecture-first refactoring — progress

**Policy (current):** internal APIs may break; behaviour should be preserved; no compatibility re-exports without real consumers; no speculative abstraction crates; vendored engines stay pinned; protocol and persisted formats unchanged; no memory-safety, sandbox or origin-security compromises; nothing is pushed automatically.

Branch `feature/browser-cache-accessibility`. This document covers the first round: Phase A (ownership model), Phase B (JavaScript/V8 decoupling) and the raster part of Phase C. All commits are local and unpushed.

## 1. Phase A — ownership model (validated against the code)

### 1.1 Shared types and coupling inspected

| Crate | Observation | Decision |
|---|---|---|
| `wega-protocol` | Wire contract plus `Origin`/`Principal`/frame tree; no workspace dependencies | stays the single shared vocabulary; no new crate |
| `wega-platform` | OS process/IPC/sandbox, `FrameStore`, process metrics | unchanged |
| `wega-network` | HTTP, policy, broker; cookies/storage/security/permissions already extracted | unchanged this round |
| `wega-engine` | Monolith: view, DOM host, V8 glue, canvas/WebGL/media, workers | restructured below |

**Foundation crate: not justified.** A search of function names across `protocol`, `platform`, `network`, `services/*`, `adapters/*` and the engine found no shared helper defined in more than one crate (the repeated names are trait-method implementations such as `fetch`, `referrer`, `connect_check`; `base64` exists once in `wega-security`). There is no existing consumer pair for a common crate, so none is created.

### 1.2 Ownership model

| Concern | Owner (after this round) | Evidence / rule |
|---|---|---|
| **Engine** | `Engine` owns the shared `Network` (policy, dispatcher threads, cookies, caches, broker handle) **and** the worker `Hubs` (shared workers, message ports, service workers) and the `Limits`. | `lib.rs` `Engine::assemble` wires them once. |
| **Document / view** | `View` (one per tab slot) owns the Blitz document, scheduler, raster thread, permissions and a `Box<dyn ScriptHost>`; per navigation it **builds the document host (`Dom`) and passes it to the script host**. | `view.rs` builds `Dom::new(HostContext{…})`, then `script::create(dom, limits)`. |
| **JavaScript runtime** | The script host (`V8Host`) owns the isolate, watchdog, executor and module state; it receives its `Dom` instead of creating it. `Dom` implements `NativeHost` (`dom_host.rs`). The V8 glue lives in `src/v8/` and reaches the engine only through `Dom`, `Limits`, `ScriptHost`/`ScriptError` and `Requests`. | §3 |
| **Network service** | `Network` owns dispatch and transport; the script glue sees only `ScriptNetwork` (`remote`, `storage_dir`, `queue`, `fetch_module`). `Network` offers requests to a `FetchInterceptor` and does not name worker types. | `v8/net.rs`, `network.rs` |
| **Workers** | Worker state is engine-owned (`workers::Hubs`); each worker realm is an OS thread with its own isolate, started from a `Spec` that carries the hubs it needs; worker code depends on `Network`, not the reverse. Cross-thread: only `IsolateHandle` termination and the `sink` callback. | `workers/mod.rs`, `workers/runtime.rs` |
| **DOM and rendering** | Blitz owns the DOM/style/layout/paint; `View` shares `Rc<RefCell<BaseDocument>>` with `Dom` and the parser; the display list (`anyrender::Scene`) crosses to the raster thread owned by `wega-adapter-skia`. Canvas 2D and WebGL stay in the engine with direct Skia use (documented exception). | gate report §4-§6 |
| **Errors** | Operations return `Result<_, String>` internally; script failures are `ScriptError { fatal, message }` (fatal = tear the realm down); worker/network failures cross as strings. No change this round. | `script.rs` |
| **Cancellation** | Script steps are bounded by a watchdog thread holding a thread-safe `IsolateHandle`; fatal limits drop the realm; requests are cancelled by dropping channels/`AbortController`; workers stop via `Command::Stop` + isolate termination. No change this round. | `v8/mod.rs`, `workers/runtime.rs` |

Lifecycle: `Engine::new → Network + Hubs → create_view → View::new → navigation: HostContext → Dom → V8Host (isolate from snapshot) → script runs; teardown drops V8Host (isolate, watchdog) and Dom (workers manager, canvas/WebGL/media engines)`. The V8 flag setup (`--lite-mode`) stays process-wide in `main.rs` before platform initialisation.

## 2. Architectural decisions

1. The view, not the V8 host, creates the document host (`Dom`); the V8 host is injected with it.
2. The Deno glue depends on the narrow `ScriptNetwork` interface (defined next to the glue), implemented by `Network`. `wega_network::{Request, Response}` are unchanged.
3. Worker hubs are owned by the engine (`Hubs`), not by `Network`; `Network` uses a `FetchInterceptor` implemented for the service-worker hub.
4. All V8/Deno glue is one directory (`src/v8/`), so a later crate extraction is a directory move plus `build.rs`.
5. `RawIsolateGuard` (the engine's only `unsafe`) is kept, with written safety invariants; a safe rewrite is not attempted.
6. Raster rendering is a separate crate (`wega-adapter-skia`); Canvas/WebGL keep direct Skia use.
7. `wega-js-runtime` is **not** created yet (see §8); no foundation crate (§1.1).
8. Compatibility re-exports removed or not added: `crate::native_ops`, `crate::web`, … paths simply moved; `wega_engine::{Painted, Pixels}` stay because they appear in `View`'s public signatures.

## 3. Changes implemented (commits)

| Commit | Change |
|---|---|
| `f3ba19c` | Test fix: accepted sockets in the Cache API fixture are made blocking (root cause of the flaky `cache_api` test; 0 failures in 25 runs, previously ~20-35%) |
| `c699d02` | **B1** `V8Host::new(dom, limits)`; view builds `Dom`; adds the ignored `hot_path_bench` test |
| `68b4997` | **B2** `ScriptNetwork` for module loader, WebSocket connector and extension setup |
| `f066709` | **B3** `workers::Hubs` owned by the engine; `FetchInterceptor`; `Network` no longer references workers |
| `e430f0b` | **B4** `src/v8/` directory (host, ops, extension adapter, loader, broker socket, Deno setup, secure-context exposure, `ScriptNetwork`); `dom_host.rs`; `build.rs` paths; mutation-script paths. **B5** `RawIsolateGuard` invariants documented |
| `7bffc42` | **C** `wega-adapter-skia` (raster only) extracted; engine drops direct `anyrender_skia` and `kurbo` dependencies |
| `1e5da1a` | this report (round 1), post-refactor graph and architecture report |

## 4. Crates created or removed

Created: `wega-adapter-skia` (`engines/adapters/skia`, `forbid(unsafe_code)`). Removed: none. Not created (by decision): `wega-js-runtime`, `wega-adapter-v8`, `wega-dom`, `wega-html`, `wega-workers`, any foundation crate.

## 5. Dependency changes

Graph (9 members, 14 edges; [`baseline/dependency-graph-after-refactor.md`](./baseline/dependency-graph-after-refactor.md)): new edges `wega-engine → wega-adapter-skia`, `wega-adapter-skia → wega-platform`. Architecture checker: **0 violations** ([`baseline/architecture-check-after-refactor.md`](./baseline/architecture-check-after-refactor.md)); informational: `wega-engine` still depends directly on `servo` (root workspace), `anyrender`, `blitz-{dom,html,paint,traits}`, `deno_core` (normal and build), `html5ever`, `skia-safe`, `stylo`.

Cargo changes: `wega-adapter-skia` added as a workspace member and engine dependency; `anyrender_skia` and `kurbo` removed from `wega-engine` (now owned by the adapter, same versions). `engines/Cargo.lock` gained only the new path package; no external version changed. Root `Cargo.lock` unchanged (the root crate does not depend on the engine).

In-crate dependency direction after the refactor: `workers → network` (previously also `network → workers`); `v8/* → {dom::Dom, Limits, script::{ScriptHost,ScriptError}, dom::Requests, NetworkPolicy}` and nothing else in the engine.

## 6. Tests and known regressions

- Full engine suite (`--workspace --all-targets`, `--test-threads=1`) at the Phase C commit: **360 passed, 0 failed, 3 ignored** (2 native-sandbox ignored tests + the benchmark).
- Broker mode (`WEGA_TEST_BROKER=1`: security, forms, scripts, documents, history): all passed. Required-worker test with a CI-style content helper: passed. Clippy `-D warnings`, `fmt --all --check`, root `cargo check --locked`, root `cargo metadata`: pass.
- Per-step focused tests (see commits): lib, scripts, web, web_workers, worker, broker_websocket, inherited_broker, cache_api, dom, canvas2d, webgl, image_limits, pages.
- **Incident (resolved, no product impact):** a scratch copy of the engine built into the *shared* `target/wega-engine` directory produced artifacts with the same cache identity as the real crates, so one run of the service-worker test (`web_workers::service_worker_serves_offline_survives_restart_updates_and_unregisters`) linked stale scratch objects and failed. After `cargo clean -p wega-network -p wega-engine` and a rebuild it passes and the full suite is green. **Rule: scratch workspaces must use their own `--target-dir`.** Earlier results that used the shared directory after the Wave 2 gate work were re-covered by the clean full run above.
- Known pre-existing issues (not regressions): 9 stale mutation-script targets (`scripts/engine-web-mutations.py`, `engine-p0-mutations.py`) point at `view.rs` text that now lives in `document_host.rs`; `tokio` deprecation warnings and 8 `deno_core` warnings (not suppressed); `cache.addAll` transport errors surface as `Error` instead of `TypeError`.

## 7. Build and memory performance

| Measure | Wave 1 baseline | Now |
|---|---|---|
| Full test run wall time | 375 s | 393 s (includes recompiling the engine after the cache clean; machine load average 11-30 throughout) |
| Max RSS / peak footprint | 1.47 GB / 177 MB | 1.48 GB / 181 MB |

Hot-path benchmark (`tests/hot_path_bench.rs`, debug build, medians of 7; the machine was heavily loaded, so only large changes are meaningful):

| Metric | Before (`f3ba19c`) | After (`7bffc42`) |
|---|---|---|
| `evaluate_json("1+1")` | 13.1 / 13.5 µs | 13.3 / 14.0 µs |
| DOM op iteration (createElement + setAttribute + appendChild + getAttribute) | 450 / 360 µs | 361 / 481 µs |
| `create_view` + `set_html` (document lifecycle) | 211 / 189 ms | 234 / 236 ms |

Interpretation: evaluation and DOM-op costs are unchanged within noise (they pass through the same `Rc<dyn NativeHost>`). The document-lifecycle number is 12-20% higher in this pair of runs; the added per-document work is a handful of `Arc` clones and one `Arc<dyn ScriptNetwork>` allocation (microseconds), so this is most likely load noise, but it **should be re-measured on a quiet machine** before declaring the milestone done.

## 8. Remaining Blitz, V8 and Skia coupling

| Engine | Where it remains | Count |
|---|---|---|
| Blitz | `dom.rs`, `view.rs`, `parser.rs`, `script.rs`, `document_host.rs`, `components.rs` (direct `BaseDocument`, `NodeId`, `DocumentMutator`, `blitz_paint`, `blitz_html`) | 6 files; ≈86 `get_node`, 40 `mutate()` call sites |
| Stylo / html5ever | `dom.rs` (media-query parsing), `parser.rs` (`TreeSink`) | 2 files |
| V8 / Deno | `v8/*` (7 files) + `workers/runtime.rs` + `main.rs` (`v8_set_flags`) + `build.rs` | 9 files with `deno_core`; engine `Cargo.toml` still lists the whole Deno stack |
| Skia | `canvas/*` (5 files), `webgl/context.rs` (PNG encode) — documented exceptions; the raster path is now in `wega-adapter-skia` | 5 files |

## 9. Recommended next step

**Extract `wega-adapter-v8` together with the host traits, as one change, after one more in-engine step.**

1. Replace `V8Host`'s remaining use of the concrete `Dom` (~13 sites: `begin_task`, `errors`, `requests`, `budget`, `set_allocated`, `history`, `network.{wake,policy,remote,storage_dir}`, document URL) with a small environment trait (`V8Environment`) implemented by `Dom` in `dom_host.rs`. This is the last coupling that is not already an interface: after it, `src/v8/` references nothing in the engine except `Limits`, `ScriptError`/`ScriptHost`, `Requests` (an associated type) and the `NetworkPolicy` alias.
2. Then move `src/v8/` into `engines/adapters/v8` (`wega-adapter-v8`) together with `build.rs`, the snapshot build and the `src/dom/*.js` assets decision (the WebIDL generator location stays frozen), and decide at that point whether the host traits (`NativeHost`, `ScriptHost`, `ScriptNetwork`, `V8Environment`) live in the adapter or in `wega-js-runtime` (the gate report recommends the adapter until a second consumer exists). Workers keep using the adapter's runtime-construction helpers.
3. Re-measure the hot-path benchmark on a quiet machine first and after.

Other open items: `wega-dom`/`wega-html` stay deferred (no viable contract; ≈86/40 Blitz call sites); Canvas/WebGL Skia exception; the `Error`-vs-`TypeError` conformance bug; refresh the stale mutation-script targets.

---

# Round 2 — remove the Dom dependency and extract `wega-adapter-v8`

Baseline verified first: branch `feature/browser-cache-accessibility`, the seven round-1 commits present, clean tree, full suite green.

## 10. Remaining `Dom` references in the V8 glue (classified)

All 13 references were in `V8Host` (`src/v8/mod.rs`); none were elsewhere in the glue. They were **not** one concern, so the interface is grouped by concern rather than being a thin wrapper over `Dom`:

| Reference (before) | Concern | Interface method(s) |
|---|---|---|
| `dom.document.borrow().url()` | security context | `document_url()` |
| `Arc::new(dom.network.clone())` | network access | `network() -> Arc<dyn ScriptNetwork>` |
| `dom.network.policy` | network access / permissions | `network_policy()` |
| `dom.network.remote` (×1) | network access | removed: `ScriptNetwork::remote()` already existed |
| `dom` as `Rc<dyn NativeHost>` | JavaScript callbacks | `ScriptEnvironment: NativeHost` (upcast) |
| `dom.begin_task()` | lifecycle | `begin_task()` |
| `dom.network.wake` (×2, in tick/module wakers) | resource ownership (cross-thread waker) | `wake()`, captured once in `V8Host::new` |
| `dom.errors` (×2: drain, bounded slow-script note) | results drained by the view | `take_errors()`, `note_slow_script()` |
| `dom.requests` | results drained by the view | `take_requests()` (associated `Requests`) |
| `dom.budget()`, `set_allocated`, `history` | resource budgets | `budget()`, `sync_nodes()`, `history()` |
| (isolate/thread affinity) | V8 isolate ownership | stays inside `V8Host`; the environment never sees V8/Deno types |

## 11. New interfaces and ownership

- **`ScriptEnvironment`** (`env.rs`): the table above. No `Dom`, Blitz or V8/Deno types; `wega_network::Policy`/`Remote`/`Request`/`Response` are kept. **Threading/reentrancy** is documented on the trait: the environment is shared as `Rc<E>` on the document thread (not `Send`/`Sync`); the values it hands to other threads (`network`, `wake`) are `Send + Sync`; methods may be called from V8 callbacks and must not re-enter script or hold a `RefCell` borrow across a call into script. `V8Host<E>` is **generic**, so these calls are statically dispatched; the only dynamic dispatch remains `Rc<dyn NativeHost>` in the isolate's `OpState` (as before) and the engine's `Box<dyn ScriptHost>`.
- **Location of the contract:** in `wega-adapter-v8` itself. Cargo shows exactly one implementer/consumer pair (the engine implements, the adapter consumes), so a separate `wega-js-runtime` crate would add a boundary with no second user. It can be split out later without changing the traits.
- **`ScriptHost`/`ScriptError`/`NativeHost`/`ScriptNetwork`** moved with the adapter. `NativeHost` gained `is_generated_hook` so the engine owns the generated-hook registry (`dom/generated/hooks.json`) and the adapter no longer embeds an engine path.
- **`create_runtime(RuntimeSpec)`** replaces two near-duplicate runtime set-ups (documents and workers); `RuntimeKind::{Document, Worker}` carries the differences (Web Storage backend/extension directory).
- **Snapshot ownership:** the engine's `build.rs` still generates the WebIDL bindings and reads `src/dom/*.js`, then calls the adapter's `build_startup_snapshot`; the engine embeds the snapshot and passes snapshot and bootstrap script to `V8Host::new(env, &HostConfig)`. The adapter has **no** references to root `tools/`, root `vendor/` or `engines/wega` paths.
- The adapter does **not** depend on `wega-engine`; the one isolate per document/worker is still created by `create_runtime` on the owning thread (no second isolate, no duplicated runtime state).

## 12. Commits (round 2)

| Commit | Change |
|---|---|
| `8a56b4a` | `V8Host<E: ScriptEnvironment>`; `Dom` implements it (`dom_host.rs`); `src/v8` has no `Dom`/Blitz/engine request references |
| `5973fc9` | Extract `wega-adapter-v8` (`engines/adapters/v8`); build.rs now calls the adapter's snapshot builder; mutation-script paths updated |
| `784b623` | Architecture checker: documented unsafe exception (`wega-adapter-v8`) is informational; post-V8 graph and report |
| (this commit) | this section |

## 13. Dependency graph before/after (engines workspace)

Before (round 1): `wega-engine → {network, platform, protocol, security, permissions, adapter-skia}` plus a build-dependency on `wega-network` and the whole Deno stack as normal and build dependencies. After ([`baseline/dependency-graph-after-v8.md`](./baseline/dependency-graph-after-v8.md)): 10 members, 18 edges; new `wega-engine → wega-adapter-v8`, `wega-adapter-v8 → {wega-network, wega-protocol, wega-security}`; the engine's build-dependency on `wega-network` is gone. The engine dropped ten direct Deno-related dependencies (`deno_error`, `deno_webidl`, `deno_web`, `deno_fetch`, `deno_websocket`, `deno_webstorage`, `deno_crypto`, `deno_permissions`, `deno_net`, `deno_tls`, `sys_traits`); `deno_core` remains (workers). Lockfile: new path package only, no external version change. No cycles; checker: **0 violations** ([`baseline/architecture-check-after-v8.md`](./baseline/architecture-check-after-v8.md)).

## 14. Unsafe code audit

`grep` across the engines workspace sources: the engine (`wega-engine`, `deny(unsafe_code)`) now contains **no** `unsafe`. `wega-adapter-v8` is `deny(unsafe_code)` with one documented exception: `RawIsolateGuard` (two `unsafe` blocks in `host.rs`) with written invariants (balanced enter/exit on one thread, valid isolate pointer, LIFO nesting, no aliasing). `wega-adapter-skia` is `forbid(unsafe_code)`. Native `unsafe` otherwise lives only in `wega-platform` as before. The architecture checker recognises the V8 exception explicitly.

## 15. Validation (round 2)

- Full engines suite at `5973fc9`: **360 passed, 0 failed, 3 ignored**; 386 s wall, 1.55 GB max RSS, 181 MB peak footprint (round-1: 393 s, 1.48 GB; the +70 MB RSS is within build/link variation, not re-profiled).
- Broker mode (`WEGA_TEST_BROKER=1`: security, forms, scripts, documents, history): passed. Required-worker test with a CI-style content helper: passed (exercises the worker runtime built by `create_runtime`).
- Clippy `-D warnings` (workspace and tools), `fmt --check`, root `cargo check --locked`, both `cargo metadata`: pass. Tools workspace tests pass.
- Mutation-script targets: all 54 checked targets resolve (including the adapter files); the 9 stale `view.rs` targets from before remain.
- Not run: Windows and Linux builds/tests (cfg-specific `sys_traits` features and `winapi` paths moved with the code unchanged), the MSVC Skia cache jobs, and the native-sandbox ignored tests.

## 16. Performance comparison

Hot-path benchmark, debug build, load average 14-17 on the machine (noise is ±30%, larger than any difference), medians of 7, three runs per revision interleaved with checkouts of the same tree:

| Metric | Before extraction (`8a56b4a`) | After (`HEAD`) |
|---|---|---|
| `evaluate_json("1+1")` | 13.4 / 14.0 / 14.8 µs | 13.3 / 13.3 / 15.2 µs |
| DOM op iteration | 425 / 442 / 587 µs | 361 / 398 / 369 µs |
| `create_view` + `set_html` | 211 / 298 / 229 ms | 235 / 256 / 187 ms |

No measurable change. (Two earlier pairs on the same loaded machine also overlapped.) Generic `V8Host<E>` plus cross-crate calls did not change the cost of the existing `Rc<dyn NativeHost>` path.

## 17. Remaining blockers and coupling

- **Engine still depends on `deno_core`** for worker realms (`workers/runtime.rs` uses `JsRuntime`, `execute_script`, `IsolateHandle`). Removing it needs a worker-runtime wrapper in the adapter; `WorkerHost` also depends on the engine's `canvas`/`webgl`, which stay for now.
- `build.rs` still `#[path]`-includes root `tools/webidl/*.rs` and reads root `vendor/servo-script-bindings` WebIDL inputs (WebIDL generator location unchanged by decision); the DOM JS assets remain under `engines/wega/src/dom/`.
- `wega-js-runtime` is intentionally not created (single implementer/consumer pair).
- Blitz (`dom.rs`, `view.rs`, `parser.rs`, `script.rs`, `document_host.rs`, `components.rs`), Stylo/html5ever (`dom.rs`, `parser.rs`) and Skia in Canvas/WebGL are unchanged.
- Stale mutation-script targets (9) and the `Error`-vs-`TypeError` fetch conformance bug remain open.

## 18. Recommended next step

Move worker realm creation behind the adapter (a `WorkerRuntime` wrapper that owns `JsRuntime`, the dispatch/execute calls and the isolate handle), then drop `deno_core` from `wega-engine`; separately decide whether `canvas`/`webgl` hosts stay in the engine (they are needed by `WorkerHost`). After that, the first candidate for the DOM work is a measured look at the ≈86 `get_node`/40 `mutate()` Blitz call sites, not a new crate.
