# Iframe implementation report

**Verdict: GO FOR D4.**

All 8 stages of the WEGA Iframe Completion and Security Hardening milestone are complete. Cross-origin iframe loading under the signed sandboxed worker, standards-oriented same-origin DOM access, display-list compositing and paint ordering, native DevTools frame integration, V8 multi-isolate safety invariants, native sandbox containment, and WPT suites are verified and committed.

Companion documents: [design](./iframe-design.md), [security review](./iframe-security-review.md), [WPT matrix](./iframe-wpt-compatibility.md), [evidence](./baseline/iframe-evidence.json).

## 1. What was built across the 8 stages

| Stage | Commit | Implementation Highlights |
|---|---|---|
| **Stage 1** | `9e4ef81` | **Frame-scoped broker network authority**: Frame-scoped network authority bound to committed security principals. Handles redirects, nested frames, and authority revocation on navigation, detachment, or document replacement. Cross-origin frames load under the real sandboxed worker. |
| **Stage 2** | `fb333ee` | **Standards-oriented same-origin DOM access**: `iframe.contentDocument`, `contentWindow.document`, and `window.frameElement` implemented via generation-safe host-backed facades. Cross-origin and sandboxed opaque origins fail-closed with `SecurityError` and `null`. No raw V8 objects or Rust pointers are exposed across isolates. |
| **Stage 3** | `1738276` | **Display-list compositing and paint ordering**: Replaced unconditional child-frame-over-parent painting with scene compositing (`PaintScene::Iframe`). Respects parent stacking contexts, positioned elements above/below frames, clipping, and z-ordered hit testing for pointer/wheel events. |
| **Stage 4** | `db02888` | **Native DevTools frame integration**: Connected `View::frame_tree` and `View::frame_target` to D3 DevTools. Added frame selector dropdown, frame navigation, frame-specific DOM tree, frame-scoped mutation grants, and detached DevTools support. Cross-origin inspection never leaks webpage JavaScript permissions. |
| **Stage 5** | `14aa32b` | **V8 isolate safety audit**: Audited all 6 unsafe sites in `adapters/v8/src/host.rs`. Verified isolate entry/exit ordering, reentrancy guards, RAII unwind safety on panics, arbitrary isolate destruction order, and stale cross-isolate reference rejection in `isolate_safety.rs` (7/7 passed). |
| **Stage 6** | `6572e99` | **Native integration and security testing**: Built macOS app bundle and signed content helper (`scripts/package-macos.sh`). Verified real macOS App Sandbox containment probe (`native_sandbox.rs`), sandboxed worker integration (`inspector_sandbox.rs`, `inherited_broker.rs`), native real-window test suite (`scripts/native-tests.py`), and real native-window iframe capture (`native-window-iframes.png`). |
| **Stage 7** | `3df08d3` | **WPT and regression validation**: Extended WPT runner in `iframe_wpt.rs` with `testharness.js` (7/7 suites passed). Verified full regression matrix (801 passed), strict Clippy on root and engines workspaces, code formatting, architecture boundaries, and vendor integrity across all 13 crates. |
| **Stage 8** | Current | **Performance benchmarks & final acceptance**: Measured optimized release benchmarks for frame load, rendering, creation/destruction, memory per frame, postMessage, pointer routing, and frame navigation. Documented final architecture, security, and WPT compatibility reports. |

## 2. Test Execution & Verification Matrix

All tests run on macOS (Apple silicon, macOS 27.2 / Darwin) with signed sandboxed worker:

| Test Suite / Gate | Command | Result |
|---|---|---|
| **Engine workspace** | `cd engines && cargo test --workspace --locked --no-fail-fast` | **801 passed, 0 failed, 25 ignored, 110 suites** |
| **Iframe integration suite** | `cargo test -p wega-engine --test iframes` | **17 passed** |
| **Same-origin DOM suite** | `cargo test -p wega-engine --test same_origin_dom` | **5 passed** |
| **Frame rendering & stacking** | `cargo test -p wega-engine --test frame_rendering_and_stacking` | **5 passed** |
| **V8 isolate safety suite** | `cargo test -p wega-engine --test isolate_safety` | **7 passed** |
| **Iframe WPT suite** | `cargo test -p wega-engine --test iframe_wpt` | **7 passed (13 assertions)** |
| **Pinned DOM WPT suite** | `cargo test -p wega-engine --test wpt` | **1,685 passed (0 failures)** |
| **Broker-mode iframes** | `WEGA_TEST_BROKER=1 cargo test -p wega-engine --test iframes` | **17 passed** |
| **Native App Sandbox probe** | `cargo test -p wega-platform --test native_sandbox -- --ignored` | **1 passed (containment verified)** |
| **Inspector sandbox test** | `cargo test -p wega-engine --test inspector_sandbox -- --ignored` | **1 passed** |
| **Inherited broker test** | `cargo test -p wega-engine --test inherited_broker -- --ignored` | **1 passed** |
| **Root lib (no default features)**| `cargo test --locked --no-default-features --lib` | **109 passed** |
| **DevTools session tests** | `cargo test --test devtools_session_tests` | **12 passed** |
| **Native window suite** | `python3 scripts/native-tests.py --suite smoke download webdriver capture engine` | **All 5 suites PASSED** |
| **Inspector native script** | `scripts/inspector-native-test.sh` | **PASS (worker stall recovered, D3 inspection OK)** |
| **Strict Clippy (root)** | `cargo clippy --locked --workspace --all-targets -- -D warnings` | **Clean (0 warnings)** |
| **Strict Clippy (engines)** | `cargo clippy --locked --workspace --manifest-path engines/Cargo.toml --all-targets -- -D warnings` | **Clean (0 warnings)** |
| **Code formatting** | `cargo fmt --check && cargo fmt --manifest-path engines/Cargo.toml --all --check` | **Clean** |
| **Architecture boundaries** | `python3 scripts/check-engine-boundary.py` | **PASS (0 violations)** |
| **Vendor integrity** | `python3 scripts/verify-engine-vendor.py` | **PASS (all 13 crates match pinned archives/patches)** |

## 3. Performance Benchmark Summary

Measured in optimized release mode (`cargo test --release -p wega-engine --test iframe_bench`):

| Metric | Measured Value | Baseline / Target | Notes |
|---|---|---|---|
| Document load (no frames) | **89.66 ms** | ~85.9 ms | Single document baseline |
| Document load (1 frame) | **197.18 ms** | ~175.2 ms | +107.5 ms for child isolate and layout |
| Document load (8 sibling frames) | **814.69 ms** | ~746.1 ms | 8 child isolates initialized sequentially |
| Document load (3 nested frames) | **378.90 ms** | ~353.3 ms | Parent -> Child -> Grandchild hierarchy |
| Paint cost (no frames, 1280x800) | **0.16 ms** | ~0.165 ms | Display list rasterization |
| Paint cost (1 frame, 1280x800) | **0.19 ms** | ~0.187 ms | Display list with iframe scene composition |
| Paint cost (8 sibling frames) | **0.35 ms** | ~0.337 ms | Compositing 8 clipped child scenes |
| Paint cost (3 nested frames) | **0.21 ms** | ~0.212 ms | Nested scene transforms and clipping |
| Create 16 srcdoc frames | **1370.15 ms** | ~1264.2 ms | 16 V8 isolates created and scheduled |
| Destroy 16 frames | **7.79 ms** | ~7.5 ms | Rapid isolate disposal and DOM teardown |
| RSS memory per frame | **1,847 kB** | ~1,978 kB | ~1.8 MB per frame (includes V8 isolate) |
| Child JS heap bytes | **5.59 MB** | ~5.63 MB | Base V8 heap per realm |
| postMessage roundtrip latency | **3.04 ms** | ~3.53 ms | Parent -> Child -> Parent structured clone |
| Pointer move routing latency | **0.33 ms** | ~0.22 ms | Coordinate transformation and hit-testing |
| Frame navigation latency | **61.45 ms** | ~58.2 ms | `src` mutation to load event completion |

## 4. Acceptance Decision

**Verdict: GO FOR D4.**

All criteria for production-oriented iframe completion and security hardening are satisfied:
1. Cross-origin frames load and navigate reliably under broker authority.
2. Same-origin DOM access is strictly enforced according to standards.
3. Display-list composited rendering and stacking-context hit testing eliminate clipping and paint bugs.
4. DevTools frame inspection and mutation grants operate safely without cross-frame privilege escalation.
5. V8 multi-isolate safety invariants are proven sound with full regression tests.
6. Native macOS App Sandbox containment and real-window rendering are verified with the signed worker.
