# P4–P6 iteration

Branch: `feature/wega-p4-performance`, started from merged main `5d7c1fb`; integrated `5b284ed` and then
merged latest main `634168b` (PR #9 web correctness) before final validation.
Scope: implement P4–P6 together before the next CI handoff. Checkmarks below
record engineering scope; performance targets and platform gates are separate.
Servo remains the general default; Wega rollout is limited to validated sites.

- [x] P4: embedded startup snapshot of compiled binding/bootstrap factories;
  restore per-document Web API, origin, permissions and storage state at runtime.
- [x] P4: native worker RSS, bounded V8/style pools, lite-worker policy, freeze GC
  and allocator/cache release with state-preservation regressions.
- [x] P4: shared browser network broker, bounded HTTP cache and profile cookie jar.
- [x] P4: owned display lists, asynchronous raster worker, bounded shared-memory frames.
- [x] P4: repeatable compatibility/startup/active/frozen resource measurements.
- [x] P5: Skia raster backend, CSS feature/pixel regressions and pinned CSS WPT subset;
  documented/upstream-ready patches for demonstrated Blitz gaps.
- [x] P6: persisted per-site engine preference, address-bar engine badge,
  safe one-way Servo fallback and a narrow validated default-on allowlist.
- [ ] Combined engine/browser/tooling checks, packaging/CI dependency audit,
  recorded measurements and explicit remote-platform validation boundaries.

The pre-change development resource report is `/tmp/wega-p4-before/results.json`.
It uses debug builds and is not an optimized-release performance certification.
Historical JS heap figures are never treated as process RSS. Frozen live-page
state must survive; a memory target does not authorize destructive tab discard.

## Merge and release integration

Latest main's web-correctness work is retained: CORS/preflight/response filtering,
SameSite cookies on each hop, CSP, referrer policy, parser/script scheduling,
forms, encodings, history, dialogs and full pointer input. Deno's Fetch algorithms
now use the shared broker transport, with bounded response resources feeding its
existing stream lifecycle. Two lightweight dispatchers allow resources to finish
out of order; HTTP/TLS clients and the four network workers live in the browser.
The same security fixtures run with both the local library and broker transports.

Protocol version 4 combines both branches' messages; an older worker fails the
version handshake. Pointer-down input participates in edited-page protection.
Offline mode blocks HTTP but preserves script access to local cookies. A brokered
library view also enforces its live local policy when the broker session is broader.
Fetch cache modes reach the broker; no-store bypasses storage, cache-only misses
never start HTTP, and reload refreshes the cached response body.

The reported older CD failure was the Windows URL-handoff title readiness race,
fixed by `c7592ce` on main. Latest main's complete
[CD run 40349998980](https://github.com/wulfnb/wega/actions/runs/40349998980)
passed Windows/macOS/Linux packaging, container validation and GitHub release
publishing. That result certifies main `634168b`, not this unpushed combined branch.
The main CI cache and parallel packaging changes are preserved.

## Validation boundaries

The engine target `target/p4-p6-clean` was built from scratch for this iteration,
then rebuilt as source and main integration changed. A clean build caught a macOS
fixture socket inheritance race; the fixture now explicitly uses blocking accepted
sockets. Combined local engine tests, broker fixtures, fault proofs and native
browser results are recorded below once the final runs complete.

The frozen-tab target is **under 10 MB RSS**; it remains a gate, not permission to
discard a live isolate, draft, DOM or sign-in state. Images held by live nodes stay
available offline after thaw. Freeze drops redundant image/raster caches and frame
mappings, requests V8 collection and trims glibc allocations. Existing app isolates
keep their JIT state; lite mode is chosen only at process creation.

Skia/CSS tests cover demonstrated rendering gaps, seven pinned CSS WPT assertions,
an overflow reftest and eleven pixel/layout regressions. This is a tested subset of
CSS, not full conformance. Exact vendor patches and archive hashes are reviewable;
upstream PRs have not been submitted. WebSocket still uses Deno's native transport.

Local macOS results do not certify Linux/Windows runtime or distribution behavior
for this branch. Broad-site captures measure load only; they do not establish
functional compatibility for editors or applications. Servo remains the general
default; only exact HTTPS origins example.com/org/net on port 443 are default-on. Preferences route
address-bar, new-tab and supported browser navigations. In-page POST submissions
and engine-owned history keep their method, payload and current engine.

Reference-pixel review also exposed a fragment insertion bug inherited from the
web-correctness merge: `body.insertAdjacentHTML("afterbegin", ...)` parsed through
an HTML-root context, inserting nested head/body nodes and applying body padding
twice. Insertion now chooses its context by position and preserves table/SVG
namespaces, following the
[HTML insertion algorithm](https://html.spec.whatwg.org/multipage/dynamic-markup-insertion.html#the-insertadjacenthtml()-method).
Reverting the fix fails both a DOM context test and a rendered SVG position test.
