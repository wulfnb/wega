# Browser completion work

Updated 11 November 2026. Wega's standalone runtime is the default, but browser
feature completion remains open. This page and the blockers in
`validation/standalone/progress.json` supersede older claims of complete parity.

## Implemented in this change

### Persistent page Cache API

Secure HTTP contexts (HTTPS and HTTP loopback) expose `caches`, `CacheStorage`
and `Cache`. Named caches support open/has/delete/keys and cross-cache match.
Cache objects support match/matchAll/keys, put/add/addAll and delete. Responses
retain their bytes, status, headers and original response URL; reconstructed
headers are immutable. Matching handles fragments, query options, GET method
checks, and request headers named by Vary. Invalid partial responses and Vary:*
are rejected. addAll fetches real resources and commits the complete batch in a
single SQLite transaction, including duplicate-request and quota rollback.

The host broker checks the committed document's origin and service grant. It
derives the storage path from the profile root and origin hash; renderers never
receive a filesystem path. Site-data clearing closes the relevant connection
before deletion. The Rust storage helpers and page API use the same records;
existing origin-scoped cache files migrate transactionally without discarding
their response bodies.

Limits: 1 MiB response bodies, 32 operations per batch, 128 named caches, 4096
records and a 10 MiB serialized-record budget per origin. Large result sets can
also hit the broker's 8 MiB response limit. The current implementation rejects
operations through a Cache object after its named cache is deleted; keeping
detached cache objects alive across deletion still requires handle leases and
garbage collection. Opaque responses and worker-global Cache exposure remain
outside this implementation. Cache Storage alone does not implement Service
Worker script execution or automatic offline navigation.

The binding follows the matching and atomic batch behavior described by the
[Service Workers specification](https://www.w3.org/TR/service-workers/#cache-interface).

### Desktop page accessibility

Page trees are validated and assigned host-generated subtree identities scoped
to the tab, worker session and document revision. They are grafted under egui's
Web page node after the chrome tree has been published. Page node IDs cannot
replace chrome node IDs. Content focus waits for the first subtree publication;
actions to an old document's subtree are ignored. Bounds use page zoom and
display scaling. Painting emits a current, bounded accessibility snapshot so
page mutations update the tree. Deactivation stops worker updates.

The ownership and publication ordering follow
[AccessKit's subtree contract](https://github.com/AccessKit/accesskit/blob/main/ARCHITECTURE.md#sub-trees).
Local type/consumer regressions do not certify VoiceOver, NVDA or Orca behavior;
manual screen-reader and supported-platform validation remain required.

### Native-page navigation

Opening a native `wega://` page from a website releases the old Wega worker and
preserves the previous URL in the tab's history. The tab now reports the native
page's URL/title rather than the stale website document. Returning to the website
creates a new worker and retains its origin-scoped cached data. The desktop
WebDriver suite covers this transition and the persistent Cache API.

## Remaining implementation stages

| Area | Required implementation and acceptance |
| --- | --- |
| Iframes | Real child realms and documents, site-isolated worker ownership, surfaces/input/focus and frame policy enforcement. Prove nested pages, embeds and login widgets with pixels and interactions. |
| Shadow DOM | Stylo scoped cascade and flattened Blitz layout, slot changes, focus and accessibility. Validate styled component libraries, not only API presence. |
| IndexedDB | Expose the existing SQLite backend through an authoritative page binding with version-change transactions, structured clone, requests/events, indexes/cursors and rollback/concurrency behavior. |
| Service Workers | Fetch and execute actual worker scripts, install/activate/update lifetimes, persistent registrations, clients and fetch events. Nonexistent scripts must fail registration. |
| Media | Broker resource loading, actual decoding/audio output/video surface integration and accurate codec capabilities. Remove synthetic successful loads and generated video patterns. |
| WebGL/WebGPU | Real shader compilation/execution, buffers/textures/draw calls and compositor integration; conformance and real graphics applications. Current WebGL software simulation is not a complete backend. |
| Devices and real-time APIs | Native camera/microphone enumeration and capture, permission/revocation ownership, WebRTC and Web Audio. Hardcoded device descriptions are not hardware support. |
| Clipboard/editing | Broker the page clipboard API to the OS with permission and activation checks; complete real selection/editing behavior. |
| Security | Complete CSP frame directives/reporting, policy coverage for frames/workers and current-head native confinement/platform acceptance. |
| Browser services | Password storage/autofill and credentials/passkeys, printing, extensions, account sync and DRM integration each require separate designs and end-to-end acceptance. |

## Validation commands

```sh
cargo test --locked --offline --manifest-path engines/Cargo.toml --target-dir target/wega-engine --workspace --all-targets -- --test-threads=1
cargo test --locked --offline --bin wega accessibility::tests -- --test-threads=1
cargo check --locked --offline --bin wega
```

The engine tests include page-level persistence/isolation and real HTTP addAll
failure/duplicate rollback, plus a subprocess using inherited broker grants.
The required OS-sandbox variant is a separately enabled native acceptance test;
the default subprocess check does not certify sandbox confinement.

Local macOS results: 355 engine-workspace tests passed (two opt-in native tests
ignored in that command), 113 core and 53 desktop tests passed, and the required
App Sandbox worker test passed separately with the Cache API check. The rebuilt
desktop passed the WebDriver suite and a direct cache-persistence capture. Strict
engine-workspace and desktop Clippy and formatting checks passed. Linux/Windows,
hosted CI and manual screen-reader acceptance were not run for this change.
See [the recorded validation scope](validation/browser-completion-2026-11-11.json).
