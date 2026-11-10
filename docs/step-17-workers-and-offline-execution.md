# Step 17 Contract: Workers and Offline Execution

## 1. Overview & Objectives

Step 17 delivers complete worker and offline execution infrastructure for Wega:
- **Transfer and Structured Clone Primitives** (`engines/wega/src/workers/transfer.rs`):
  - Deep clone of structured JS data across isolate boundaries.
  - Transferable ownership semantics: transfer of `ArrayBuffer` detaches the source buffer (byte length set to 0, data detached) and moves byte buffer ownership to the destination.
  - `MessagePort` entanglement, peer routing, queueing, active message dispatch, and port closure.
  - `MessageChannel` instantiation providing two entangled `MessagePort` endpoints.
- **Dedicated Web Workers** (`engines/wega/src/workers/dedicated.rs`):
  - Each dedicated worker runs on an independent background OS thread with its own isolated V8 isolate / `JsRuntime` and dedicated event loop.
  - Cross-origin script loading rejection enforcing same-origin security.
  - Bidirectional communication via `postMessage` and `onmessage`.
  - Deterministic termination via `worker.terminate()` and document navigation lifecycle teardown.
- **Shared Workers** (`engines/wega/src/workers/shared.rs`):
  - Multi-document connection multiplexing across same-origin documents.
  - Shared state retention across connecting pages via entangled message ports.
  - Host-managed singleton instance lifecycle per origin and script URL.
- **Service Workers and Offline Execution** (`engines/wega/src/workers/service.rs`):
  - Registration with path prefix scope matching and validation.
  - Lifecycle state machine: `Installing`, `Installed`, `Activating`, `Activated`, and `Redundant`.
  - Offline fetch interception: `fetch_intercept` resolving matching requests against `CacheStorage` offline caches.
  - Scope unregistration and lifecycle cleanup.
- **V8 Multi-Isolate Thread Safety** (`engines/wega/src/v8.rs`):
  - RAII `RawIsolateGuard` entering and exiting V8 isolates around evaluation and context operations, safely supporting multiple isolate stacks on the same thread without isolate/context mismatches.

## 2. Architecture & Implementation

### Worker Primitives (`engines/wega/src/workers/`)
- `transfer.rs`:
  - `StructuredClone`: transfers primitive and object values, `ArrayBufferTransfer`, and `MessagePortTransfer`.
  - `ArrayBufferTransfer::detach()`: takes memory buffer and marks detached flag.
  - `MessagePortHub`: thread-safe cross-thread registry for entangled ports, delivery channels, and buffered inboxes.
- `dedicated.rs`:
  - `DedicatedWorker`: spawns background thread, initializes `WorkerJsRuntime`, binds `postMessage`, routes messages to parent outbox, processes incoming messages in worker loop, and provides graceful or hard shutdown via thread join.
- `shared.rs`:
  - `SharedWorkerHub`: maps `(Origin, Url)` to `SharedWorkerInstance`.
  - `connect(port)`: delivers connected port to worker's `onconnect` handler.
- `service.rs`:
  - `ServiceWorkerManager`: registers workers with scope matching (`is_in_scope`).
  - Cache storage integration: intercepts fetch events and returns stored cached HTTP responses when network is offline or cache matches.
- `mod.rs`:
  - `WorkerManager`: coordinates dedicated workers, shared worker hub, service worker manager, and entangled message channels.

### Document Lifecycle Teardown
- `View::navigate()` and `View::drop()` cleanly trigger `WorkerManager::terminate_all()`, terminating all worker background threads and cleaning up isolates upon document navigation.

## 3. Verification & Evidence

- Integration test suite `engines/wega/tests/web_workers.rs`:
  - `transferable_ownership_detaches_source_buffer` — PASSED
  - `message_channel_port_entanglement_and_closure` — PASSED
  - `dedicated_worker_rejects_cross_origin_script` — PASSED
  - `service_worker_registration_scope_matching_and_unregistration` — PASSED
  - `dedicated_worker_script_execution_and_bidirectional_messaging` — PASSED
  - `service_worker_offline_fetch_cache_interception` — PASSED
  - `document_navigation_terminates_worker_isolates` — PASSED
  - `shared_worker_shares_state_across_same_origin_documents` — PASSED
  - Total: 8/8 tests passing.
- Unit test suite:
  - All 29 unit tests in `engines/wega` passing.
- Servo binary compatibility:
  - `cargo check --bin wega` passing with 0 errors.
