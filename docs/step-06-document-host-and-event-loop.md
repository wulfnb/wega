# Step 06 — document host and event loop

Implementation delivered on `feature/standalone-wega-engine`, 1 November 2026.

## Ownership and task entry

`document_host.rs` owns incremental parsing, parser-blocking/deferred/ordered/async
script queues, script admission, encoding/meta processing and ready/load lifecycle.
It is a child module of the view so that orchestration retains one document owner;
no second DOM or shadow lifecycle is introduced. `view.rs` handles presentation,
navigation and native input. The script trait owns the runtime boundary, and V8
owns its microtask queue.

`event_loop.rs` admits deduplicated parser, lifecycle, runtime/timer and resource
tasks tagged with the current document revision. Each parser turn yields after
8 steps or 4 ms; pending work requests an immediate next wakeup. Script/module,
evaluation and animation-frame tasks finish with a microtask checkpoint even
when ordinary script execution throws. A checkpoint does not dispatch due timers
between parser-blocking scripts. The poll's runtime turn still drains V8 foreground
work/finalizers even without a due timer; removing this drain broke live-collection
GC behavior during validation and was corrected.

RAF and observer mechanisms remain in their existing runtime/DOM owners and enter
through the coordinated checkpoint/visibility lifecycle. This is not a claim that
all HTML event-loop task-source ordering or WPT conformance is complete.

## Navigation, freeze and teardown

Starting a provisional navigation suspends and freezes the old realm while
retaining its pixels. Old timers, asynchronous completions and native default
input actions cannot execute during that navigation. Stop/download handling
resumes the current document. Commit or explicit `set_html` replacement clears
queued work and drops old parser/runtime state; explicit replacement also cancels
an outstanding navigation. New document revisions cannot consume old task entries.
Visibility notification runs before freezing; resume unfreezes before visible
notification. Already-started broker work may finish on the host, but cannot run
callbacks in a dropped realm. Session/navigation revocation closes its socket and
storage authority.

## Exceptions and recovery

Ordinary exceptions report through the existing error path and preserve unrelated
listeners, timers and subsequent scripts. Fatal deadline/heap/stack failures stop
that realm. JSON serialization runs inside the same watchdog and heap checks,
including hostile `toJSON`; result length is checked before copying to Rust.
Module-pending/error-report paths preserve fatal classification.

Protocol 14 adds `ScriptStopped`. The worker emits it with the current document
scope and exits; the desktop rejects stale events, revokes resources, terminates
and shows explicit recovery. Creating a replacement document/restarting is the
recovery boundary. A fatal script is not classified as a compatibility fallback.
Older protocol peers receive a notice, while required workers demand version 14.

## Runtime adaptation audit

| Family | Runtime owner and integration |
| --- | --- |
| URL/URLSearchParams, binary/streams | Existing Deno extension implementations retained; no new parallel JavaScript semantic implementation |
| Fetch | Deno API with Rust broker routing; actual destination/mode and document context propagated; host credentials/CORS/DNS policy |
| WebSocket | Existing Deno API/framing over a Rust injected broker byte stream; no production direct socket permission |
| WebStorage | Existing Deno API/proxy over Rust broker storage; no production profile path |
| Crypto | Rust configures per-document `isSecureContext`; insecure documents lose subtle/randomUUID on both object and prototype; opaque about:blank receives no invented inherited trust |
| Timers/microtasks | Runtime polling at task turns; task completion checkpoints preserve microtasks without running timers early |
| Cancellation | Document replacement drops realm resources; provisional navigation suspends callbacks; broker session/navigation revocation rejects stale operations |

Secure-context exposure remains an incremental audit. Later API families must add
exposure rules through the WebIDL work, including inherited about:blank/frame
origins. This step does not assert complete WebCrypto, Fetch or Streams standards
coverage. Step 04 retains the full authority/CSP/caching audit; Step 08 retains API
family expansion.

## Regression evidence

`tests/document_host.rs` covers parser microtask/timer ordering, provisional
navigation callback suppression, pending-navigation replacement, ordinary exception
continuation, hostile serialization termination/recovery and insecure prototype
exposure. Existing documents/scripts/modules/encoding/write/DOM/error/input tests
cover preserved behavior. `broker_websocket.rs` exercises a real upgrade and Deno
message framing; inherited-worker fixtures cover the process boundary. Final test
results and platform limitations are recorded in `validation/standalone/progress.json`.
