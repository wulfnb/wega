# P4–P6 iteration

Branch: `feature/wega-p4-performance`, started from merged main `5d7c1fb`;
integrated `5b284ed` and then merged latest main `634168b` (PR #9 web
correctness) before final validation. Scope: implement P4–P6 together before the
next CI handoff. Checkmarks below record engineering scope; performance targets
and platform gates are separate. Servo remains the general default; Wega rollout
is limited to validated sites.

- [x] P4: embedded startup snapshot of compiled binding/bootstrap factories;
      restore per-document Web API, origin, permissions and storage state at
      runtime.
- [x] P4: native worker RSS, bounded V8/style pools, lite-worker policy, freeze
      GC and allocator/cache release with state-preservation regressions.
- [x] P4: shared browser network broker, bounded HTTP cache and profile cookie
      jar.
- [x] P4: owned display lists, asynchronous raster worker, bounded shared-memory
      frames.
- [x] P4: repeatable compatibility/startup/active/frozen resource measurements.
- [x] P5: Skia raster backend, CSS feature/pixel regressions and pinned CSS WPT
      subset; documented/upstream-ready patches for demonstrated Blitz gaps.
- [x] P6: persisted per-site engine preference, address-bar engine badge, safe
      one-way Servo fallback and a narrow validated default-on allowlist.
- [x] Combined local engine/browser/tooling checks, packaging/CI dependency
      audit, recorded measurements and explicit remote-platform validation
      boundaries.

The pre-change development resource report is
`/tmp/wega-p4-before/results.json`. It uses debug builds and is not an
optimized-release performance certification. Historical JS heap figures are
never treated as process RSS. Frozen live-page state must survive; a memory
target does not authorize destructive tab discard.

## Merge and release integration

Latest main's web-correctness work is retained: CORS/preflight/response
filtering, SameSite cookies on each hop, CSP, referrer policy, parser/script
scheduling, forms, encodings, history, dialogs and full pointer input. Deno's
Fetch algorithms now use the shared broker transport, with bounded response
resources feeding its existing stream lifecycle. Two lightweight dispatchers
allow resources to finish out of order; HTTP/TLS clients and the four network
workers live in the browser. The same security fixtures run with both the local
library and broker transports.

Protocol version 4 combines both branches' messages; an older worker fails the
version handshake. Pointer-down input participates in edited-page protection.
Offline mode blocks HTTP but preserves script access to local cookies. A
brokered library view also enforces its live local policy when the broker
session is broader. Fetch cache modes reach the broker; no-store bypasses
storage, cache-only misses never start HTTP, and reload refreshes the cached
response body.

The reported older CD failure was the Windows URL-handoff title readiness race,
fixed by `c7592ce` on main. Latest main's complete
[CD run 40349998980](https://github.com/wulfnb/wega/actions/runs/40349998980)
passed Windows/macOS/Linux packaging, container validation and GitHub release
publishing. That result certifies main `634168b`, not this unpushed combined
branch. The main CI cache and parallel packaging changes are preserved.

## Validation boundaries

The engine target `target/p4-p6-clean` was built from scratch for this
iteration, then rebuilt as source and main integration changed. A clean build
caught a macOS fixture socket inheritance race; the fixture now explicitly uses
blocking accepted sockets. Combined local engine tests, broker fixtures, fault
proofs and native browser results are recorded below.

The frozen-tab target is **under 10 MB RSS**; it remains a gate, not permission
to discard a live isolate, draft, DOM or sign-in state. Images held by live
nodes stay available offline after thaw. Freeze drops redundant image/raster
caches and frame mappings, requests V8 collection and trims glibc allocations.
Existing app isolates keep their JIT state; lite mode is chosen only at process
creation.

Skia/CSS tests cover demonstrated rendering gaps, seven pinned CSS WPT
assertions, an overflow reftest and eleven pixel/layout regressions. This is a
tested subset of CSS, not full conformance. Exact vendor patches and archive
hashes are reviewable; upstream PRs have not been submitted. WebSocket still
uses Deno's native transport.

Local macOS results do not certify Linux/Windows runtime or distribution
behavior for this branch. Broad-site captures measure load only; they do not
establish functional compatibility for editors or applications. Servo remains
the general default; only exact HTTPS origins example.com/org/net on port 443
are default-on. Preferences route address-bar, new-tab and supported browser
navigations. In-page POST submissions and engine-owned history keep their
method, payload and current engine.

Reference-pixel review also exposed a fragment insertion bug inherited from the
web-correctness merge: `body.insertAdjacentHTML("afterbegin", ...)` parsed
through an HTML-root context, inserting nested head/body nodes and applying body
padding twice. Insertion now chooses its context by position and preserves
table/SVG namespaces, following the
[HTML insertion algorithm](<https://html.spec.whatwg.org/multipage/dynamic-markup-insertion.html#the-insertadjacenthtml()-method>).
Reverting the fix fails both a DOM context test and a rendered SVG position
test.

## Final local validation

Final implementation source: `f529ac88fe41`; Apple M4, 16 GiB RAM, macOS ARM64.
Both binaries use debug builds. The browser includes `test-harness`; no build or
fault injection ran during the final benchmark.

Passed: 110 workspace Rust tests (engine, network and platform), 31 DOM WPT
assertions, seven CSS WPT assertions, eleven CSS pixel/layout tests, 25 brokered
HTTP fixtures, 84 core tests, 84 + 34 desktop tests, 71 Python tests, all nine
native suites, and strict core/desktop/engine lints. The worker-native suite was
rerun after the fragment fix. One generated Deno-extension doctest is ignored
upstream; all six framework acceptance fixtures remain enabled.

All 26 targeted P4–P6/merge regression faults were detected. The vendor verifier
matches all eight pinned archives and rejects an undocumented edit. Actual
desktop and engine license notices were generated with
`cargo-about --locked --fail`.

[Machine-readable evidence](validation/p4-p6-2026-10-26.json) records
fingerprints, raw per-tab RSS, all reference outcomes and fault cases. Full
reports, screenshots and fault logs are preserved under local
`target/p4-p6-validation/`. Original run directories are
`/tmp/wega-p4-p6-bench-validated`, `/tmp/wega-p4-p6-native-final`,
`/tmp/wega-p4-p6-native-svg-fix` and `/tmp/wega-p4-p6-merged-mutations`.

### Fixed local article workload

One repeat, one and five loaded tabs, three-second idle window, fresh profiles.
RSS sums the browser and descendant processes; shared pages may be counted more
than once. This is a development comparison with uncontrolled OS caches,
including the merged web-correctness changes. It is not release certification or
a statistical performance claim.

| Build            | Engine | Tabs | Ready (s) | Tree RSS (MiB) | Idle CPU (%) |
| ---------------- | ------ | ---: | --------: | -------------: | -----------: |
| Before (5d7c1fb) | servo  |    1 |      2.98 |          212.6 |         0.00 |
| Before (5d7c1fb) | servo  |    5 |      1.89 |          225.2 |         0.00 |
| Before (5d7c1fb) | wega   |    1 |      1.69 |          223.0 |         1.14 |
| Before (5d7c1fb) | wega   |    5 |      3.60 |          450.5 |         1.03 |
| Final            | servo  |    1 |      0.67 |          224.4 |         0.38 |
| Final            | servo  |    5 |      1.44 |          239.4 |         0.00 |
| Final            | wega   |    1 |      1.06 |          330.9 |         0.35 |
| Final            | wega   |    5 |      2.13 |          620.7 |         0.77 |

Frozen Wega workers in the five-tab run used **74.1–87.9 MiB RSS**, while their
V8 heaps were around 3 MiB. The **10 MB frozen target is unmet**. The one-tab
article target below 100 MB is not reliably met either; see per-tab bytes in the
evidence. Wega uses more process-tree RSS than Servo here. Preserve drafts and
live isolate state instead of discarding a tab to improve the number.

### Ten live reference sites

These are load/idle observations with automatic fallback disabled and the actual
engine checked. A title or screenshot is `captured_unverified`, not a functional
site pass. Application memory targets cannot be certified by a partial or failed
application load.

| Site                                                      | Servo tree (MiB) | Wega tree (MiB) | Wega worker (MiB) | Wega capture                             |
| --------------------------------------------------------- | ---------------: | --------------: | ----------------: | ---------------------------------------- |
| https://example.com/                                      |            236.8 |           482.6 |             265.1 | captured_unverified                      |
| https://example.org/                                      |            237.2 |           451.4 |             265.8 | captured_unverified                      |
| https://example.net/                                      |            236.8 |           385.5 |             193.5 | captured_unverified                      |
| https://en.wikipedia.org/wiki/Rust_(programming_language) |            389.5 |           376.0 |             139.2 | captured_unverified; compatibility error |
| https://developer.mozilla.org/en-US/                      |            315.2 |           307.9 |             101.1 | captured_unverified                      |
| https://github.com/                                       |                — |           223.3 |              33.2 | error                                    |
| https://news.ycombinator.com/                             |            239.0 |           274.9 |              86.7 | captured_unverified                      |
| https://react.dev/                                        |            326.5 |           399.5 |             173.5 | captured_unverified                      |
| https://angular.dev/                                      |            371.8 |           771.5 |             558.1 | captured_unverified                      |
| https://vuejs.org/                                        |            278.6 |           291.6 |              96.3 | captured_unverified; compatibility error |

All seven bounded API probes pass for both engines; the required Wega set now
includes events, forms, storage and fetch. Site-specific failures and error
titles remain visible in the JSON rather than earning compatibility credit.

**Still open:** the P4 RSS budgets, full app/website compatibility, broader CSS
conformance and upstream submissions, and Linux/Windows runtime plus remote CI
for this branch. Main’s green CD result predates this branch. Servo remains the
general default.

The three final allowlist screenshots were visually reviewed after the fix. Text
is readable, the accidental second body is absent, and the example SVG starts at
133 CSS pixels in a 708 CSS-pixel viewport, matching `25vh - 44px`. These
observations do not claim pixel identity with Servo.
