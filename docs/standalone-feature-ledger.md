# Standalone engine feature ledger

Source baseline: `d853e5f` on `feature/standalone-wega-engine`, 31 October 2026.
Runtime verification: **deferred until the final test pass at the owner's request**.
No row in this ledger is runtime-verified by the migration work yet.

This expands the [execution plan](standalone-engine-execution-plan.md). The generated
[source inventory](validation/standalone/source-inventory.json) records hashes,
all 574 vendored WebIDL files, preference gates, build conditions, delegate methods,
WebDriver command mentions and fixture sources. Each IDL file has a provisional
replacement-step route; routing is not proof of runtime exposure or implementation.
The companion [feature ledger](validation/standalone/features.json) supplies stable
IDs, source references, target steps, pending gates and empty runtime evidence.

Regenerate the source snapshot without building, launching or testing the browser:

```sh
cargo run --locked --no-default-features --features migration-tools --bin wega-inventory
```

The Rust tool reads the repository and the pinned `servo-config 0.7.0` source from the
Cargo registry. If it is unavailable or ambiguous, supply `--servo-config /path/to/prefs.rs`.
It never reads a personal browser profile. Source hashes preserve the actual inputs
even if the capture's HEAD includes unfinished migration work.

The inventory generator is implemented in Rust. Its snapshot also lists existing
Python, handwritten JavaScript and shell files in Wega's source/tooling directories
for ownership and Rust-migration audit; no Python inventory implementation remains.

## Source audit findings that change the parity scope

The enabled root Servo dependency uses `bundled`, `js_jit`, `webcrypto` and `webgl`;
desktop default features add media and Brotli. `multiprocess` is opt-in. Windows
adds `no-wgl`. Do not equate Servo's upstream default features with this configuration:
the root dependency has `default-features = false`.

Preferences are set by `servo-config 0.7.0`'s `Preferences::const_default`, then by
`src/browser.rs`'s builder. In the inspected source:

| API/feature | Source gate | Migration scope |
| --- | --- | --- |
| WebCrypto | `dom_crypto_subtle_enabled = true`, Cargo `webcrypto` enabled | Existing enabled surface; replace and verify. |
| DedicatedWorker | Worker IDL has Window/Worker exposure without a preference gate | Existing source candidate; runtime/API behavior must be captured. |
| SharedWorker | `dom_sharedworker_enabled = true` | Existing source candidate; include it in worker parity. |
| IndexedDB | `dom_indexeddb_enabled = false` | Extension backlog; preserve any separately discovered enabled entry points. |
| ServiceWorker | `dom_serviceworker_enabled = false` | Extension backlog; not a delivered browser capability merely because IDL exists. |
| Notifications | Default false; browser overrides true only under `test-harness` | Preserve native integration and harness behavior; production enablement is a separate decision. |
| Async Clipboard | `dom_async_clipboard_enabled = false` | Distinguish native clipboard/selection from navigator.clipboard API exposure. |
| Protocol handlers | `dom_navigator_protocol_handlers_enabled = false` | Preserve existing responder code; verify actual release exposure separately. |
| WebGL2 / OffscreenCanvas / WebGPU | Respective prefs false | Separate from enabled WebGL1 and Canvas 2D parity. |
| WebRTC / capture / Permissions API | Relevant prefs false | Keep enabled backend fragments and page-scoped native permissions distinct from full API support. |
| ResizeObserver | `dom_resize_observer_enabled = true` | Include in DOM/API parity. |
| IntersectionObserver / FontFace / execCommand | Relevant prefs false | Audit member-level gates; avoid claiming these are all shipped Servo features. |
| Gamepad / XR | Preferences true, but root Cargo backend features not enabled | A preference alone does not deliver an input/device/runtime backend. |

IDL build filters and member-level `Pref`, `Func`, exposure and secure-context
conditions still need semantic/runtime audit. The index records those inputs rather
than guessing that every declared interface exists on `window`.

## Product and engine feature register

`partial` and `implemented` below describe source paths only. All platform runtime
states remain `unknown` until the deferred test pass. The JSON companion is the
machine-readable register; do not mark a family verified based on this table.

| ID | Current product behavior and sources | Wega source observation / required replacement | Steps |
| --- | --- | --- | --- |
| F01 | Application/session ownership, movable tabs/panes, history/bookmarks/settings: `src/application.rs`, `session.rs`, `ui.rs`, `profile.rs`, `containers.rs` | Browser core reusable; desktop tab objects and container engine/data plumbing still depend on Servo. Preserve URL/title/engine migration and account boundaries. | 02,09,10,23 |
| F02 | Native navigation and acknowledged traversal: `src/navigation.rs`, `navigation_adapter.rs`, `automation.rs` | Wega has browser history plus pushState/fragment history. Port stop, cache bypass, completion and clear-history with correlated document identity. | 06,09 |
| F03 | Servo delegates publish metadata, geometry, popups and controls: `src/engine.rs` | Wega State/Cursor/OpenWindow events exist; favicon/geometry/opener and move/resize parity need dedicated implementations. | 09,14 |
| F04 | Conservative unload/close/quit confirmation and edit probes: `src/page_safety.rs`, `page_state.js`, `lifecycle.rs` | Wega PageState is request-correlated, but not document-correlated; late responses can outlive navigation. Keep conservative confirmation on errors/timeouts. | 02,05,09 |
| F05 | 574 IDL source files; scripts/crypto/DOM preferences: `vendor/servo-script-bindings/webidls/`, `src/browser.rs` | Wega handwritten DOM, parser and Deno adapters exist; generation, complete semantics and task ordering remain open. Every file is indexed, not certified. | 06,07,08 |
| F06 | HTTP interception/auth/cache/cookies: `src/capabilities.rs`, `network_controls.rs`, `download_actions.rs` | Shared Wega broker exists. Origin/destination/partition authority, broader caching, auth and transport parity require audit; direct-worker bypasses must close before sandboxing. | 04,06,09 |
| F07 | Origin/policy behavior in pinned Servo scripts/network, and `src/engine.rs` policy intercept | Wega CORS/CSP/mixed-content/referrer subsets exist; module dependencies and extension-based resource classification have known limitations. | 04,05,06,07,08,14 |
| F08 | Input and embedded controls: `src/input.rs`, `engine.rs`, `file_handoff.rs` | Wega pointer/keyboard/IME/form submission exist; select popup, file capabilities and full native-control/a11y integration remain missing. | 11 |
| F09 | Native find/reader scripts, OS clipboard and Servo DOM Range: `src/page_actions.js`, `reader.js`, `engine.rs` | Wega has partial Range and input selection; page selection, editing/geometry, copy/cut/paste/find and drag/drop require real implementations. | 12 |
| F10 | CSS/SVG/text/rendering/zoom: `vendor/servo/`, `src/browser.rs`, `input.rs`, `capture.rs` | Blitz/Stylo/Skia already render; coverage gaps need source-family and pixel/interaction evidence. Mask fallbacks are not general mask conformance. | 13,19 |
| F11 | Frames/Window/popup relationships: pinned Servo IDL and scripts, `src/engine.rs` | Visible frames currently trigger Servo fallback. Need browser-owned frame tree, origin policy and process placement, composed input/a11y, real WindowProxy and messaging. | 14 |
| F12 | ShadowRoot/CustomElementRegistry/HTMLSlotElement IDL | Wega missing component trees/style scoping/lifecycle. Preserve exact enabled Servo behavior after exposure capture. | 15 |
| F13 | Site-data UI and HttpOnly cookie table: `src/capabilities_ui.rs`, `cookie_inspector.rs` | Wega Deno storage globals and broker cookie jar exist, but native data-management/UI adapters still call Servo. Session scoping and deletion require direct verification. | 04,16,22 |
| F14 | IndexedDB/ServiceWorker disabled by source preferences; existing clear-data UI only claims cookies/local/session/cache/history | New storage scope must remain explicitly additional. No IndexedDB/CacheStorage/SW data-clear claim until stores and clear paths exist. | 16,17 |
| F15 | Worker/SharedWorker IDL and source preference gates | Wega lacks public Worker/SharedWorker constructors and multi-agent lifecycle. Include SharedWorker because its source preference is true. ServiceWorker is tracked separately. | 17 |
| F16 | Cargo WebGL enabled; Canvas/WebGL IDL | Wega lacks canvas/WebGL surfaces. Skia can supply 2D rasterization but not WebGL API/GLSL semantics. WebGL2 is source-disabled. | 18,19 |
| F17 | Default GStreamer playback, WebM/WAV fixtures, native media controls: `src/native_media.rs`, `tests/media_smoke.py` | Wega media backend missing. Reuse native action/metadata behavior through engine-neutral messages; port before deleting adapters. | 20 |
| F18 | Enabled audio/media families and disabled WebRTC/capture prefs in pinned sources | Audit Web Audio/MSE separately from capture/WebRTC and platform backend availability. Do not infer from an IDL file that a codec/device works. | 20,21 |
| F19 | Page-scoped permission responders, notification/control dialogs: `src/permissions.rs`, `permission_adapter.rs`, `native_notifications.rs` | Wega needs trusted requesting-document/frame context and revocation acknowledgement before persistent grants. Native notifications need actual platform evidence. | 02,21 |
| F20 | User scripts, reader/dark style, screenshots and segmented download manager: `src/probes.rs`, `capture.rs`, `downloads.rs`, `download_actions.rs` | Wega has evaluation/frames/download events but browser UI/tools often use Servo types. Preserve credentials, pause/resume/cancel and atomic publication. | 09,12,19,22 |
| F21 | All native tool panels: `src/toolbox.rs`, `inspector_ui.rs`, `debugger_ui.rs`, `network_har.rs`, `cookie_inspector.rs` | Wega logs exist; full DOM/style/network/debugger/storage/memory adapters remain open. | 03,22,24 |
| F22 | Loopback WebDriver and opt-in Firefox remote server: `src/automation.rs`, `debug_driver.rs`, `options.rs` | Wega protocol has evaluate without request IDs. Automation explicitly rejects many Wega operations. Preserve supported endpoints; record remote wire compatibility as a separate requirement. | 03,22 |
| F23 | Servo AccessKit tree/actions bridged into native window: `src/accessibility.rs`, `engine.rs` | Chrome AccessKit exists; Wega content tree transport and frame/document action ownership remain missing. | 11,13,14 |
| F24 | Native external-scheme approval, protocol registration, file handoff/auth and viewers: `src/engine.rs`, `file_handoff.rs`, `capabilities.rs` | Wega text/image/download viewers exist; internal routes and native privileged actions require browser mediation. Release protocol API may be preference-disabled. | 09,10,21 |
| F25 | Container process/profile registry and Docker viewer: `src/containers.rs`, `container_launch.rs`, `docker/`, `tests/containers_smoke.py` | Preserve authenticated launch, per-container data/download paths, upload handoff and remote gestures. No login/state transfer between engines currently exists. | 10,11,23,25 |
| F26 | Structured diagnostics, signing/outbox, resource accounting and packaging: `src/diagnostic_event.rs`, `diagnostics.rs`, `error_reporting.rs`, `scripts/` | Wega stderr/log/metrics paths exist. Shared protocol ownership, resource totals, sandbox services and release backend closure still need implementation. | 03,05,24,25,26 |
| F27 | Entire source inventory including less common IDL families and build/preference gates | Every IDL has a provisional owner step; declaration-level effective exposure and semantic completeness remain unknown until audited. No catch-all row can satisfy the parity gate. | 01,08 |

## Baseline evidence to collect in the final pass

Freeze the original application source at `d853e5f` and record its clean-build
commands, features, platform, toolchain and binary/worker hashes. Existing target
binaries may have been built from other commits; their presence is not provenance.
Run baseline and migration builds in separate target/profile directories. Never
reuse a personal profile. Keep capture metadata identifying Servo versus direct
Wega; fallback is a failed Wega result.

Run the nine native suites, containers/viewer suites, the direct engine fixtures,
DOM/CSS WPT subsets and upstream behavioral tests relevant to new code. Collect
the same source/API observations from normal release and `test-harness` builds
where their gates differ. Record macOS, Linux and Windows outcomes independently.

## Source-only profile migration audit

| Existing owner/path | Preservation requirement | Source |
| --- | --- | --- |
| `browser.json` | Preserve layout, dock/theme/sidebar width, URL/title order, active index, bookmarks/history/favicons, policy and saved engine choices. Old missing engine fields still deserialize. | `src/profile.rs`, `model.rs`, `storage.rs` |
| Profile lock and temporary save files | Failed loads must not overwrite the existing profile; keep exclusive ownership and atomic-write recovery semantics. | `src/storage.rs`, `profile.rs` |
| `engine/` | Keep Servo cookies/storage/cache/media registry intact until an audited import can preserve them. Never interpret its files as Wega's storage format. | `src/browser.rs`, `docs/containers.md` |
| `wega-storage/` | Preserve the broker cookie jar and origin-keyed Deno storage; migrate keys transactionally if top-level partitioning changes. | `src/wega_page.rs`, `engines/wega/src/web.rs`, `engines/network/src/cookies.rs` |
| `containers.json`, `containers/<id>/` | Preserve IDs/paths and private account data, rename semantics, corrupt-registry failure behavior and authenticated per-profile launch. | `src/containers.rs`, `container_launch.rs` |
| Downloads, screenshots, log/outbox and viewer account records | Retain independent application data and private file permissions; engine retirement must not wipe them. | `src/downloads.rs`, `capture.rs`, `diagnostics.rs`, `error_reporting.rs`, `docker/gateway.py` |

No real profile was opened or copied for this audit. Migration fixtures and any
supported Servo cookie/storage importer remain pending. Re-login, if unavoidable,
must be reported explicitly; do not silently discard the old data.

## Progress and deferred verification

- 01.a: source inventory and family ledger prepared; full effective exposure audit pending.
- 01.b: runtime baseline capture deferred by owner.
- 01.c: source-only data-path audit prepared; copied-profile migration fixtures pending.
- Step 01 remains **in progress**, not complete or verified.
- 02.a: the existing bounded v5 wire contract is now owned by the Rust-only
  `wega-protocol` crate. Browser, worker and subprocess fixtures share that crate
  without importing application source files. Wire behavior is intentionally
  unchanged in this extraction; document/request correlation follows in 02.b.
- 02.b: protocol v6 adds typed document/request identities, capability discovery,
  structured failures and bounded, deadline-aware pending requests. Desktop edit
  checks and evaluation now use scoped exchanges; old/duplicate replies are ignored,
  navigation/crashes cancel pending operations, and even a buffered old clean-state
  reply cannot authorize closing a replacement document. Loading documents reject
  scoped queries conservatively. The updated worker retains v4/v5 packet support
  for existing older browser sessions.
- Regression tests for request expiry, forged/duplicate replies, navigation,
  buffered edit-state responses, worker shutdown and oversized evaluation results
  are written but **not executed**, following the owner's instruction.
- Remaining Step 02 work: explicit lifecycle/commit states, scoped native events,
  frame/service identities and queued-command cancellation. Do not mark the entire
  step complete based on the extraction or correlation slice.
- Later implementation may proceed from source contracts while tests are deferred;
  final parity/retirement gates remain blocked until those tests actually pass.
