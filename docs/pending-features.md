# Pending browser features

Recorded 11 November 2026 from `browser-completion.md`,
`validation/standalone/progress.json`, `validation/standalone/features.json`
and a spot-check of the engine source. This is a checklist of open work, not a
design; each area's contract lives in its `step-NN-*.md` page.

## Web platform features

| Area | What remains | Current state in source | Step |
| --- | --- | --- | --- |
| Iframes | Child realms and documents, site-isolated workers, surface composition, input and focus routing | FrameTree authority exists; visible child documents do not | 14 |
| Shadow DOM | Scoped Stylo styling, flattened Blitz layout, slots, focus, accessibility | `attachShadow` and slot metadata exist; no scoped style or flattened layout | 15 |
| IndexedDB | Page-facing API, version-change transactions, structured clone, indexes, cursors | Rust backend only (`engines/network/src/indexeddb.rs`); no page `indexedDB` | 16 |
| Cache API | Opaque responses, worker-global exposure, handles that survive named-cache deletion | Page API implemented and persistent | 16 |
| Service Workers | Script fetch and execution, install/activate/update lifecycle, clients, fetch events | `register()` only records a registration (`engines/wega/src/dom/web.js`) | 17 |
| Workers | Complete worker script loading and API support | Dedicated and shared workers exist with gaps | 17 |
| Canvas in workers | Real `OffscreenCanvas` rendering | No-op rendering paths | 18 |
| WebGL | Real shader compilation and execution, conformance | Software simulation | 19 |
| WebGPU | Everything | Not exposed | 19 |
| Media | Brokered loading, real decoding, audio output, video surfaces, accurate codec reporting | Loads fabricate success; video is a generated colour pattern | 20 |
| Devices | Native camera/microphone enumeration and capture, revocation | `enumerateDevices()` returns hardcoded devices | 21 |
| WebRTC and Web Audio | Everything | No `RTCPeerConnection` or `AudioContext` | 21 |
| Clipboard/editing | OS clipboard with permission and activation checks, complete selection/editing | `navigator.clipboard` uses an in-memory string (`engines/wega/src/dom.rs`) | 12 |
| Security | CSP `frame-ancestors`/`frame-src`, violation reporting, frame and worker policy coverage | Fetch-type directives and `sandbox` only | 04 |
| HTML element interfaces | 64 per-tag classes such as `HTMLDivElement` | Not exposed | 08 |
| Accessibility | Screen-reader acceptance on all platforms | Page subtree grafts implemented | 11 |

## Browser services

None of these is started; each needs its own design and end-to-end acceptance.

- Password storage and autofill
- Credentials and passkeys (`PublicKeyCredential`)
- Printing
- Extensions
- Account sync
- DRM

## Infrastructure and audits

- Step 03: full offline framework matrix and WPT asynchronous, reftest and
  testdriver paths.
- Step 04: full authority/CSP/redirect/cache audit and renderer-initiated
  navigation grants.
- Parity ledger: all 27 features in `features.json` have a pending semantic
  audit and unknown runtime status on macOS, Linux and Windows.
- Step 26: Servo retirement remains closed (`servo_retirement_allowed: false`).

## Validation

- Linux and Windows acceptance for the Cache API and accessibility change.
- Hosted CI.
- Manual VoiceOver, NVDA and Orca checks.
- Opt-in native ambient-authority denial probe.
- Packaged release, signing and notarization.
