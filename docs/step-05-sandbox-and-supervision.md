# Step 05 — content-process containment and supervision

Implementation delivered on `feature/standalone-wega-engine`, 1 November 2026.
Protocol 14 connects the browser, resource services and private worker. Native
acceptance is recorded separately below; cross-compilation is not runtime proof.

## Threat model and ownership

Treat HTML, CSS, scripts, images, fonts, WebAssembly, worker messages and worker
metrics as hostile. A compromised native content process must not acquire other
tabs' credentials, arbitrary user files, direct network access or child execution.
The coordinator, narrow resource services, packaged helper and kernel are trusted.
Administrator/kernel compromise is outside this boundary.

`PreparedLaunch` selects required containment before spawn, sanitizes credentials,
loader/proxy/runtime environment and inherited resources, and owns private launch
state until reaping. Required startup rejects missing native primitives, excessive
entitlements, legacy path/TCP grants and incompatible protocol versions. No Ready
packet or renderer claim substitutes for policy entry. Explicit debug-only
`WEGA_UNSAFE_DEVELOPMENT_WORKER=1` remains available for trusted development;
release builds reject that override. Library embeddings choose their own policy.

## Native backends

| Platform | Implemented boundary | Support/failure behavior |
| --- | --- | --- |
| Linux | Landlock ABI 6 filesystem/network/scoped restrictions, no allowed file paths; architecture-specific seccomp syscall allowlist; thread-only clone; no exec, process creation, arbitrary sockets, foreign signals/debugging; no-new-privileges, no dumps, parent-death handling and descriptor sanitation | ABI <6 or policy installation failure aborts startup. x86_64 and aarch64 syscall policies are explicit. Elevated workers are rejected. |
| macOS | Separately signed App Sandbox helper; hardened runtime and only App Sandbox/JIT security entitlements; no network or user-file entitlement; no child processes/core dumps; explicit inherited descriptors; ambient bootstrap namespace removed on initial-thread entry | Signature/entitlement checks run on the initial thread. Signing or sandbox setup failure aborts startup. Public Security/Mach APIs are used; no private Seatbelt profile API. |
| Windows | Suspended process in a per-worker AppContainer with LPAC and exactly the read-only `registryRead` runtime capability; explicit inherited handle list; one-process, memory-limited kill-on-close job attached before resume; child-process policy and job UI/clipboard/desktop restrictions | Missing AppContainer, handle-list or job policy aborts startup. Trusted helper copy/profile are retained through reaping and removed on normal cleanup. |

Windows LPAC needs the read-only `registryRead` capability for native runtime
initialization. It grants access where Windows ACLs explicitly permit that
capability; it grants no network, COM or arbitrary user-file capability. The child
verifies the exact derived SID rather than accepting any capability count. See
[Microsoft's LPAC model](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer).

The Rust `wega-content-bundle` tool creates/signs the nested macOS helper at
`Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine`. Packaging invokes it;
the browser prefers this helper. Development signatures are ad hoc unless
`WEGA_CODESIGN_IDENTITY` is supplied. This proves local confinement, not Developer
ID distribution or notarization.

## Explicit resources and privileged services

Four preconnected channels plus one fixed frame backing are inherited on Unix and
Windows. Server endpoints bind a session independently of caller-supplied tokens;
selecting inherited transport permanently disables TCP for that session. Numeric
FDs/HANDLEs are validated and duplicated into owned objects before VM startup.
No renderer receives profile paths, a broker listener address or a frame path.
Revocation shuts down active/idle channels and WebSockets and rejects late grants.
Unix workers lower the open-descriptor ceiling to 128 after duplicating grants.
Windows pipes use overlapped I/O with cancellation/draining and disconnect on revoke.

| Service | Host-owned authority and bounds |
| --- | --- |
| Fetch | Browser-approved navigation URL or pinned observed document; destination/mode/credentials checked; host Origin/cookies, validated Referer, redirect handling, CORS/preflight and DNS address pinning/private-address policy |
| Cookies | Script calls limited to the bound document's tuple origin; no script HttpOnly visibility or Netscape/network-cookie export |
| Storage | Origin/partition-bound SQLite using the existing origin hash/schema; persistent data preserved, transient data session-isolated; 10 MiB/4096 keys, bounded database caches |
| Files | Browser-selected file's directory, canonical containment, regular-file and bounded-byte checks; no worker path opens |
| Fonts | Host-selected sans/serif/monospace bytes, at most 8 MiB each; worker font discovery disabled |
| WebSockets | Host DNS/TLS/upgrade, Origin/cookies and subprotocol checks; 8/session, 64/global including pending; 64 KiB chunks, bounded I/O and revocation |
| Frames | Two fixed slots, credit acknowledgements and document/sequence/dimension checks; host positional copies validate bounds and tolerate hostile truncation without SIGBUS |

Deno's existing WebStorage/WebSocket JavaScript/WebIDL layers are unchanged. Small
pinned Rust backend patches route their operations to these services. Deno retains
WebSocket framing/events over a broker-owned byte stream. Direct library/debug
paths are not selected by the bound production worker.

Request policy is still a bounded browser implementation, not complete Fetch/CSP,
ORB, cache or certificate-policy conformance. Step 04's full audit remains separate:
renderer-initiated new navigation/POST authorization and the existing extension-based
CSP decisions need that work. These limitations do not grant the worker ambient
OS file/network authority.

## Budgets and recovery

| Resource | Default limit/enforcement |
| --- | --- |
| Live workers | 8, admission before spawn |
| Resident memory | 512 MiB/worker, 2 GiB aggregate, native sampling every 250 ms |
| Shared frames | 67.5 MiB/worker, 1 GiB aggregate, reserved before allocation |
| Startup | 20 seconds to Ready |
| Shutdown | At most one-second grace, then terminate and reap |
| Bitmap decoding | 8192-axis/8-million-pixel limits; 64 MiB decoder allocation ceiling |
| Images | 128 MiB pre-decode reservations, 256 MiB combined retained/in-flight accounting, 4096 entries |
| SVG | 8 MiB expanded source, 16384 XML nodes, bounded nested/data images, no external file resolver |

Image charges follow weak references to the actual retained objects rather than
being freed when a cache key disappears. SVG native-tree charges are conservative
estimates. Sampled RSS can overshoot between observations and does not hard-bound
all native, canvas, display-list or host GPU allocations. Windows also enforces a
committed-memory job limit; that is a different measurement from residency. These
controls are not a proof against every host OOM workload.

`OwnedProcess` retains budget tickets, handles, launch directories and jobs until
actual exit/reaping. Pending kills do not free admission capacity. Frame owners
retain their reservation while presenting the last frame. Native telemetry failure,
startup expiry, process exit, protocol failure and resource pressure revoke grants
before termination. Desktop memory reporting uses host samples, never worker RSS.
Failure clears queued native actions/dialogs/captures and offers explicit restart;
fatal script termination does not silently retry the workload through Servo.

## Validation and remaining platform evidence

The final local pass passed 226 workspace, 28 brokered, 106 core and 47 desktop
tests, plus vendor integrity (11 crates), strict workspace lints, formatting and
the desktop build. Native macOS probes have
passed denial of private read/write, direct TCP/UDP and child execution, alongside
an explicit file grant. A separately signed required worker has passed real JIT,
storage and cookie operations; nontransparent shared-frame pixel validation passed on the same required path.

`standalone-engine-validation.yml` runs the native probe, required-worker JIT/
storage/cookie/pixel fixture and full suite on Linux, macOS and Windows. Unsupported
sandbox setup is a test failure, never a successful denial. Current exact-head
results belong in `validation/standalone/progress.json`. Linux/Windows source
cross-checks alone do not satisfy the three-platform acceptance gate.

Remaining release evidence includes native Linux/Windows results, parent-crash and
pressure/restart stress across all platforms, distribution signing/notarization,
and whole-browser resource calibration. A parent crash may leave Windows profile
metadata/trusted helper copies for later cleanup; kernel job lifetime still kills
the worker. Broader browser parity and Servo removal remain later plan steps.

Primary references: [Landlock ABI](https://docs.kernel.org/userspace-api/landlock.html),
[seccomp](https://docs.kernel.org/userspace-api/seccomp_filter.html),
[AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer),
[Windows job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects),
[Apple App Sandbox](https://developer.apple.com/documentation/security/protecting-user-data-with-app-sandbox),
[Apple JIT entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.allow-jit).
