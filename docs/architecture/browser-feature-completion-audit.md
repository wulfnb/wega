# Browser feature completion audit (Round 12)

Classification (no percentages):

1. **Implemented and tested**: real behaviour, with at least one test that exercises it.
2. **Partially implemented**: real behaviour for part of the feature; gaps stated.
3. **Placeholder or simulation**: an API or UI exists but the backing behaviour is fixed, generated or absent.
4. **Not implemented**: no code found.
5. **Unknown due to missing evidence**: not verified.

Evidence method. The feature and security tables were produced by a read-only source review (grep and file reads; no build, no test run) and then **spot-checked** by the author: items marked ✔ were re-verified against the source in this round (listed at the end). "Tested" means a test exists in the source; the engine suite was run (506 passed, 0 failed, 11 ignored at Round 11; this round's run is in the report), so engine tests named here pass on macOS. Tests under `src/` (the browser UI crate, root workspace) and the Python/JS tests were **not** run in this round. Paths are relative to the repository root; `E` = `engines/wega`.

## 1. Document and web APIs

| Feature | Cat | What exists and key gaps | Source | Tests |
|---|---|---|---|---|
| Iframes: DOM and frame isolation model | 2 | The frame/authority model exists in `wega_protocol` (frame tree, sandbox flags, origin policy, depth/count limits). `contentWindow`/`contentDocument` always return null ✔; a visible iframe sets the notice "Visible iframe rendering is not implemented by Wega" ✔. | `E/src/dom/semantics.js:2029-2033`, `E/src/view.rs` (iframe check), `engines/protocol/src/browsing.rs` | `E/tests/frames.rs` (6; they assert that frames expose no document), protocol unit tests |
| Iframes: visible rendering | 4 | No sub-document is laid out or painted. | — | none |
| Shadow DOM: DOM semantics | 1 | attachShadow, open/closed roots, slots, event retargeting, `composedPath`, `delegatesFocus`, focus retargeting. | `E/src/dom.rs` (`shadowAttach`), `semantics.js` | `E/tests/custom_elements.rs`, `events.rs` |
| Shadow DOM: style/layout/paint | 4 | A shadow root is a detached fragment: shadow content gets no style, layout or paint; `:host`, `:host-context`, `::slotted` have no effect; no `adoptedStyleSheets`. The engine reports a compatibility notice. | `E/src/dom.rs:1135-1148` | pinned in `E/tests/style.rs`, `layout.rs`, `render.rs` (known-gap tests) |
| IndexedDB | 4 for pages | A Rust store with transactions, versioning and crash recovery exists, but no `indexedDB`/`IDBFactory` global exists in the JS bindings or the V8 adapter ✔; page script cannot reach it. | `engines/services/storage/src/indexeddb.rs` | `E/tests/storage.rs::indexeddb_transactions_version_changes_and_crash_recovery` (Rust API only) |
| Cache API | 1 | Persistent CacheStorage bound to the storage service; Vary/query/method matching, quota, origin/profile isolation, secure-context gating. Not every method audited. | `E/src/dom/cache.js`, `engines/services/storage` | `E/tests/cache_api.rs` (6), `storage.rs` |
| Service Workers | 2 | Real worker runtime, register/update/unregister, install/activate/fetch, offline serving, restart persistence. `Clients.navigate()` unsupported; push/sync and the full container surface not verified. | `E/src/workers/service.rs` (1189 lines), `E/src/dom/web.js`, `worker.js` | `E/tests/web_workers.rs` (4 service-worker tests) |
| Dedicated workers | 1 | Real isolated realm with its own global scope; classic, module and blob workers; transfer, MessageChannel, termination, document-teardown cleanup. Worker message round trip ≈ 6 ms in release (see report; consistent with a 4 ms poll tick, not isolated). | `E/src/workers/runtime.rs`, `transfer.rs`, `E/src/dom/worker.js` | `web_workers.rs`, `worker_runtime.rs`, `worker.rs` |
| Shared workers | 2 | Same-origin sharing works; one test; connect/port lifecycle edge cases unverified. | `E/src/workers/shared.rs` (43 lines) | `web_workers.rs::shared_worker_is_shared_by_same_origin_documents` |
| OffscreenCanvas | 2 | Worker 2D drawing committed to the placeholder canvas; no WebGL on offscreen, `convertToBlob`/`ImageBitmap` unverified. | `E/src/dom/worker.js`, `E/src/workers/runtime.rs` | `web_workers.rs`, `canvas2d.rs` |
| Canvas 2D | 1 | Real Skia raster surface; paths, Path2D, arcs, text, `drawImage`, serialisation, tainting, memory quota. `ctx.filter` application unverified. | `E/src/canvas/context.rs`, `E/src/dom/canvas.js` | `E/tests/canvas2d.rs` (9) |
| WebGL 1/2 | 3 | A state machine (context, shaders as text, programs, buffers, loss, tainting). No shader execution: `draw_arrays` ignores `mode` and fills the viewport with a colour guessed from uniforms ✔ (`_mode` unused). WebGL2 is a flag. | `E/src/webgl/context.rs:625-665`, `engine.rs`, `host.rs` | `E/tests/webgl.rs` (7; they test the state machine, not rendering) |
| WebGPU | 4 | No `navigator.gpu`/`requestAdapter` ✔. | — | none |
| Media playback | 2 (WAV/WebM metadata and a clock) / 3 (frames, sound) / 4 (MediaSource) | Only WAV and WebM metadata are parsed; playback state is a clock model; no decode and no audio output found in the engine (video frames are a generated pattern per `docs/pending-features.md`). | `E/src/media/{engine,wav,webm,clock}.rs` | `E/tests/media.rs` (8; metadata and state only) |
| Camera/microphone | 3 | Permission flow is real (secure context, opaque origins, revocation); tracks are placeholders with fixed labels and no capture backend; `enumerateDevices` returns `[]`. | `E/src/dom.rs:596-660`, `semantics.js:3141-3175` | `E/tests/permissions.rs::mediastream_device_capture_and_acknowledged_revocation` |
| WebRTC | 4 | No `RTCPeerConnection` ✔. | — | none |
| Web Audio | 4 | No `AudioContext` ✔. | — | none |
| Clipboard and editing | 2 | `execCommand` supports only selectAll, copy, cut, paste ✔ (the `case` list); Range, Selection, DataTransfer, ClipboardEvent classes exist; editing defaults (typing, selection) come from Blitz. No bold/insertText/etc. | `semantics.js:4146-4190`, `E/src/view.rs` | incidental (`input.rs`); no dedicated execCommand/Range/Selection test |
| HTML element interfaces | 2 | Form submission/FormData/Enter tested; templates and custom elements tested. `HTMLInputElement`/`HTMLSelectElement` are thin; dialog `showModal` absent ✔; customized built-ins fail with "not implemented"; no `ElementInternals`. | `semantics.js`, `E/src/dom/forms.js` | `E/tests/forms.rs` (4), `custom_elements.rs`; none for dialog/details/select |
| Accessibility | 2 | An AccessKit tree is built from Blitz and published as `PageEvent::Accessibility`; the UI bridges it. No engine-level test of the page tree or ARIA mapping. | `E/src/view.rs`, `blitz-dom/src/accessibility.rs`, `src/accessibility.rs` | chrome nodes only (`src/components_tests.rs`) |
| fetch/XHR/WebSocket | 2 | fetch from `deno_fetch` with local override; XHR in `dom/xhr.js` (sync XHR refused); WebSocket via a broker socket. EventSource not exposed ✔ (absent from `web.js`) and untested. | `E/src/dom/web.js`, `xhr.js`, `engines/adapters/v8/src/broker_socket.rs` | `E/tests/web.rs`, `broker_websocket.rs` |
| localStorage/sessionStorage | 1 | Persistent, origin-partitioned, quota-limited, opaque origins rejected. | `E/src/dom/web.js`, `deno_webstorage` (patched) | `E/tests/storage.rs`, `web.rs` |
| Permissions / Notifications | 1 | Query/request/prompt/revocation/cancel-on-navigation; notifications gated and queued. Grants are in memory only. | `E/src/permissions/`, `E/src/dom.rs` | `E/tests/permissions.rs` (5) |
| History/navigation | 2 | pushState, fragment navigation, hashchange, back/forward over pushState entries. Navigation API unsupported. | `E/src/dom/observers.js`, `E/src/view.rs` | `E/tests/history.rs` (3) |
| Observers | 2 | Mutation/Resize/Intersection observers in JS; Resize reads layout boxes (tested in Round 10). Thresholds, rootMargin, multiple entries untested. | `semantics.js`, `observers.js` | `dom.rs`, WPT MutationObserver files, `layout.rs` |
| Web Crypto | 2 | `crypto`/`CryptoKey` from `deno_crypto`; completeness unchecked. | `E/src/dom/web.js` | no Wega-level test |
| Intl / Temporal | 5 | V8's Intl is presumably available; Temporal absent; nothing in Wega touches or tests it. | — | none |

## 2. Browser services

| Service | Cat | Evidence | Source | Tests |
|---|---|---|---|---|
| Password manager / autofill | 4 | No credential store or autofill; only `<input type=password>` handling. | `E/src/view.rs:2142,2190` | none |
| Credentials / passkeys / WebAuthn | 4 | No `navigator.credentials`/`PublicKeyCredential`. | — | none |
| Printing | 4 | No print API or UI. | — | none |
| Extensions | 4 | No extension system (the `extension_adapter` in the V8 adapter is an internal DOM-hook installer). | — | none |
| Account sync | 4 | No sync or sign-in (`ed25519` use is error-report signing). | — | none |
| DRM / EME | 4 | No `MediaKeys` support. | — | none |
| Downloads | 2 | Bounded, multipart, resumable, publish-after-validate, mark-of-the-web; shells out to a system `curl`; no safe-browsing warnings. | `src/downloads.rs`, `download_actions.rs`, `downloads_ui.rs` | `src/downloads_tests.rs` (~20, not run this round) |
| Containers / profile isolation | 2 | One browser process and one profile directory per container (separate window), authenticated loopback hand-off; isolation is per profile directory, not in-engine; `docs/containers.md` may be stale (mentions Servo). | `src/containers.rs`, `container_launch.rs` | `src/containers_tests.rs`, `container_launch_tests.rs`, `engines/network/tests/security.rs` (cross-container test), `tests/containers_smoke.py` |
| DevTools | 3 / 2 | An egui console panel and an injected `inspector.js` that walks the DOM; no engine-backed debugger or network panel. | `src/devtools_ui.rs`, `src/inspector.js` | one font test |
| Find in page | 2 | Request flow to the engine and a result event; highlight and scroll-to-match unverified. | `E/src/view.rs:1254`, `main.rs:1277` | `E/src/view.rs::find_in_page_and_tree_walker`, protocol serialisation |
| Permissions UI | 2 | Bounded prompt queue; only blocks persist per origin; grants are in memory; devices behind prompts are simulated. | `src/permissions.rs`, `engines/services/permissions` | `src/permissions_tests.rs`, `model_tests.rs` |

## 3. Security

| Item | Cat | Evidence | Source | Tests / gaps |
|---|---|---|---|---|
| CSP: script-src(-elem/-attr), style-src-elem, img-src, font-src, connect-src, worker-src, nonce, hash, strict-dynamic, eval | 1 | Parser, fallback chains, URL/inline/eval decisions, enforced at document, script, worker and fetch call sites. Subresource classification uses file extension for Blitz-loaded resources (unknown extension passes if any of img/style/font allows). Policies capped at 16. | `engines/services/security/src/csp.rs`, `E/src/document_host.rs`, `dom.rs`, `workers/*` | `csp.rs` unit tests, `E/tests/security.rs` |
| CSP: frame-ancestors, frame-src, form-action, base-uri, trusted-types, sandbox, upgrade-insecure-requests | 4 | Only `child-src` appears (as the worker-src fallback) ✔; the file header says frame directives and reporting are not implemented. | `csp.rs` | none |
| CSP reporting (report-uri/report-to) | 4 | None. | — | none |
| Iframe sandbox attribute / frame policy | 2 | Flags parsed and inherited by intersection; the only enforcement found is the top-navigation check; forms/popups/scripts flags have no consumer; no iframes are rendered. | `engines/protocol/src/browsing.rs:42-80, 343` | `browsing.rs` tests (model-level) |
| Cross-origin isolation (COOP/COEP/CORP, SharedArrayBuffer gating, X-Frame-Options) | 4 | No handling ✔ (`crossOriginIsolated` is hard-coded false in worker scope). CORS exists separately. | — | none |
| Navigation authority | 2 | UI-side scheme rules (page-initiated `file:`/`data:`/`wega:` restricted), protocol grants, a file-to-file check in the engine. Renderer-initiated navigation grants listed as pending in `docs/pending-features.md`; engine `javascript:` handling not located. | `src/navigation.rs:49`, `engines/protocol`, `E/src/dom.rs:1593` | `src/navigation_tests.rs`, `requests_tests.rs` |
| Redirects | 1 / 2 | Broker: 8-hop cap, downgrade refusal, per-hop policy, `authorization` stripped cross-origin, method rewrite; engine-side loop follows manually with the same method rules. Only `authorization` is stripped. | `engines/network/src/lib.rs:637-690`, `E/src/network.rs:575-631` | `engines/network/tests/security.rs`, `E/tests/web.rs` |
| Cookie / storage partitioning | 2 | SameSite and public-suffix handling are real; the jar is keyed per profile directory (so per container). `StorageKey` with `top_level_site` exists in the protocol crate but **has no caller outside it** ✔. No third-party cookie policy beyond SameSite; no CHIPS. | `engines/services/cookies`, `engines/protocol/src/storage.rs` | cookies unit tests, `E/tests/security.rs`, `storage.rs`, network cross-container test |
| Worker and frame permissions | 2 | Worker CSP applies; permission registry is model-level; Permissions-Policy not found. | `workers/mod.rs`, `engines/protocol/src/permissions.rs` | `worker.rs`, `web_workers.rs` |
| Brokered filesystem and network | 2 | Content process receives inherited broker channels; a direct-network `Local` client and a development launch mode exist as explicit bypasses (release builds refuse the bypass per the launch tests). Media is still read from local files. | `E/src/main.rs:285-340`, `E/src/network.rs`, `engines/platform/src/launch.rs` | `engines/network/tests/security.rs`, `engines/platform/src/launch_tests.rs` |
| OS sandbox: Linux | 1 (code) / 5 (runtime on this host) | Landlock (ABI ≥ 6), seccomp BPF, no-new-privs, capability refusal; fails closed on older kernels. | `engines/platform/src/sandbox_linux.rs` | `engines/platform/tests/native_sandbox.rs` (`#[ignore]`; run only by the CI `native` job; not run here) |
| OS sandbox: macOS | 2 | Verifies a separately signed App Sandbox helper's entitlements rather than applying a sandbox itself; depends on packaging/signing. | `sandbox_macos.rs`, `bin/wega-content-bundle.rs` | native probe (`#[ignore]`) not run this round |
| OS sandbox: Windows | 2 / 5 | AppContainer launch + job object; verified at startup. `windows_spawn.rs` has 55 `unsafe` occurrences and no SAFETY comments ✔. | `windows_spawn.rs`, `launch_windows.rs` | one job-limit test; not run (no Windows host) |
| Renderer process isolation | 2 | One sandboxed `wega-engine` process per page (not per site); iframes unrendered so no out-of-process frames; the parent keeps the broker, cookie jar and downloads. | `src/wega_page.rs:683` | `process_tests.rs` (not read) |

## 4. Reliability findings from this round

| Finding | Evidence | Status |
|---|---|---|
| **Layout hangs for ≥ 122 nested block elements** (121: ≈ 22 ms; 122+: not finished in minutes; CPU time sampled inside Taffy `compute_block_layout` recursion in vendored Blitz). The script watchdog does not cover layout/paint. | probe and `E/tests/reliability.rs::known_gap_nesting_of_122_blocks_never_finishes_layout` | open, pinned |
| Idle pages repaint continuously (every `poll()` marks dirty) | Round 11; `render.rs::known_gap_every_poll_marks_a_settled_page_dirty_again` | open, pinned |
| Page-event producers (`window.open`, clipboard, Notification, `pushState`) cannot flood the browser queue (queue stayed ≤ 1 after 50 000 calls) | `reliability.rs::page_event_producers_cannot_flood_the_browser_queue` | verified |
| Worker command channels and MessagePort queues are unbounded `mpsc::channel`/`VecDeque` (flooding `postMessage` is bounded only by the script deadline and memory) | review (`workers/runtime.rs:97,367`, `service.rs:1100`, `transfer.rs:50`); **not exercised** | open, unverified impact |
| `get_node(node_id()?).unwrap()` in the DOM host ops looked page-reachable in review; each is immediately preceded by `node_id()?`, which rejects a stale id | `dom.rs:1073-1081` ✔ | guarded precondition; not a defect |
| `policy.read().unwrap()` (5 sites in `dom.rs`) panics on a poisoned lock | review | low (single writer path), open |
| Non-test `unwrap/expect/panic` counts: engine 26 + 2, V8 adapter 28, network 7 + 2, platform 3, protocol 3 | script over the crates (test modules excluded) | recorded |
| `unsafe` outside vendor: platform (Windows files without SAFETY comments), UI crate FFI (`native_*`, `windows_instance.rs`, `mac_application.rs`), V8 adapter 3 documented | review (221 occurrences in 35 files incl. false positives such as the string `unsafe-inline`) | recorded; Windows files uncommented |

## 5. Re-verified by the author in this round (✔)

Iframe `contentWindow`/`contentDocument` null; no `indexedDB` anywhere in the engine JS or V8 adapter; WebGL `draw_arrays(_mode…)`; no `navigator.gpu`/`RTCPeerConnection`/`AudioContext`; no `showModal`; no EventSource in `web.js`; CSP file lists only `child-src` among frame directives; no COOP/COEP/CORP/XFO handling; `StorageKey` has no caller outside the protocol crate; `windows_spawn.rs` 55 `unsafe` / 0 `SAFETY`; the `node_id()` guard; execCommand cases. Everything else in this document comes from the unverified-by-execution source review above.
