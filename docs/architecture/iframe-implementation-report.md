# Iframe implementation report

**Verdict: PARTIAL / CONDITIONAL GO.** Iframe browsing contexts work end to end in the engine
and in the real native window with the signed sandboxed worker for same-origin frames, nested
frames and sandboxed `srcdoc`. Four acceptance items are **not met** and are listed in §5.
Companion documents: [design](./iframe-design.md), [security review](./iframe-security-review.md),
[WPT matrix](./iframe-wpt-compatibility.md), [evidence](./baseline/iframe-evidence.json).

## 1. What was built

A child browsing context is a nested `View` owned by its parent (`engines/wega/src/frames.rs`):
own document, own V8 isolate, own network lifecycle, console and revision. Frame identity
(`key`, never reused) is separate from document identity (`document_revision`).

| Area | Implementation |
|---|---|
| Lifecycle | creation (parser- and script-created), `src`/`srcdoc`/`sandbox` attribute navigation, removal, parent-navigation teardown, limits (depth 8, 32 frames/tab, 2 creations per turn), `load` events on the element, parent `load` waits for frames |
| Origins | principal from the final URL; `srcdoc`/`about:blank` inherit; sandbox → opaque (no cookies, storage, workers, `Origin: null`); opaque frames cannot be authorised as a tuple origin |
| Sandbox | enforced: scripts (no realm), same-origin, forms, popups, modals; top navigation always denied; unenforced tokens stay denied |
| Policy | parent `frame-src`/`child-src`/`default-src`; child `frame-ancestors`; `X-Frame-Options`; CSP inheritance for `srcdoc`/`about:blank` |
| Windows | host-backed `WindowProxy` facade (`contentWindow`, `parent`, `top`, `frames`, `length`, named access), cross-origin-safe surface only, `SecurityError` otherwise |
| Messaging | host-routed `postMessage`; `targetOrigin` matched against the receiver's committed principal; host-computed `event.origin`; `event.source` facade for adjacent frames |
| Rendering | child scene composited into the parent scene at the content box with a clip (`PaintSource::compose_child`); no flattening, no second layout engine; each child lays out itself |
| Input | pointer/click/wheel consumed by the frame under the pointer with coordinate conversion; keyboard/IME to the single focused frame |
| DevTools (engine) | `View::frame_tree`, `View::frame_target`: each frame is a D3 target with its own stamp, node handles, grants |
| Infrastructure fixes | multi-isolate-per-thread V8 host safety; Blitz's own iframe loader disabled (see security review F-1) |

## 2. Test counts (exact commands)

All run from `/Users/noufalbabu/dev/personal/wega` on macOS (Apple silicon), debug unless noted.

| Gate | Command | Result |
|---|---|---|
| Engine workspace | `cd engines && cargo test --workspace --locked --no-fail-fast` | see `iframe-evidence.json` (`engine_workspace`) |
| Iframe tests | `cargo test -p wega-engine --test iframes --test iframe_inspector --test frames` | 17 + 1 + 6 passed |
| Frame policy unit tests | `cargo test -p wega-security --lib frame_policy` | 3 passed |
| Broker mode | `WEGA_TEST_BROKER=1 cargo test -p wega-engine --test iframes --test iframe_inspector` | 17 + 1 passed |
| Root lib, no default features | `cargo test --locked --no-default-features --lib` | 109 passed |
| Root clippy | `cargo clippy --locked --no-default-features --all-targets -- -D warnings` | clean |
| Engine clippy | `cd engines && cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| Formatting | `cargo fmt --check` (root and `engines`) | clean |
| Architecture boundary | `engines/tools`: `cargo run -p wega-architecture-check -- --manifest-path ../Cargo.toml` | 0 violations |
| Servo ban / dependency graph | `python3 scripts/check-engine-boundary.py` | pass |
| Vendor integrity | `python3 scripts/verify-engine-vendor.py` | pass |
| Native suites, signed worker | see §4 | smoke, download, webdriver, capture, engine: PASS |
| Existing DOM WPT (35 files / 1685 cases), CSS-WPT, pixel reftest | included in the engine workspace run | pass (no failures) |
| Iframe WPT | none run | see WPT matrix |

Not run: Linux, Windows, Docker suites (no host); the coverage gates; `scripts/native-tests.py`
`media` suite (explicitly unsupported before this work); native App Sandbox containment probe
and docked/detached native DevTools tests were **not re-run** (they are D3 harnesses that
need the host UI flows; this milestone did not touch them, but no claim is made for them).

### The D3 WebDriver title failure

Reproduced with a freshly built, ad-hoc-signed bundled helper and `WEGA_CONTENT_HELPER` set:
`scripts/native-tests.py --suite webdriver` **passes**; `smoke`, `download`, `capture`, `engine`
pass too (these need a `--features test-harness` desktop binary; without it they fail with
"Native test modes require --features test-harness", which is a different, expected message).
The D3 failure ("New tab" title) was therefore a helper/configuration artefact (the script's
own comment: an unsigned adjacent worker never navigates), not a browser regression. It was
not attributed to iframes.

Commands:
```
cd engines && cargo build --locked -p wega-engine --bin wega-engine
cd .. && cargo build --locked --features test-harness
engines/target/debug/wega-content-bundle engines/target/debug/wega-engine /tmp/EngineApp
WEGA_CONTENT_HELPER=/tmp/EngineApp/Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine \
  python3 scripts/native-tests.py --binary target/debug/wega --output /tmp/nt \
  --suite smoke download engine webdriver capture
```

## 3. Performance (release build, `--test-threads=1`, nothing else compiling)

`cargo test --release --locked -p wega-engine --test iframe_bench -- --ignored --nocapture --test-threads=1`
(medians; tiny fixtures; one machine; the benchmark was taken before the creation rate limit,
which does not change per-frame cost).

| Measurement | Value |
|---|---|
| Page load, no frames | 85.9 ms |
| Page load, one iframe | 175.2 ms (+≈90 ms) |
| Page load, 8 sibling iframes | 746 ms (≈82 ms per frame) |
| Page load, 3 nested levels | 353 ms |
| JS realm (child View) creation | ≈ 75–80 ms each, dominated by the Deno/V8 bootstrap |
| Create 16 `srcdoc` frames | 1264 ms |
| Destroy 16 frames | 7.5 ms |
| RSS per frame | ≈ 1.98 MB (plus 5.6 MB JS heap reported per child realm) |
| Paint 1280×800, none / 1 / 8 / nested | 0.16 / 0.19 / 0.34 / 0.21 ms (trivial content) |
| `postMessage` parent→child→parent | 3.5 ms |
| Pointer move into a frame | 0.22 ms |
| Frame navigation (`src` change to load) | 58 ms |

Not measured: DevTools frame inspection latency (engine path only; no UI), closed-state
DevTools overhead delta (no change to the closed path: a frame costs nothing when no session
is live), CPU with many frames, comparison on a dedicated host.

Hard limits and their visible behaviour: depth > 8 or > 32 live frames → the iframe stays an
empty box and the parent gets a notice; cross-frame message > 1 MiB or > 256 queued →
exception in the sender; `srcdoc` > `html_bytes` → refused; realm creation is spread over
turns (2 per turn) so a flood cannot stall one turn for seconds. Layout keeps the existing
depth guard (121) per document; frames add no recursion in layout because each document lays
out itself.

## 4. Native end-to-end

Real `target/debug/wega --features test-harness` window with the bundled, ad-hoc-signed
sandboxed content worker (`--engine wega --capture`). Image:
`baseline/iframe-evidence/native-window-iframes.png`, metadata `native-capture.json`
(page: same-origin frame, cross-origin frame, nested frames, sandboxed `srcdoc`, log line).

Demonstrated natively: parent loads; same-origin frame renders; nested frame renders (two
levels); sandboxed `srcdoc` renders; `contentDocument` is `null`; cross-origin
`contentWindow.document` throws `SecurityError`; `postMessage` echo from the same-origin child
arrived with the host-computed origin; the window stays live.

**Not demonstrated / failing natively:**
* The cross-origin frame **did not load** in the production bound-broker configuration (it
  shows the engine's error page). Engine tests with the unbound broker (`WEGA_TEST_BROKER=1`)
  and without a broker load cross-origin frames correctly. The broker session is bound to
  the top-level document and enforces `request.origin == session origin` on non-top-level
  requests; frames of other origins need broker-side per-frame authority (a protocol/host
  change). The exact failing check was not isolated (worker diagnostics are not relayed in
  sandboxed mode and an in-process bound-session harness needs service identities this
  milestone did not wire). **Cross-origin frames therefore do not work in the shipped
  configuration.**
* Items 10–11, 13–20 of the native list (scrolling by injected input, pointer/keyboard
  routing, removal cleanup, DevTools parent/child selection, picker inside a frame, styles,
  authorised edits, revocation by navigation, stalled worker with frames, sandbox
  enforcement) were verified only through the engine-level tests in `iframes.rs` and
  `iframe_inspector.rs`, **not** in the native window.

## 5. Status by feature

**Fully implemented and tested (engine level):** parser/script-created frames; `src`, `srcdoc`,
`about:blank`, `name`, `sandbox`, `width`/`height`; independent documents and realms; sibling and
nested frames; navigation and removal with stale-response protection; origin inheritance and
opaque origins; sandbox scripts/forms/popups/modals; `frame-src`, `frame-ancestors`,
`X-Frame-Options`; host-routed `postMessage` with origin validation; compositing, clipping,
pointer/keyboard/wheel routing; limits; per-frame D3 DOM, geometry, pick and mutation
authority.

**Partially implemented:** `window.frames` (a view, not `window`), `WindowProxy` (a facade),
`loading=lazy` (attribute reflected; frames are created eagerly), `referrerpolicy` (reflected;
frame navigation uses the parent's policy), back/forward (no joint session history), z-order
(frames paint above the parent's content), Styles/Computed for frames (the D3 backend serves
them from the child port, but no style test was written for frames), picker/overlay geometry for
frames (the child picks in its own coordinates; mapping to the parent's overlay is not done),
resize events inside frames, `event.source` beyond one hop.

**Explicitly unsupported (fail closed):** same-origin synchronous DOM access (`contentDocument`,
`frameElement`), Permissions Policy, COOP/COEP/CORP, cookie partitioning, `data:`/`blob:`/
`javascript:` frames, `<object>/<embed>` frames, transfer lists across frames, per-site OS
processes, native DevTools frame picker (host protocol), cross-origin frames under a bound
broker session.

**Acceptance criteria not met:** same-origin DOM access works; cross-origin frames load in the
shipped configuration; DevTools can select frames in the native UI; native verification of
input, scrolling and inspection inside frames.

## 6. Remaining engine defects / risks

1. Bound-broker cross-origin frame loads fail (above). Highest priority.
2. Frames paint above all parent content (design L-1).
3. Realm creation ≈ 80 ms per frame (bootstrap not shared across realms).
4. `document.URL` of `srcdoc`/`about:blank` frames is the parent URL.
5. Wheel over a frame never chains to the parent when the frame cannot scroll.
6. The V8 host's unsafe surface grew from 2 to 6 sites, all in `adapters/v8/src/host.rs` with
   SAFETY notes (enter/exit balancing for several isolates per thread).
7. The frame table is rebuilt per mutation epoch with a document-wide `querySelectorAll`
   (O(n) per mutation burst).

## 7. Recommendation for D4

**CONDITIONAL GO.** The engine-side architecture (frame identity, principals, per-frame
targets, compositing seam) is stable and does not depend on Blitz or Stylo types beyond the
`PaintSource`/`DocumentView` contracts, so a Blitz/Stylo replacement can host it unchanged.
Before D4 depends on iframes: (1) give the broker per-frame authority and feed the host
`FrameTree` from the engine's frames; (2) add the native DevTools frame selector over
`View::frame_target`; (3) re-run the native list items listed above in the window.

## 8. Commits

```
d1d6ada docs(iframe): architecture audit and security design
1dedad0 feat(iframe): multi-isolate host safety, disable Blitz subdocument loader, frame CSP/XFO policy, compose seam
75cdb5b feat(iframe): browsing contexts as nested views with principals, sandbox, messaging, compositing and input routing
ee9398a test(iframe): loopback fixtures for frames, isolation, policy, limits, compositing and input
f9ba47f test(iframe): keyboard focus, wheel, sandbox, window relationships, navigation and stale references
b322ad2 feat(iframe): frame tree and per-frame D3 inspector targets with isolated stamps, handles and grants
34cd986 fix(iframe): stable isolate pointers for multi-realm hosts, replaceable window props, boundary allow-list, updated frame tests; docs
9c4f72d chore(iframe): strict clippy, remove unused role fields, add iframe cost benchmark
```
(the final documentation/evidence commit follows this report)
