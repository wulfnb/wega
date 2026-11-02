# Step 02 — engine contract and document lifecycle

Implementation delivered on `feature/standalone-wega-engine`, 31 October 2026.
Protocol version: **11**. Wega-owned changes in this step are Rust. V8/Skia and
upstream Deno remain permitted dependencies.

**Implementation is complete; runtime acceptance is deferred by the owner.**
Compilation includes regression-test sources but does not execute them. Neither
browser compatibility nor cross-platform runtime behavior is claimed from these
checks. Step 01's remaining baseline work and the later parity gates still apply.

## Ownership and identity

The browser owns stable `TabId`, top-level `FrameId`, random `ProcessSessionId`,
and the broker-issued `ServiceSessionId`/`PartitionId`. The current worker hosts
one real top-level frame (`FrameId(1)`, scoped by tab and process session).
Nested frames and process placement remain Step 14; this identifier does not
advertise an iframe implementation.

`DocumentId` is the browser navigation generation; `DocumentRevision` advances
when the worker actually replaces its DOM/global, including worker-initiated
replacements. `RequestId` advances across navigations. `DialogId` and
`ActivationId` cannot wrap or be reused within their owner. Reloading an unloaded
tab retains its tab key but receives a fresh process/service session.

The first private command is a bounded, versioned `Init`. All subsequent v11
commands and events, including Ready, dialogs, frame headers and shutdown,
carry the exact session identity. Both ends reject foreign, absent or nested
envelopes before dispatch. Raw RGBA follows a validated frame header, under the
existing size and frame-credit limits. The desktop requires the current worker
version; the updated worker preserves explicitly negotiated v4–v10 sessions.
Older sessions do not acquire v11 guarantees merely by discovering capabilities.

The broker validates its bearer capability **and** service/partition identity for
bound browser sessions. The host selects and canonicalizes the storage directory;
tabs sharing that live directory share its cookie jar and partition identity,
while different directories receive different identities. Legacy library clients
can explicitly open an unbound session; production browser pages use bound ones.

## Implemented contract

| Requirement | Implementation | Regression sources, execution deferred |
| --- | --- | --- |
| Shared engine-independent types | `engines/protocol`; application re-export; no Servo/V8/UI dependencies | Protocol tests and core compilation without desktop features |
| Typed session/document/frame/request identities | `identity.rs`, `DocumentContext`, `RequestContext`; stable tab key passed to Page | Identity envelope tests, worker foreign-session test |
| Capability/version negotiation and structured errors | Version-filtered capabilities, `ProtocolError`/`ErrorCode`, strict Ready handshake | Legacy v4/v6 fixtures, protocol compatibility tests |
| Correlation, deadlines, bounded admission | `DocumentRequests`, worker `CommandQueue`, bounded channels, evaluation-result limits | Request expiry/capacity tests, worker oversized-result test |
| Reused IDs and malformed IPC | v11 request high-water mark, strict navigation sequence, bounded decoding; v11 rejects legacy unscoped evaluation/state queries | Completed-ID replay, navigation reuse, truncation/oversize and session tests |
| Cancellation before VM entry | Input-thread cancellation and queue deadlines; RAII completion | Blocked-dialog queued-cancellation and queue deadline tests |
| Actual document commit and stale results/frames | Navigation generation plus DOM revision checks at admission, completion and native consumption | Navigation, internal replacement, buffered clean-state and frame tests |
| Host-owned lifecycle | Starting, Loading, Active, Frozen, Closing, Crashed, Closed; terminal states cannot reopen | Lifecycle and desktop snapshot tests |
| Native request attribution | `FrameAuthority` resolves a host-only `TrustedRequest` with session, document, principal and top-level site | Opaque-origin, replacement, dialog and native-action tests |
| Trusted user activation | Host input adapter issues five-second tokens; worker echoes them with popup requests; host consumes once at native dispatch | Forged, expired and replayed activation tests |
| Permission cancellation contract | Engine-neutral `ScopedPermissions` requires current authority on answer; bounded FIFO, deadline and deny-on-cancel/drop | Cancelled, stale, foreign-session, opaque-origin and expired permission tests |
| Close and process ownership | Runtime-release acknowledgement, `OwnedProcess` reaping, bounded shutdown grace, browser-exit drain; immediate broker revocation on close or IPC failure | Worker shutdown and platform subprocess tests; revoked capability transport test |

## Native authority rules

Native origins are never read from a page-supplied origin string or an ordinary
State event. The host authorizes an initial navigation URL; the network broker
records its observed response, including followed redirects. A document commit's
URL must corroborate that receipt before a tuple principal is issued. Receipts
are consumed once; fragment navigation can retain a previously observed principal.
No matching receipt means an opaque/unverified principal. In particular, an
unapproved worker-internal replacement or a retained page after a different-origin
download cannot obtain that response's origin simply by claiming a commit.
An opaque principal can display ordinary JS dialogs but cannot receive device
permission grants through the scoped permission port. Current permission admission
conservatively requires HTTPS; secure-context exceptions and real device backends
belong to Step 21.

Dialogs show their host-resolved origin and retain the exact document/requester
until answered. Native action queues retain that full requester through UI
consumption, so a replacement worker with reused document counters cannot inherit
an old worker's actions. Navigation, replacement, crash and closing invalidate authority.
Only trusted pointer/button input and qualifying non-repeat key input issue
activation. Evaluation, synthetic DOM events and IME updates issue none. A popup
request must match its current document and live token when drained; background
queueing does not extend the five-second lifetime. Freezing clears activation.
Downloads obtain cookie material from the host-owned broker, ignoring the worker's
legacy cookie payload.

The scoped permission queue is the contract for future native/device responders;
it does not expose simulated camera, geolocation or other device APIs. Existing
Servo permission ports retain their existing interface. Their request counter now
also fails on exhaustion rather than wrapping.

## Boundaries carried into later steps

- Step 03 supplies direct-Wega harness assertions and runtime observability. The
  source regressions here must be executed during the owner's final test pass.
- Step 04 must apply authoritative document origins, destinations, redirect/CORS
  checks and partition policy to **every** network/storage request. The new session
  binding and native-principal receipts are not complete network security. Existing
  renderer-origin fields and direct storage access still need that migration.
- Step 05 supplies OS sandboxing, job/parent-crash containment and broker-only
  access. An IPC identity is a routing/authority key, not an OS security boundary.
- Steps 09/21 complete document-native integrations and device permission backends;
  Step 14 supplies actual nested-frame ownership. Step 02 provides their contracts.
- Cancelling queued work prevents VM entry. It cannot undo effects of script or
  network I/O that already started. A shutdown acknowledgement proves release of
  DOM/JS ownership; the supervisor must still observe OS exit. Durable storage
  flushes and saving page edits are separate acceptance requirements.

## Compile-only checks and deferred acceptance

Run on macOS arm64 with the locked dependencies:

```sh
cargo check --locked --offline --no-default-features --lib
cargo check --locked --offline --features test-harness --bin wega --all-targets
cargo check --locked --offline --manifest-path engines/Cargo.toml --target-dir target/wega-engine -p wega-engine -p wega-network -p wega-protocol -p wega-platform --all-targets
```

The worker build script required the pinned Skia archive; fetching that archive
needed network access even though Cargo dependency resolution used `--offline`.
Existing Deno deprecation warnings and Servo vendor warnings remain.

The final test pass must run protocol, core permission, desktop snapshot, worker
subprocess and platform ownership regressions, then exercise the existing browser
with the extracted contract. It must cover direct Wega navigation, dialogs,
popup gestures, background tabs, fragment/reload/stop/download retention, forged
IPC, expiry/cancellation, crashes and normal shutdown. Linux/Windows evidence and
Servo/direct-Wega parity remain separate pending gates. No tests have been run for
this step at delivery.
