# Step 05 — content-process containment and supervision

Status, 31 October 2026: **in progress**. This branch implements the independent
supervision and launch-policy foundation while Steps 03/04 are developed in a
separate clone. Full sandbox activation depends on merging Step 04 and completing
native resource mediation, platform backends, signing and runtime probes.
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

The existing TCP broker and path-based storage/frame handoff do not satisfy this
allowlist. The debug escape hatch below preserves explicit development access
while their replacements are implemented.

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
The current descriptor policy retains only standard streams; Step 04 inherited
broker/frame descriptors require explicit allowlisted remapping before activation.
This is not a filesystem/network/child-execution sandbox.

On Windows, a non-inheritable unnamed job is configured with kill-on-last-close,
one active process, committed-memory limits and disabled exception error boxes.
Assignment finishes before the host sends Init containing content. Failure to
create/configure/assign a job aborts startup and the owned process is reaped.
The job handle stays alive through reaping. AppContainer/restricted-token isolation
and an audited handle list are still required; a job alone is insufficient.

## Required mode and explicit development mode

The browser and private worker now fail closed in required mode. Since native
content sandboxes and the replacement resource transport are unfinished,
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

## Merge contract for Steps 03/04

The other AI's work remains on its separate branch. No network implementation or
harness features are implemented here. When merging, preserve these boundaries:

1. Step 04 must supply preconnected/inherited service endpoints with broker-owned
   authority. Do not enable a TCP-port exception as a substitute for denying
   arbitrary network access across platforms. Landlock's TCP rights select ports,
   not a unique authenticated peer. Preserve service/document/session revocation.
2. Frame and storage paths in Init must become native handle/operation grants.
   Add font/file brokers and descriptor/handle ownership before enabling required
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
6. Resolve overlapping edits in `src/wega_page.rs`, `engines/wega/src/main.rs`,
   `engines/wega/tests/worker.rs`, `engines/platform/src/process.rs` and progress
   documents deliberately. Shared protocol wire version remains 11 in this slice.

## Remaining Step 05 work

- Merge Step 04 and finish brokered fonts/files/storage and narrow native handles.
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
close, policy opt-in and process reaping. They have **not been executed**.
Cross-compilation is not runtime or packaging proof. Full Step 05 stays unchecked.

Primary design references:

- [Linux Landlock ABI and network rules](https://docs.kernel.org/userspace-api/landlock.html)
- [Linux seccomp filtering and its scope](https://docs.kernel.org/userspace-api/seccomp_filter.html)
- [Windows job-object lifetime](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
- [Windows job limits and committed memory](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
- [Apple allow-jit entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.allow-jit)
- [Apple silicon JIT requirements](https://developer.apple.com/documentation/apple-silicon/porting-just-in-time-compilers-to-apple-silicon)
