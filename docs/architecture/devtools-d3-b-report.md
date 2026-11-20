# D3-B: read-only CSS and style inspection — implementation report

Implemented locally on 2026-11-15 from `ac6928e`. Nothing pushed. No D3-C work.
The Elements panel now has bounded Inline, Computed, Stylesheets, Rules and
Declarations views. **Matched rules are explicitly unsupported**, following the
runtime feasibility stop required by the task. Computed inspection reads 23
cached scalar longhands without requesting layout or style resolution.

Design: [`devtools-d3-b-design.md`](./devtools-d3-b-design.md).
Machine-readable results and individual benchmark samples:
[`baseline/devtools-d3-b-evidence.json`](./baseline/devtools-d3-b-evidence.json).
Counts below overlap across configurations; do not sum them as unique tests.

## Local commits

| Commit | Stage |
|---|---|
| `9b08a19` | Executed feasibility spike and design, committed before production changes |
| `bee2cfb` | Wega contracts, bounded adapter, protocol, backend and narrow vendor fixes |
| `dc32589` | Browser plumbing and Styles/Computed UI, including native harness coverage |
| `fdd3941` | Escaped-byte pagination, deep-node coverage and optimized style benchmarks |
| `a2580a4` | Preserve the single outstanding read during selection and epoch churn |

The final documentation commit adds this report, evidence and roadmap status.

## Runtime feasibility and vendor decisions

The three executable spike tests proved author stylesheet enumeration, nested
rule access and cached computed reads. Fixture-owned rendering produced width
`42px !important` and inherited `rgb(1, 2, 3)`; inspection itself never renders.
Before first rendering the same node has no cached computed style.

Actual Stylo `StyleSource` retains an immutable declaration block, not the rule
object claimed by an older feasibility note. The spike deliberately shares one
block between matching `.a` and nonmatching `.never`. Retained block identity
maps to both rules and cannot identify the matching selector. Selector-list
identity, specificity and winning/overridden metadata are also absent.
**Matched extraction stopped here.** No heuristic reverse lookup, selector
re-matching, fabricated cascade metadata or broad vendor patch was introduced.
The UI/API returns Unsupported with an explanation rather than an empty match
list. Wega-owned `MatchedSelector` is a future contract shape, never populated.

The old CSSOM group serializer emitted **144,016 bytes in 1,953 µs (debug)** for
2,000 children. Direct borrowed public rule lists support shallow pagination, so
no new Blitz inspection accessor was needed. Two measured, narrow patches were
necessary:

- `bounded-declaration-writer.patch`: an additive generic writer entry point in
  Stylo's generated property serializer (six added code lines). The original
  `String` entry point delegates to it. It permits stopping a million-character
  value at 512 bytes without first allocating its complete serialization.
- `cssom-inspection-generation.patch`: seven generation increments in successful
  existing CSSOM invalidation/disabled-state paths. This closes D3.0's documented
  in-sheet epoch gap so old rule paths cannot alias after insertion/deletion or
  declaration/selector changes. A failed deletion leaves generation unchanged.

Both exact patches are registered in `engines/vendor/sources.json` and verified
against pinned archives. No new dependencies, manifests or lockfile changes.

## Authoritative contracts, protocol and lifecycle

`wega_dom::inspect::StyleInspect` borrows the existing authoritative DOM/style
state. The Blitz adapter maps that state to Wega-owned sheet/rule/declaration,
availability and source-location values. It retains no alternate CSS engine,
DOM tree, stylesheet cache or declaration-block copy. Public protocol messages
contain no Blitz/Stylo types. Engine protocol **25**, inspector **4**, with
`InspectorStyles` capability gating.

Every rule reference contains tab/frame/process, inspector session, document
identity/replacement revision, mutation epoch, stylesheet generation,
generation-bearing owner node identity and a bounded index path. Execution checks
all dimensions after the existing browser-owned inspector authorization gate.
Node reads use existing document-scoped generation-bearing keys and reject
removed/anonymous/disconnected nodes. Browser preflight and paint-time authority
checks apply in docked and detached tools. No inspection command executes page
JavaScript or gains evaluation authority.

In-sheet CSSOM changes now feed the D3.0 mutation epoch through the adapter's
existing stylesheet-generation observation. Document replacement remains a
separate identity event. Old references fail as stale; they are never silently
rebound to a new rule. Navigation, session revocation, tab changes and replacement
processes clear retained style data. Mutation changes discard rule references and
return the rules view to sheet enumeration, without rebuilding the DOM tree.

The UI retains one response page, one focus reference and one pending style read.
Selection/epoch churn clears values but preserves a same-authority outstanding
request until its result is discarded or completes; it cannot enqueue a flood.
Computed refresh is paced to one second while displayed, with explicit refresh.
D3-A's independently bounded DOM page cache and selection implementation remain
in use. Styles render at most one bounded page of native egui text beside the
expandable Elements tree. Nothing is persisted into the profile.

## Supported scope and enforced limits

| Area | Behavior |
|---|---|
| Sheets | Author sheets, owner-key order, disabled flag, source URL and top-level count; at most 512 owners scanned/listed |
| Rules | Shallow pages of 32; child pages for Style, Media, Supports and Layer blocks; bounded selector/media text |
| Declarations | Canonical longhands/custom declarations, value preview and `!important`; inline and style-rule blocks |
| Source | Style-rule parser start line/column and stylesheet URL; stylesheet-relative, no invented end ranges |
| Computed | Cached primary styles for the 23 scalar longhands below; Available/Cached/NotComputed/Unsupported distinguished |
| References | At most 16 path components; connected-node walk at most 128 ancestor edges |
| Text/bytes | At most 512 UTF-8 bytes/string, 64 KiB serialized style page, escaped JSON bytes counted before paging |
| Time | 10 ms cooperative backend deadline, existing 2 s inspector transport deadline; no layout/matching request |
| UI | One style page, one focus reference and one pending style query; existing four-request session cap remains |

Computed properties: color, background-color, display, position, width, height,
min-width, min-height, max-width, max-height, margin-top/right/bottom/left,
padding-top/right/bottom/left, font-size, font-weight, line-height, opacity and
box-sizing. The serializer borrows scalar values rather than cloning arbitrary
list/custom-property computed state. Computed declarations do not claim
`!important` provenance.

Cached values can precede pending mutations, viewport updates or animation work.
Responses expose the existing `layout_pending` flag and always label computed
values cached; this is not a promise of live used values. Normal browser rendering
refreshes the cache. The inspector never calls resolve, layout, geometry,
`getComputedStyle` JavaScript, selector matching or an undisplayed-style resolver.
Budgets are cooperative at traversal/write boundaries, not hard preemption of a
vendor function or a scheduling guarantee on a busy host.

Unsupported and labelled: matched selectors/rules and inherited-rule chains;
specificity, winners/overridden state, cascade layer/source-order ranking;
UA/user/imported child sheets; non-scalar computed longhands, computed custom,
pseudo/visited styles and used-value resolution. Font-face descriptors,
keyframes and other unimplemented rule kinds show explicit unsupported labels.
Selector text is the rule's selector list, never a claim that it matched the
selected element. Shorthand/source formatting and complete source ranges are
not reconstructed.

## Executed validation

| Gate | Final result |
|---|---|
| Engines workspace, every lib/bin/integration target, locked, serial | **87 targets; 690 passed, 0 failed, 19 ignored** |
| Within matrix: protocol / engine unit / inspector / console / DOM / styles | **87 / 68 / 15 / 21 / 7 / 5 passed** |
| Runtime spike / new adapter style tests | **3 / 5 passed** |
| Root harness library / final browser binary | **113 / 98 passed** (56 DevTools tests in the binary) |
| Root no-default library | **109 passed** |
| Architecture/dependency tools / WebIDL tests | **8 + 6 / 6 passed** |
| Broker mode: documents, forms, history, scripts, security | **5 + 4 + 3 + 8 + 9 = 29 passed** |
| Signed required worker: inspector DOM/console/CSS/evaluation | **1 passed explicitly with `--ignored`** |
| Signed required worker: JIT/storage/cookies/shared-frame grants | **1 passed explicitly with `--ignored`** |
| Signed native App Sandbox denial/grant probe | **1 passed explicitly with `--ignored`** |
| Native browser / detached window | **1 full end-to-end scenario passed** |
| DOM WPT | **1 Rust test, 1,685/1,685 upstream assertions passed** |
| CSS-WPT | **2 Rust tests, 7/7 assertions and pixel reftest passed** |
| Formatting / whitespace | Root, engines, architecture tools and WebIDL checks pass; `git diff --check` clean |
| Locked clippy, `-D warnings` | Engines workspace/all targets; root harness/default/no-default; architecture tools and WebIDL pass |
| Locked metadata | Root, engines and tools resolve; no manifest or lockfile diff |
| Architecture / dependency DAG | **13 members, 24 edges, 0 violations, 11 informational, no cycles** |
| Boundary / vendor / generated WebIDL freshness | Pass; **13 pinned archives** match exact documented patches; generated families current |
| Unsafe diff from `ac6928e` | **0 added unsafe code lines** |

The matrix runs each target separately with `--test-threads=1`, removing only its
completed regenerable test executable to bound disk use. Default ignores are
opt-in benchmarks/native fixtures, not silently skipped failures. Explicit native
fixtures and five benchmark functions (style, DOM, render, console, mutation)
were executed separately; historical general IPC/engine benchmarks stay opt-in.

Coverage includes inheritance, specificity, important versus inline declarations,
media rules, cached-versus-refreshed values, CSSOM insert/delete/property/selector/
cssText/remove/disabled changes, failed mutation stability, source metadata,
1,000-descendant selectors, 2,000-rule lists/groups, a million-character value,
20,000-rule benchmark sheets and a 1,000-level unrendered DOM. Reading a node at
140 ancestor levels is refused without layout; removed nodes are stale. A
40-declaration escaped-name test forces byte-budget pagination and verifies
complete continuation without lost entries.

Negative tests exercise wrong tab/frame/process, old replacement process,
forged/revoked sessions, stale documents, mutation epochs and stylesheet
references, invalid paths/offsets, foreign/oversized response data and selection
churn. The signed sandbox CSS test enumerates sheets/rules/declarations without
an evaluation grant and rejects a forged rule session.

The final native scenario reads cached color and sheets/rules/declarations in a
real detached OS window, expands/selects the DOM, pins across a real tab switch,
navigates and clears selection/authority, detaches/reattaches and stops/resumes
the actual worker. Stall detection **2,460 ms / 406 UI polls**; recovery **110 ms**
after SIGCONT. The browser continues polling during the stall. App Sandbox
ambient-authority denials and explicit grants also pass with a freshly bundled
signed containment probe.

Reproduce core gates (use the freshly bundled helper for opt-in native fixtures):

```sh
cargo test --locked --manifest-path engines/Cargo.toml -p wega-adapter-blitz --test css_inspection_spike --test style_inspect
cargo test --locked --manifest-path engines/Cargo.toml -p wega-engine --test inspector_styles -- --test-threads=1
cargo test --locked --features test-harness --lib --bin wega
cargo clippy --locked --manifest-path engines/Cargo.toml --workspace --all-targets -- -D warnings
cargo fmt --manifest-path engines/Cargo.toml --all --check
python3 scripts/check-engine-boundary.py
python3 scripts/verify-engine-vendor.py
```

## Latency and retained memory

macOS 27.2.1 arm64, rustc 1.98.0; optimized engine built at `fdd3941`. Each style
operation ran 1,000 times. Timing includes backend mapping, bounds validation and
JSON encoding; it excludes IPC/UI. Small/large fixtures render before measurement;
huge/deep fixtures deliberately do not render. Deep selects the outer element;
the separate depth-refusal test covers an actually deep selected node.

| Fixture / read | Median µs | p95 µs | Maximum µs | Response bytes | Incremental RSS MiB |
|---|---:|---:|---:|---:|---:|
| Small / sheets | 5.083 | 5.292 | 72.000 | 749 | .047 |
| Small / rules | 14.042 | 14.334 | 808.292 | 1,903 | .109 |
| Small / declarations | 4.917 | 5.042 | 36.167 | 736 | .031 |
| Small / computed | 18.833 | 21.500 | 381.708 | 2,560 | .219 |
| 20,000 rules / last 32 rules | 108.625 | 129.542 | 188.166 | 14,450 | .078 |
| 20,000 rules / computed | 21.583 | 22.084 | 45.458 | 2,558 | .000 |
| Million-character value / declarations | 9.750 | 9.958 | 34.833 | 1,326 | .000 |
| 1,000-level unrendered DOM / computed | 4.084 | 4.208 | 10.125 | 517 | .000 |

All **16,000 reads succeeded with zero timeouts**. The last row returns
NotComputed, not fabricated values. Four-operation retained RSS deltas were
**.438 / .141 / .016 / .000 MiB** for small / large / huge / deep. Setup deltas were
14.172 / 14.531 / .469 / .453 MiB. These RSS differences include allocator/font
startup and reuse; they are not total document allocation measurements or a leak
proof. Responses are dropped each iteration and the backend retains no inspection
state. The UI's additional retained payload is one bounded 64 KiB style page plus
bounded query/reference/model overhead; D3-A's 512 KiB DOM cache is unchanged.

Reproduce with the checked-in opt-in benchmark:

```sh
cargo test --release --locked --manifest-path engines/Cargo.toml -p wega-engine --test inspector_styles_bench -- --ignored --nocapture --test-threads=1
```

### Existing-path comparison

Five adjacent before/after pairs per benchmark, reversing order each pair, used
preserved optimized D3-A binaries (`ac6928e`) and optimized D3-B binaries
(`fdd3941`). Our compiler/test dispatcher was paused; other host work remained
(observed load approximately 5.23–6.96). An earlier three-pair run while compiling
was noisier and was not used for the comparison. Individual samples are retained
in the evidence JSON. Values below are medians of five run medians, with ranges.

| Closed/normal path | D3-A median [range] | D3-B median [range] |
|---|---|---|
| 800×600 record, ms | .19 [.09–.24] | .18 [.14–.23] |
| 800×600 raster, ms | .39 [.24–.50] | .38 [.34–.58] |
| 1600×1200 record, ms | .23 [.10–.30] | .24 [.14–.35] |
| 1600×1200 raster, ms | 1.12 [.61–1.60] | 1.10 [.87–1.79] |
| Large DOM 64-child page, µs | 122.292 [87.416–124.375] | 122.167 [101.583–123.667] |
| Native attribute mutation, µs | .0516 [.0370–.0584] | .0523 [.0378–.0659] |
| Native text mutation, µs | .0347 [.0253–.0367] | .0334 [.0238–.0345] |
| Console simple log, µs | 17.80 [13.13–25.18] | 18.86 [11.24–19.96] |
| Console nested object, µs | 47.75 [37.67–61.01] | 50.12 [32.33–51.03] |
| Console warning, µs | 20.82 [18.73–28.27] | 24.98 [15.99–25.91] |
| Console eight numbers, µs | 18.47 [16.03–40.43] | 18.90 [15.94–26.32] |
| Console wide object, µs | 142.64 [130.95–186.63] | 137.60 [119.88–164.24] |

Rendering, DOM pagination and native mutations remain within measured spread.
Console warning median increased **20.0%**, with overlapping ranges. A further
five alternating whole-console-process CPU comparisons measured median aggregate
user+system CPU **7.248 s before / 7.771 s after (+7.2%)**; corresponding wall times
7.105 / 7.648 s. These include setup, stderr formatting and the 100,000-call flood,
not only capture. Console capture/mutation hot-path files have no diff from
D3-A, and no always-on style inspection hook was added. The measurements preserve
the D3.0 bounded algorithms but cannot establish a tight console no-regression
claim or attribute this noisy increase. Repeat on an isolated host before using
a small percentage as a release gate; this performance uncertainty remains open.

## Remaining limitations and D3-C prerequisites

No final executed test failures. Supported D3-B reads are complete; matched rules
remain a deliberate stopped subfeature, and console microbenchmark uncertainty
above is unresolved. Linux/Windows, screenshot visual review, full upstream WPT,
long-duration memory soak and an isolated-host performance gate were not run.
The pre-existing depth-122 first-layout hang remains: this work avoids it rather
than fixing normal layout. No DOM/CSS editing, picker, overlay, highlighting or
D3-C implementation was added. WebDriver code and its prior D3-A fixture repair
are unchanged; its HTTP suite was not rerun for this slice.

**D3-C geometry/highlighting is not safe to start with a synchronous layout
flush.** First fix the reproducible first-layout hang or provide a separately
proven bounded/cancellable geometry contract. Editing also needs a design for
page-visible changes, permission/undo lifecycle and broader CSSOM epoch tests;
this read-only protocol grants no edit authority. Matched rules need a separate
runtime-backed provenance design: either retain rule/selector provenance during
matching (measured vendor/common-path impact), or introduce bounded cancellable
matching. Wider computed coverage needs bounded serializers and explicit
freshness/used-value semantics. Preserve this slice's identity, epoch, session,
byte/time and pending-request checks in any subsequent work.
