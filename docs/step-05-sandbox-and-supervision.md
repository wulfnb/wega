# Step 05 — content-process containment and supervision

Status, 1 November 2026: **in progress**. Steps 03/04 were merged at `62269d9`.
This branch integrates their protocol/harness additions with supervision and adds
inherited Unix service/frame grants under protocol v13. The merge supplies partial
Step 03/04 implementations, not their full acceptance criteria. Full sandbox
activation still needs authoritative resource mediation, platform backends,
signing and runtime probes.
No part of this document claims a verified OS content sandbox.

## Threat model

Assume arbitrary hostile HTML, CSS, images, fonts, WebAssembly and JavaScript.
Assume an exploited V8, Blitz, Skia, decoder or binding can execute native code in
the content process and fabricate every renderer-originated message and metric.
The browser coordinator, narrow privileged services, signed application resources
and operating-system kernel are the trusted computing base. Protect other tabs,
profile/container credentials, user files, devices and host availability. Local
administrator compromise and kernel exploits are outside this boundary; ordinary
renderer privilege escalation, inherited authority and child-process escape are
inside it. Never run a required worker with elevated browser privileges retained.

The protocol's tab/frame/process identities prevent routing confusion but do not
confine native code. The kernel must enforce file, network, process and handle
restrictions before untrusted content is dispatched. Renderer-reported success,
an environment flag, a signed executable or a Ready packet alone is not proof.

## Resource allowlist required for activation

| Resource | Allowed authority | Denied authority |
| --- | --- | --- |
| Browser control | Inherited private command/event pipes, bounded messages | Discovering or connecting to another browser/session's control channel |
| Network | Preconnected/inherited broker endpoints, host-issued session/document authority | Arbitrary socket creation/connect/bind, DNS, loopback/private-network bypasses |
| Storage | Broker operations on the current origin/partition | Profile paths, cookie stores, credential databases, direct local/session storage files |
| Frames | Explicitly owned fixed-size mappings/handles with credit acknowledgement | Arbitrary paths, enlarging mappings, stale frames or unbounded surface allocation |
| Files | Browser-selected resources as narrow read-only handles/bytes | Arbitrary user/profile files, directory enumeration, path-based file opens |
| Fonts | Broker-selected system/user font bytes or bounded native handles | Broad filesystem/font-cache access, font-manager IPC not included in the policy |
| Temporary data | One owner-only directory where a backend needs it | Shared writable working directories, inherited profile HOME and arbitrary temp paths |
| Code/JIT | Verified packaged libraries, V8's required JIT mappings | Child execution, library injection, arbitrary executable files or unnecessary signing exceptions |
| Processes/devices | Necessary threads, clocks and anonymous synchronization | Child processes, ptrace/debugging other processes, signals to other sessions, device APIs without a broker grant |

On macOS/Linux, the browser now passes preconnected Unix broker channels and a
frame file descriptor. Path-based profile storage, direct worker network/file
access and the Windows development TCP/frame-path handoff still violate this
allowlist. The debug escape hatch below remains necessary.

## Inherited grants and merge integration

The browser selects four connected Unix channels before spawn. Each server endpoint
is bound to the host's service object independently of the token inside a packet;
a different service's valid token cannot transfer authority. Selecting inherited
channels permanently disables TCP access for that session. The broker retains its
legacy TCP listener for other callers, but these Unix workers receive no broker
address. Closed or failed channels never reconnect over TCP.

The renderer uses a bounded four-channel pool. Each channel serializes request,
body and response; the server waits for its bounded network job to finish before
reading another packet. Partial bodies, failed writes, malformed envelopes and
panics close that channel. Revocation shuts down published endpoints and prevents
late publication during setup; it unblocks idle and active broker I/O. Channel
admission waits at most two seconds, packet reads use a 20-second timeout, and
writes use five seconds. Idle server endpoints wait for explicit revocation/EOF.
These are I/O timeouts, not hard wall-clock deadlines against a trickling peer.
Hostile network traffic remains subject to the existing global job/body bounds.

Only explicit broker/file grants regain inheritance after Linux's descriptor
sanitization. Native validation rejects standard streams, repeated descriptors,
non-Unix/unconnected sockets and invalid frame backing sizes before VM startup.
The worker duplicates grants into safe owned types and restores CLOEXEC on the
original inherited descriptors. Originals remain bounded to four channels and one
frame file until process exit; no safe API adopts an arbitrary caller-owned FD.
The host retains its frame file and resource reservation through worker reaping.

The browser's `FrameReader` exposes bounded positional copies, with no mapping API.
It validates slot, byte count and backing length; a worker that truncates the
file produces an I/O failure instead of a browser SIGBUS. The producer still uses
its own mapping. This is not a sealed file or immutable snapshot: hostile workers
can corrupt their frame bytes or fault their own mappings. Content remains
untrusted and dimensions/sequence/document scope are checked independently.

Protocol v13 adds inherited broker/frame capabilities, with defaulted optional
fields preserving older Init JSON. Windows does not advertise these Unix grants;
its explicit debug path retains TCP and frame paths until native handle handoff
is implemented. Required/release mode remains closed on every platform.

The merged Capture/Navigate/Diagnostic events now have desktop routing, bounded
correlation and payload handling. The subprocess harness explicitly opts into
development mode, bounds stderr/diagnostic/event buffers, escapes selectors and
preserves engine/crash/transport failures. Capture-only raster targets return
their unpublished credits. Worker network adapters now propagate the initiating
document origin and actual fetch context rather than labeling CDN requests with
the CDN's origin. Server-controlled HSTS durations and entry counts are bounded;
this is not a claim of complete HSTS semantics or persistence.

## Implemented foundation

`engines/platform/src/supervision.rs` owns admission and process monitoring.
It samples native child residency through the retained `Child`/process handle,
not the State packet's `rss_bytes`. Unix sampling holds the unreaped child owner,
so a recycled PID cannot be sampled or killed in place of the real process.
Missing telemetry is not counted as zero; three failed memory samples request
termination. Native exit, startup deadline and monitoring failures also revoke
the page's broker session and invalidate its document authority.

Default provisional budgets:

| Budget | Limit | Enforcement |
| --- | --- | --- |
| Live content workers | 8 | Admission before spawn |
| Worker resident memory | 512 MiB | Native sampling every 250 ms; terminate on excess |
| Aggregate live worker residency | 2 GiB | Stop largest consumers until enough reclamation is pending |
| One worker's shared frame backing | 67.5 MiB | Two fixed maximum-size slots, reserved before allocation |
| Aggregate shared frame backing | 1 GiB | Checked admission, including stopped pages retaining presentation resources |
| Startup to Ready | 20 seconds | Host clock; a late Ready cannot cancel a selected termination |
| Normal shutdown grace | At most 1 second | Existing owned-process reaper, then termination and wait |

These numbers need workload calibration during the final resource pass. Sampling
is a recovery policy, not a hard kernel memory ceiling: allocation can overshoot
between samples. It does not account for the entire browser, GPU process/cache,
all decoded image storage, or every display-list allocation. Do not describe it
as complete host-OOM protection. On Windows the job additionally caps committed
memory at the configured worker limit; committed memory and resident bytes are
different measurements.

`OwnedProcess` exposes a weak control handle and retains budget tickets, private
directories and native containment handles through actual exit/reaping. A page
drop or an enqueued kill does not release a live process reservation. Presentation
owners retain their frame reservation even after OS exit. Termination callbacks
run outside the accounting lock, revoke grants before killing, and execute once.
Pending kills are not treated as already-free memory for admission; they are
considered when choosing additional aggregate-pressure victims to avoid a kill
cascade. Termination is retried while the process remains live.

The host clears dialogs, queued actions, pending captures and buffered frames on
failure. Stopped pages show the reason and explicit restart/Servo actions; failure
does not automatically relaunch the same workload in another engine. Ordinary
unsupported-feature compatibility fallback remains a separate path.

`engines/platform/src/launch.rs` uses an absolute executable, clears inherited
credentials/loader/proxy/runtime environment, sets a dedicated working/temp
directory (created with mode 0700 on Unix), and retains it until process exit.
Only bounded locale/timezone values, required Windows system-directory variables,
explicit temp paths and the allocator setting are passed. This is launch hygiene,
not a native isolation boundary. Windows ACL/handle inheritance must still be
audited alongside the AppContainer design.

On Linux, the parent establishes `PR_SET_PDEATHSIG` with a parent-death race check,
`NO_NEW_PRIVS`, disabled core dumps and `close_range(CLOEXEC)` for non-stdio
descriptors. The worker disables dumpability **after exec**, which can reset that
attribute. Unsupported setup fails instead of silently omitting a restriction.
The descriptor policy now preserves standard streams and explicit inherited
broker/frame grants. Grant preservation is registered after the CLOEXEC sweep.
This is not a filesystem/network/child-execution sandbox.

On Windows, a non-inheritable unnamed job is configured with kill-on-last-close,
one active process, committed-memory limits and disabled exception error boxes.
Assignment finishes before the host sends Init containing content. Failure to
create/configure/assign a job aborts startup and the owned process is reaped.
The job handle stays alive through reaping. AppContainer/restricted-token isolation
and an audited handle list are still required; a job alone is insufficient.

## Required mode and explicit development mode

The browser and private worker now fail closed in required mode. Since native
content sandboxes and resource mediation are unfinished,
**release Wega browsing is blocked on this branch**. Replacing TCP with an
inherited endpoint does not by itself satisfy the platform gate.

For an explicitly unsandboxed **debug** browser session only:

```sh
WEGA_UNSAFE_DEVELOPMENT_WORKER=1 target/debug/wega --engine wega
```

Set `WEGA_ENGINE_BINARY` in the host if the debug worker is elsewhere. The trusted
launcher adds `--development-unsandboxed` to that worker's command line; the
worker accepts it only when compiled with debug assertions. The flag/variable
cannot enable an unsandboxed release worker. Development sessions report their
unsandboxed state. Run only fixtures/trusted development workloads in this mode.
The Rust subprocess regression fixture passes the development argument explicitly;
it is not evidence of sandboxed browsing. Library embeddings remain responsible
for their own host policy; these rules gate the shipped browser/private-worker path.

## Boundaries and remaining gaps after the Steps 03/04 merge

The merged implementation does not yet close the following boundaries. Keep
these requirements when extending it:

1. Unix service endpoints are inherited; Windows still needs a native equivalent.
   Do not enable a TCP-port exception as a substitute for denying
   arbitrary network access across platforms. Landlock's TCP rights select ports,
   not a unique authenticated peer. Preserve service/document/session revocation.
2. Unix frame paths are replaced; storage paths and Windows frame paths remain.
   Add font/file/storage brokers and descriptor/handle ownership before enabling required
   mode. A renderer-selected raw handle number or path is not an authority grant.
3. Keep `PreparedLaunch` before spawn; job/resource attachment before Init; native
   sandbox entry before V8/Blitz executes content; Ready after successful setup.
   An IPC field claiming “sandboxed” must not open the production gate.
4. Preserve the host's budget Ticket across UI, reader/writer and reaper owners.
   The first valid Ready marks startup complete; protocol/IO failure requests
   termination. Scope new harness callbacks to the same process/document session.
5. Direct debug worker fixtures need `--development-unsandboxed`. Direct-Wega
   release assertions must classify sandbox-unavailable as setup failure; Servo
   fallback and development captures cannot count as sandboxed Wega passes.
6. Shared protocol wire version is now 13. The desktop/worker/harness must negotiate
   capabilities together; an unavailable capability cannot grant native authority.

Step 03 still needs the full offline framework matrix (including Angular), WPT
serving/asynchronous/reftest/testdriver paths and final failure-classification
evidence. Compiling the new harness is not a direct-Wega behavioral pass.

Step 04's current origin check exempts renderer-declared top-level requests and
empty/opaque origins. Cookie calls are service-scoped but not document-scoped.
WebSockets and profile storage still have direct paths. Destination propagation
does not remove extension-based CSP decisions elsewhere. Private-network checks
are lexical, without DNS address pinning/rebinding protection; CORS, redirects,
module dependencies, certificate handling and caching need a complete authority
audit. Inherited transport prevents endpoint switching; it does not fix those
request-authority gaps. No source test is evidence that these bypasses are closed.

## Remaining Step 05 work

- Finish Step 04 authority enforcement, brokered fonts/files/storage and narrow
  Windows native handles. Preserve the Unix grant/revocation boundary.
- Implement Linux filesystem/process/network confinement, syscall and architecture
  policy, capability removal and supported-kernel detection. Combine kernel
  mechanisms deliberately; seccomp alone is not an information-flow sandbox.
- Implement a supported macOS helper sandbox and launch/signing model. Verify
  hardened runtime, `allow-jit`, V8 JIT behavior, dylibs and narrow IPC/resource
  exceptions together. This slice supplies no macOS content-sandbox entitlement.
- Implement Windows AppContainer/restricted-token policy and handle inheritance;
  prove job attachment/child restrictions and JIT compatibility on Windows.
- Bound decoder expansion and retained decoded-image allocations before allocation,
  including SVG/raster/canvas paths. Blitz's existing decode path still needs this
  work; compressed resource limits and sampled RSS do not solve decompression bombs.
- Audit all worker queues, display lists, host presentation/GPU allocations and
  aggregate browser pressure; add hard kernel limits where supported and define
  fail-closed release support when a required primitive is unavailable.
- Execute positive fixtures and adversarial probes on macOS, Linux and Windows:
  arbitrary profile/file reads/writes, process creation, direct TCP/UDP/Unix IPC,
  stale handles, parent crash, memory pressure, startup failure and repeated restart.
  Verify private directories, mappings, native handles and grants are released.

## Validation state

Only compilation/lints are performed now, per the owner's tests-at-the-end rule.
Platform test/probe sources cover reservation lifetime, native memory/aggregate
pressure, startup deadlines, descriptor sanitation, Windows descendant denial/job
close, policy opt-in and process reaping. New regression sources cover inherited
channel reuse/concurrency, service binding, TCP rejection, revocation, real-worker
broker/file descriptor handoff, truncated frame files, v13 compatibility, harness
reply scope and HSTS bounds. They have **not been executed**.
Cross-compilation is not runtime or packaging proof. Full Step 05 stays unchecked.

Primary design references:

- [Linux Landlock ABI and network rules](https://docs.kernel.org/userspace-api/landlock.html)
- [Linux seccomp filtering and its scope](https://docs.kernel.org/userspace-api/seccomp_filter.html)
- [Windows job-object lifetime](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
- [Windows job limits and committed memory](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
- [Apple allow-jit entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.allow-jit)
- [Apple silicon JIT requirements](https://developer.apple.com/documentation/apple-silicon/porting-just-in-time-compilers-to-apple-silicon)
