# D3-A: read-only Elements inspector — implementation report

Completed locally on 2026-11-15, starting at `3f1ccb4`. **Nothing pushed. D3-B/C
have not been implemented.** Design: [devtools-d3-a-design.md](devtools-d3-a-design.md).
Machine-readable results, all 83 workspace targets, raw measurement values and
native output: [devtools-d3-a-evidence.json](baseline/devtools-d3-a-evidence.json).

## Local commits

| Commit | Stage |
|---|---|
| `2c71136` | Prerequisites and design, committed before implementation |
| `9bc0c35` | Protocol, borrowed own-text contract, bounded engine reader and process/security tests |
| `f18d0c0` | Browser plumbing, Elements panel, session lifecycle and native/detached-window test |
| `678a002` | Reader and mutation benchmarks, deadline check, lifecycle/stress validation |
| `ed8e688` | Bounded page windows, late-sibling access and selection retention |
| Report commit | This report, evidence and roadmap update |

## Prerequisites

Built the locked `wega-engine` and `wega-content-bundle`, bundled and signed
`/tmp/WegaD3A.app`, and ran the existing required-worker inspector test **before
implementation**: 1 passed. It covered D3.0 origin/principal refusals, grants,
console capture, evaluation and revocation. No D3.0 regression was found.

Reproduced WebDriver's original line-76 assertion as `AssertionError: New tab`.
The default unbundled desktop discovered an unsigned adjacent worker, required
launch refused it, and the HTTP fixture received no request. With the freshly
built development worker the entire suite passed (19.41 s); with the signed
helper and no development opt-in it passed (14.99 s). This was a **test-launch
configuration problem**, not a title/DOM browser regression. The native launcher
now honors `WEGA_CONTENT_HELPER`, matching the sandbox suites. The fixed wrapper's
whole HTTP suite passed with the signed helper in **17.33 s**, including title,
script title, Cache API, native-page transitions, history, screenshots and the
D3.0 Origin/Host/Content-Type refusal checks. Product launch/security policy was
not relaxed.

## Delivered architecture and behavior

- Open F12, then **Elements**. Expand/collapse rows, select a node, read qualified
  attribute previews and own text/comment previews, and page children. Namespace,
  prefix, node type and child count accompany the name. Truncation, cached values,
  depth limits and errors are visible. No DOM/CSS editing, highlighting, matched
  rules, selector evaluation, injected inspector script or new evaluation grant.
- The backend reads the existing `View.document` through `DocumentView`,
  `NodeView` and `MutationClock`. The only added DOM primitive is borrowed
  `NodeView::text()`: own text/comment data, without descendant concatenation.
  The backend retains **no DOM nodes, mirror, handle table or snapshot tree**.
- Audited the actual slotmap implementation and adapter conversions. Wega's
  `NodeKey` preserves both slot index and generation; dropped keys do not alias
  reused slots. Wire IDs pair that generation-bearing u64 with both document
  generation and replacement revision. A second generation mapping is unnecessary
  for the current adapter. Mutation/reparenting retains IDs; document replacement
  invalidates them. Slot-reuse, detach and reattach behaviors are tested.
- Engine protocol **24**, inspector **3**, `InspectorDom` capability. Closed
  commands: `GetDomRoot`, `GetDomChildren`, `DescribeDomNode`. Description validates
  selection; selection itself is browser UI state, without page side effects.
  No adapter/layout/style type appears in the public protocol.
- D3.0's **mutation epoch remains separate from document identity**. Child cursors
  carry an epoch and fail with `StaleMutation` after mutation. Status polling at
  the existing 1 Hz samples changes; only visible expanded cached pages refresh,
  one DOM request at a time. Surviving selection is revalidated. Mutation storms
  add no observer, console hook, eager subtree walk or full DOM rebuild.
- Child pages replace the previous page for that parent. Old page values are
  pruned from the bounded presentation cache while retaining selection. **Next
  children / First children** allow reaching arbitrarily late siblings rather
  than filling the cache after a few pages. A test pages through 1,000 siblings
  while retaining at most three rows and an off-page selection.
- Existing browser-owned sessions, identity envelopes and `DocumentRequests`
  authorize requests and reject wrong tab/frame/process/session or document.
  Resolution also checks generation and bounded ancestry to the current light-DOM
  root. Detached nodes, template contents, shadow roots and child documents are
  not inspector targets. Iframe elements expose only their own DOM metadata.
- Docked tools follow the selected tab; detached tools stay pinned. Navigation,
  process/session replacement and closure clear document-bound values/pending
  reads. Render-time validation checks target, document and live session again.
  Timeout recovery is bounded; an unavailable target cannot keep an unbounded
  queue or display another target's cached values.

## Budgets and responsiveness

| Resource | Enforced limit |
|---|---|
| Nodes returned / child slots examined per page | 64 |
| Ancestry / expansion | 128 edges from document root |
| Attributes per node | 32 |
| Each name, namespace, prefix, value or text preview | 256 UTF-8 bytes, character-boundary truncation |
| Serialized DOM page | 64 KiB, accounting for JSON escaping; consumer validates again |
| DOM request work | 10 ms cooperative deadline, checked through traversal and after validation |
| Transport deadline | Existing 2 s; missing UI completion expires after 2.1 s |
| Pending | One DOM request in the UI; existing four-request inspector cap |
| UI metadata | 512 rows / 512 KiB accounted serialized values |
| Cached edges | 512; bounded iterative drawing/pruning and viewport row virtualization |

Ancestry checks, text previews, attribute enumeration and child-slot iteration are
bounded independently. Direct `child_at` indexing avoids scanning preceding
siblings. No reader calls `LayoutControl`, layout/paint flush, subtree text,
selector matching or arbitrary JavaScript. Depth/size exhaustion is an explicit
error. One node whose bounded metadata still exceeds a page can return
`BudgetExceeded` rather than allocate an oversized response.

The work deadline is cooperative, not OS thread preemption: scheduling pauses can
cause a request to return `Timeout`. The stress test initially assumed success
under concurrent compiler load and hit that legitimate refusal. Its corrected
fixture retains 1,000 siblings and escaped-attribute byte pressure, retries only
`Timeout` at most five times, and still rejects every other error. Production
budgets were not increased. UI metadata bytes are not a total RSS limit; fixed row,
edge and queue bounds also bound structural allocation overhead.

## Validation evidence

All counts below are executed results. Repeated configurations and opt-in tests
overlap; **do not add these rows as unique tests**.

| Gate | Result |
|---|---|
| Engines workspace, every lib/bin/integration target, locked, serial test execution | **83 targets; 674 passed, 0 failed, 18 ignored** |
| Within that matrix: protocol / engine lib / inspector process / console process / DOM inspector | **84 / 68 / 15 / 21 / 7 passed** |
| Within matrix: adapter / DOM boundary | **65 passed + 2 ignored / 2 passed** |
| Root harness library / final browser binary | **113 / 93 passed** |
| Root no-default library | **109 passed** |
| Architecture/dependency tools / WebIDL generator tests | **8 + 6 / 6 passed** |
| Broker mode: documents, forms, history, scripts, security | **5 + 4 + 3 + 8 + 9 = 29 passed** |
| Required sandbox worker: inspector with DOM + console/evaluation | **1 passed explicitly with `--ignored`** |
| Required sandbox worker: JIT/storage/cookies/shared frames through grants | **1 passed explicitly with `--ignored`** |
| Native App Sandbox: ambient-authority denial and explicit file grant | **1 passed explicitly with signed containment probe** |
| Native browser/Elements/detached OS window | **1 whole end-to-end scenario passed** |
| WebDriver HTTP | **1 whole suite passed with signed helper** |
| DOM WPT | **1 Rust test; 1,685/1,685 upstream assertions passed** |
| CSS-WPT | **2 Rust tests; 7/7 assertions and pixel reftest passed (0 differing pixels)** |
| Reader and native-mutation benchmarks | **2 opt-in functions executed; release and debug measurements below** |
| Formatting / diff whitespace | Root, engines, tools all clean |
| Strict locked clippy, `-D warnings` | Engines workspace all targets; root default, no-default and harness; tools; WebIDL all pass |
| Locked metadata / dependencies | Root, engines, tools resolve; all three lockfiles unchanged |
| Architecture | **13 members, 24 internal edges, 0 violations, 11 informational; no cycles** |
| Boundaries / vendor integrity / generator freshness | Pass; **13 pinned vendor archives** match documented patches; generated families current |
| Unsafe inventory since `3f1ccb4` | **0 new unsafe code lines** |

The 18 default ignores are opt-in benchmarks and native-host fixtures. The three
required native fixtures above and the console/reader/mutation benchmarks were
run explicitly; historical general engine/render/IPC microbenchmarks remain
ignored. The initial aggregate all-target build exhausted disk during parallel
linking. The complete matrix was then run one target at a time, deleting only
completed regenerable test executables. Target-by-target final counts are in the
JSON evidence. No build failures remain.

A baseline comparison initially reused a Cargo target directory across checkouts
and produced stale-artifact compile errors. Those artifacts were cleared, the
baseline/current builds isolated, and affected targets rerun successfully. Outer
tool-sandbox loopback permission failures were rerun with host fixture networking
allowed; **the content App Sandbox stayed enabled**.

The final native scenario used a freshly signed helper. It expanded the DOM,
selected `#counter`, rendered at least two detached-window frames, remained pinned
across a real browser tab switch, navigated and cleared selection, revoked and
reattached sessions, then stopped/resumed the real worker. Stall notification:
**2,466 ms / 408 UI polls; recovery 80 ms after SIGCONT**. The browser remained
responsive. Security-negative tests also cover wrong tab/frame/process, replaced
documents, revoked sessions, stale/reused keys, disconnected nodes, old cursors,
foreign/oversized responses and page accessors that throw if executed.

## Performance

macOS 27.2.1 arm64, Rust 1.98.0. Concurrent builds were present; observed load
averages were about 5.9–9.0. Reported latencies are **backend reads**, including
validation and response serialization in the benchmark, not browser-to-worker
round-trip times. Each row uses 1,000 requests, never lays out the fixture, and
drops each response. Small: 16 text-bearing siblings. Large: 15,000 text-bearing
siblings (about 30,000 nodes), requesting the final 64 children. Deep: 1,000
nested elements; description probes beyond the supported ancestry limit.

### Optimized reader latency

| Tree / operation | Median µs | p95 µs | Max µs | Response bytes |
|---|---:|---:|---:|---:|
| Small root | 1.542 | 1.625 | 60.625 | 289 |
| Small 16-child page | 24.416 | 25.167 | 61.750 | 4,141 |
| Small description | 1.959 | 2.042 | 9.917 | 358 |
| Large root | 1.500 | 1.542 | 10.666 | 289 |
| Large final 64-child page | **85.292** | **89.917** | 188.875 | 16,097 |
| Large description | 1.958 | 2.042 | 5.917 | 361 |
| Deep root | 1.500 | 1.625 | 7.375 | 289 |
| Deep child page | 2.208 | 2.292 | 4.625 | 364 |
| Deep over-limit description | **5.000** | 5.125 | 7.500 | Refused, 1,000/1,000 |

Release RSS deltas after constructing each document: small **8.266 MiB**, large
**13.953 MiB**, deep **0.188 MiB**. Additional retained RSS across 3,000 reads of
each fixture: **0.266 / 0.062 / 0.000 MiB**, respectively. These are process RSS
deltas with allocator reuse; small includes font/startup costs. They are **not**
total document allocation sizes. The reader itself retains no tree or handles.

The initial debug run's large child page measured 1.911 ms median / 1.968 ms p95,
and deep refusal 16.916 µs median. Full raw values, including RSS, are preserved
in the evidence file. The final budget-check debug rerun also passed; compiler
load shifted its timings, reinforcing the need to state build profile and load.

### D3.0 preservation: paired before/after

Baseline is exact `3f1ccb4`; identical benchmarks, compiler and profiles. Three
interleaved release runs per binary, reversing order on the middle pair. Values
below are medians of the three run medians; individual values are in the JSON.

| Operation | D3.0 µs | D3-A µs |
|---|---:|---:|
| console log(string, number) | 11.27 | 10.95 |
| console log(string, nested object) | 32.00 | 32.27 |
| console warn(string, number) | 15.95 | 15.91 |
| console log(8 numbers) | 16.23 | 16.06 |
| console log(100-property object) | 118.53 | 117.88 |
| native attribute mutation + epoch | 0.0379 | 0.0370 |
| native text mutation + epoch | 0.0255 | 0.0252 |

Console flood RSS growth, 100,000 calls: baseline **10.9 / 10.8 / 11.2 MiB**;
after **10.8 / 11.1 / 10.9 MiB**. Ten interleaved debug mutation pairs also passed:
attribute median **0.99965 → 1.00465 µs**, text **0.42025 → 0.42595 µs**, within
their observed spread. No measured regression. D3.0 console ingestion and shared
mutation reservation code are unchanged; no always-on DOM instrumentation was
added. These measurements do not establish an idle browser CPU or IPC-latency claim.

## Reproduction

```sh
cargo build --locked --manifest-path engines/Cargo.toml -p wega-engine --bin wega-engine \
  -p wega-platform --bin wega-content-bundle
engines/target/debug/wega-content-bundle engines/target/debug/wega-engine /tmp/WegaD3A.app
export WEGA_CONTENT_HELPER=/tmp/WegaD3A.app/Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine
cargo test --locked --manifest-path engines/Cargo.toml -p wega-engine --test inspector_sandbox -- --ignored --nocapture
cargo build --locked --features test-harness --bin wega
sh scripts/inspector-native-test.sh
python3 scripts/native-tests.py --suite webdriver --output /tmp/wega-d3a-webdriver
cargo test --release --locked --manifest-path engines/Cargo.toml -p wega-engine --test inspector_dom_bench -- --ignored --nocapture
cargo test --release --locked --manifest-path engines/Cargo.toml -p wega-engine --test mutation_epoch_bench -- --ignored --nocapture
cargo test --locked --manifest-path engines/Cargo.toml -p wega-engine --test wpt --test css_wpt -- --nocapture --test-threads=1
```

For a machine with enough build space, the equivalent aggregate engine gate is
`cargo test --locked --manifest-path engines/Cargo.toml --workspace --all-targets
-- --test-threads=1`. This run used per-target invocations of that matrix to bound
disk use, with final results for all targets in the evidence artifact.

## Limitations and D3-B/C handoff

No unresolved D3-A test failures. Linux/Windows, visual screenshot review, full
browser IPC latency/RSS, and a real native-window crash/replacement sequence were
not measured here. Process replacement is tested through real worker authority
and the UI lifecycle state machine; native window coverage includes navigation,
pinning, detach/reattach and process stalls.

Only connected light DOM is exposed. No doctype data exists in the current DOM
contract, and shadow/template/child-document inspection is deferred. Previews and
pagination are bounded; a deeply expanded/wide collection of branches can still
reach the overall cache cap and require reset. A parent page window remains
navigable independently of its total sibling count.

**D3-B can safely start with a separate read-only styles design and adapter work.**
It can rely on stamped generation-safe selection, epoch-aware cursors and the
bounded session/UI path. It must test the proposed CSSOM mapping and matched-rule
access at runtime; no matched-rule implementation was added here. In-sheet
`insertRule`/`deleteRule` still does not bump the D3.0 epoch: close that gap before
CSS editing or claiming complete style invalidation.

**D3-C layout remains blocked by the pre-existing depth-122 first-flush hang.**
The 1,000-level benchmark proves read-only inspection does not trigger layout;
it does not fix general rendering of such documents. Geometry must remain cached
until that hang is resolved. The existing local-process WebDriver trust model and
its unauthenticated endpoint are unchanged. No D3-B/C work was begun.
