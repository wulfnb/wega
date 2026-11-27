# Iframe WPT compatibility matrix

**Result: PASS (7/7 suites, 13 assertions passed).**

In Stage 7, the local WPT test runner was extended to host iframe fixtures with standards-oriented `testharness.js` assertions in `engines/wega/tests/iframe_wpt.rs`. In addition, the existing pinned DOM WPT test suite (35 files / 1,685 test cases) was verified with 0 failures after resolving the disconnected document clone ID lookup regression.

## 1. WPT Iframe Test Results

The suite executes standard WPT assertions under `engines/wega/tests/iframe_wpt.rs`:

| Test Suite | Assertions | Result | Details |
|---|---|---|---|
| `wpt_iframe_navigation` | 2 | PASS | `iframe.src` navigation commits updated principal and dispatches `load` event |
| `wpt_same_origin_policy` | 3 | PASS | Same-origin document access allowed; cross-origin throws `SecurityError`; `contentDocument` is null cross-origin |
| `wpt_window_relationships` | 1 | PASS | `window.parent` identity and `window.frameElement` access verified |
| `wpt_postmessage` | 2 | PASS | Structured message transfer and `targetOrigin` enforcement |
| `wpt_sandbox` | 2 | PASS | `allow-scripts` executes in opaque sandbox; script evaluation without flag is refused |
| `wpt_nested_browsing_contexts` | 1 | PASS | Nested frame heirarchy (parent -> child -> grandchild) propagation |
| `wpt_frame_loading_and_events` | 2 | PASS | Initial load event and dynamic insertion load event ordering |

**Total: 7/7 suites passed (13 assertions passed, 0 failures, 0 skips).**

## 2. Pinned DOM WPT Regression Suite

The 35 pinned DOM WPT test files in `engines/wega/tests/wpt.rs` were executed:
- All 1,685 assertions passed.
- Regression fixed in `Event-dispatch-bubbles-false.html`: document clones created via `cloneNode(true)`, `new Document()`, and `DOMImplementation.createHTMLDocument()` previously had ID lookups defaulting to the main document root. `getById` now respects the target document scope, ensuring capturing and bubbling event dispatch traverses cloned documents correctly.

## 3. Upstream Coverage & Roadmapped Capabilities

| Area | Upstream Suite | Current Engine Status | Roadmap |
|---|---|---|---|
| HTML iframe attributes | `html/semantics/embedded-content/the-iframe-element` | Supported | Full attribute reflection |
| Same-origin policy | `html/browsers/origin`, `html/cross-origin-*` | Supported | Host-backed facades enforce origin checks at access time |
| WindowProxy | `html/browsers/the-window-object` | Partial | Safe Proxy facade over host-backed browsing context |
| postMessage | `webmessaging` | Supported | Host-routed, target-origin enforced |
| Sandbox | `html/browsers/sandboxing` | Supported | Scripts, same-origin, forms, popups, modals |
| Joint history | `html/browsers/history` | Unsupported | Frame navigations currently do not create joint session history entries |
| Process isolation | `html/browsers/browsing-the-web` | Logical | All frames in a tab share one sandboxed worker |
