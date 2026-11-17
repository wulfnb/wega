# Wega native engine campaign continuation — 2026-11-17

**Overall: PARTIAL.** Gemini's interrupted work was recovered and Rounds 3–6
now have local implementation commits and acceptance evidence. COMPLETE means
only the explicit supported slice. The browser is not generally Chrome-compatible
or standards-complete. No push was issued and DevTools D4 was not started.

## Recovery and exact checkout

Recovered on `feature/dom-rounds-9-12` at `d909d41`, following Round 1
`22a55b0` and Round 2 `d909d41`. Nine tracked files contained uncommitted
Round 3 work; there were no staged or untracked source files. No Gemini/build
process was still running. Original files, metadata, hashes, status and binary
diffs were saved before edits outside the repository. The durable copy is
`/Users/noufalbabu/Documents/wega-gemini-recovery-20261117-205428`;
all nine original hashes were verified. No reset/clean/destructive checkout or
branch replacement occurred. Existing Round 1/2 implementations were retained
and their acceptance suites ran again.

The tested implementation is `7d21f5db9ea520417b0f860b967e5ee351e95776`.
The closure commit adds this report/evidence and an unchanged iframe assertion's
failure diagnostics. It makes no production implementation change. Historical
pre-Gemini evidence at `6769d8a` is retained separately in the evidence JSON;
its older totals are not the results of this continuation.

## Round outcomes and supported scope

| Round / capability | Status | Delivered behavior and limits |
|---|---|---|
| Round 3 supported at-rules/fonts/CSSOM | COMPLETE | Media responds to viewport, orientation, scheme and resolution. Authorized imports, depth 8 and per-document resource expansion bound 512. Broker-loaded fonts update Parley shaping/layout/pixels. Owner-backed style/link sheets, live ordered `document.styleSheets`, insert/delete rules, disabled/media changes and synchronous geometry/pixels. Syntax/index/security exceptions and cross-origin restrictions tested. |
| CSS/CSSOM overall | PARTIAL | Constructed sheets, full mutable CSSRule APIs, full CSS property/selector/media grammar and font lifecycle APIs remain. Native CSS is serialized from authoritative authored rules; it does not consume Stylo computed output. |
| Round 4 common SVG/graphics | PARTIAL | Inline and broker-loaded SVG shapes/paths, fill/stroke, transforms, viewBox, clipping; compound 2D transforms; circle/polygon/inset clipping shared with picking; positioned/repeated single backgrounds; outer shadow spread. Real canvas RGBA presentation, resize/reset, bounded surfaces and origin-clean export restrictions. |
| Advanced graphics | UNSUPPORTED | 3D transforms, multiple/inset shadows, multiple backgrounds, full SVG DOM/global descendant CSS, externally resolved nested SVG resources, full Canvas 2D coverage. Genuine decoded video frames are absent; the existing synthetic frame generator is not exposed as video support. |
| Round 5 pinned compatibility gates | COMPLETE | Eleven new Chrome slices, one graphics slice, four legacy Chrome fixtures, unmodified pinned DOM WPT assertions and overflow reftest. Runner requires Chrome and enforces unchanged pixel/geometry gates. |
| General web compatibility | PARTIAL | Forms/keyboard, flex/grid/table, responsive fonts, SVG/canvas, scrolling, DOM/events and iframes have bounded fixtures. Four legacy native/reference mismatches and modern public-page layout failures remain. |
| Round 6 native accessibility | PARTIAL | AccessKit roles, explicit/implicit/ARIA labels/names, values, masked passwords, toggles, disabled/focus, actions and native transformed/scrolled geometry. Full AccName/ARIA, text navigation and cross-frame accessibility grafts remain. |
| Round 6 DOM independence | PARTIAL | CSS/layout/shaping/graphics/accessibility use native/Wega types; Blitz bridges are isolated in `dom_adapter.rs` behind `NativeResources`. One authoritative `Rc<RefCell<BlitzDocument>>` remains shared with existing V8 integration. |
| Round 6 measured optimization | COMPLETE | Cached geometry snapshots and compiled stylesheets with DOM/CSSOM/resource/policy/owner/media/focus/viewport/scroll invalidation; measured local bottlenecks improved. No whole-browser speedup claim. |
| Resource/security regression slice | COMPLETE | Broker permissions and destination CSP, redirect-hop CSP, resource bounds, canvas taint, same/cross-origin frames, inherited grants, sandbox and V8 regressions. These tests are not a security proof. |
| Default 2 MiB harness deep reference layout | PARTIAL | Existing reference/Taffy depth-121 reliability test stalls on the default test stack. It passes unchanged on the actual application's 8 MiB engine stack. Explicit killable 2 MiB depth probes remain unchanged. |

No capability is classified BLOCKED: the unsupported/incomplete areas above
need implementation rather than an outstanding permission decision.

## Functional fixes and security boundaries

CSSOM mutation now reaches actual native style, layout and paint. Invalid media
fails closed; nested comma alternatives and conjunctive groups are distinct.
Imported CSS bases use final response URLs. CSSOM access uses final origin.
Font/image/style requests recheck the correct CSP directive before each redirect
hop. Repeated identical parser/style notifications no longer duplicate import
or font fetches; strict request-count tests retain exactly-one and zero-foreign
assertions. Native serialization uses a 16,384-iteration rule budget and checks an 8 MiB
output threshold between complete rules. A single rule/group can cross that
byte threshold; this is not a strict per-rule allocation cap.

SVG uses established usvg/AnyRender infrastructure with bounded decoding and no
unrestricted external resolver. Existing SVG decoder memory limits were kept;
transient admission waits at most two seconds and nested content shares the
root's aggregate reservation. Canvas-to-canvas taint derives from the native
source. Internal display can present tainted pixels while script export stays
denied. Canvas dimensions are bounded to 8192.

Chrome comparisons exposed and fixed block controls entering inline formatting,
positioned replaced boxes contributing inline fragments, body/iframe viewport
background propagation, and `overflow:clip` scrolling semantics. Final stability
checks exposed large recursive native box-builder frames; construction now uses
an explicit heap stack. Depth/node/text limits and deadlines were unchanged.

## Validation

| Check | Passed | Failed | Existing ignored | Scope |
|---|---:|---:|---:|---|
| Engine workspace | 849 | 0 | 25 | `RUST_MIN_STACK=8388608`, four threads; all workspace packages and doc tests |
| Root package | 233 | 0 | 1 | Root application tests/doc tests |
| Native broker/security selection | 57 | 0 | 1 | Explicit native backend and broker enabled |
| Mandatory Chrome/WPT gate | 38 | 0 | 0 | 16 Chrome comparisons + selected WPT/acceptance suites |
| Required native sandbox | 1 | 0 | 0 | Fresh signed helper, strict codesign verification, real JIT/storage/cookies/frames/grants |
| Existing inspector sandbox | 1 | 0 | 0 | Existing reference backend test; no D4 source work |
| Public-page probe | 1 | 0 | 0 | Four bounded authorized navigations, captures reviewed |
| Release benchmarks | 2 | 0 | 0 | Comparable-work benchmark and native stage profile |
| Engine/root all-targets Clippy | PASS | — | — | `-D warnings` |
| Engine/root rustfmt; diff checks | PASS | — | — | Final formatting and whitespace checks |

Commands and per-step exits are preserved in `gate.json`, `sandbox-results.json`,
`remaining-validation-results.json` and the machine-readable campaign evidence.
The full-workspace command was:

```sh
RUST_MIN_STACK=8388608 cargo test --manifest-path engines/Cargo.toml --workspace --offline -- --test-threads=4
```

The compatibility command was:

```sh
python3 scripts/native-compat-gate.py --chrome-bin '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' --output /private/tmp/wega-rounds-final-evidence
```

Counts are Cargo-reported tests, not browser conformance cases. Suites overlap;
opt-in wrappers that return early without their environment are not presented
as exercised capabilities. Chrome, public probes and required sandbox were
explicitly invoked. Existing ignores were not changed; no tests/tolerances were
weakened. Vendored deno_core deprecated-API warnings remain.

An initial full-workspace attempt hit disk exhaustion. After checking active
processes, only 94 obsolete generated test executables from old dependency
fingerprints were removed (18.077 GB); sources, current graph, logs and recovery
were preserved. Another run under simultaneous root/Chrome workloads hit the
existing iframe fixture's 10-second wait (8 instead of exactly 32 frames).
The exact 32-frame assertion passed alone; its failure message now includes
notices. The default-stack reference stall was sampled and terminated; final
workspace runs on the production engine stack. These earlier attempts remain
in archived logs and are not described as successful runs.

## Chrome, WPT and public pages

Chrome 157.0.8103.79: **16/16 selected gates passed**. Thresholds are unchanged:
per-channel tolerance 8, differing-pixel fraction at most 1%, geometry at most
0.5 px. Of eleven new pins, nine are pixel-identical; forms differ at 1042/252000
pixels (0.413492%) and scrolling at 1200/252000 (0.476190%). All eleven have zero
geometry error. Graphics differs at 275/252000 (0.109127%), geometry zero. The
four legacy Chrome gates also pass; article is the largest at 0.955159%.

Pinned upstream DOM WPT: **1685/1685 assertions**, plus seven frame groups.
The unmodified overflow:clip reftest is byte-identical. These are selected
coverage, not the full WPT suite. Legacy native/reference visual corpus:
**8/12 joint gates**; login appearance 1.830159%, table 1.309524% / 6px,
scroll geometry 180px and transformed overlap geometry 22.474487px remain.
Chrome supports the pinned native scrolling/transform geometry.

Final-head authorized probes navigated example.com, CERN's original project
page, rust-lang.org (redirecting to rust-lang.org without www) and
`doc.rust-lang.org/std/`. All four produced native pixels without missing glyphs
or a navigation crash. Manual 800x600 capture inspection: example/CERN text is
readable; Rust's homepage has severe overlapping content and crowded navigation;
std documentation shows only the sidebar in the initial viewport. All four
remain PARTIAL, and the latter two fail general visual usability. Initial glyph
counts were 900 / 961 / 3122 / 19714; glyph counts do not establish layout quality.

Public captures are initial bounded-navigation snapshots, not a proof that
every asynchronous resource settled. Authentication, form submission and other
public-origin interactions were not exercised.

## Measurements

Debug 200-node/200-rule fixture, ten samples: geometry-query p50
35.865ms -> 0.851ms, mutation CSS processing 6.433ms -> 0.071ms,
mutation relayout 75.277ms -> 68.845ms, scroll recording 0.684ms -> 0.654ms.
These compare the local before/after captures, not independent machines or
whole-browser browsing performance.

Final release benchmark: five warmups, 50 interleaved samples per backend,
44 actual glyphs in each scene, identical pixels and zero geometry error.
All p50/p95/p99 values are in `benchmark-release.json`.

| Fixture stage | Native p50 / p95 ms | Reference p50 / p95 ms |
|---|---:|---:|
| DOM construction | 27.285625 / 34.017958 | 27.179375 / 28.592791 |
| Cold style/layout | 0.266959 / 0.346292 | 0.239208 / 0.300875 |
| Mutation relayout | 0.124875 / 0.149791 | 0.070292 / 0.088375 |
| Warm scene record | 0.010625 / 0.014791 | 0.018250 / 0.023083 |
| Raster | 0.103042 / 0.121125 | 0.086208 / 0.094500 |
| Scroll record | 0.005167 / 0.006416 | 0.006875 / 0.008416 |

Native is slower in this fixture's cold style/layout, mutation relayout and
raster stages; warm/scroll scene recording is faster. This is not evidence of a
whole-browser performance advantage. DOM construction still uses the shared
adapter. Shaping is included in layout and must not be added as independent work.

The final release 200-node/200-rule profile (10 samples) measured p50: 200 geometry
queries 0.102292ms, mutation CSS processing 0.002459ms, mutation relayout
4.739334ms, scroll record 0.164333ms. Native last-stage style/box time was
3.214041ms, layout 1.502416ms (including shaping 1.413666ms).

`/usr/bin/time -l` maximum RSS: comparable native+reference benchmark process
53,362,688 bytes; native stage-profile process 28,950,528 bytes. These are combined
process values, not per-backend allocation or whole-browser memory measurements.
Process cold start, allocation counts, GPU memory and independent repeat-run
confidence remain unmeasured.

## Commits, evidence and remaining work

| Local commit | Scope |
|---|---|
| `de8d9b8` | Round 3 recovery, media/import/font/CSSOM |
| `91a185c` | Round 4 SVG/canvas/graphics |
| `a5f760a` | Round 5 Chrome/WPT gates and compatibility roots |
| `0fe87e2` | Round 6 accessibility, DOM bridge isolation and measured caches |
| `7d21f5d` | Redirect CSP, duplicate resource notifications and native stack use |
| closure commit containing this report | Final evidence and iframe failure diagnostics |

[Machine-readable evidence](baseline/wega-native-engine-evidence.json) includes
commands, exact test counts, hashes and release metrics. Captures, metrics and
compressed validation logs are in
[the continuation evidence directory](baseline/native-rounds-3-6-2026-11-17/).
Inspector/DevTools source has no continuation diff. No live remote query or
push was needed.

Next work: fix the recorded public-page layout roots and legacy visual failures;
expand CSS/CSSOM/font lifecycle and SVG/Canvas coverage; implement real media
decoding before claiming video; complete accessibility naming/frame integration;
replace remaining authoritative DOM adapter dependencies; broaden stress/fuzz
and independent release/memory measurements. The final tested stack setting is
part of the validation scope, and the default-stack reference stall remains.
