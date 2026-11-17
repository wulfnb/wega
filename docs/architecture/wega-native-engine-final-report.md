# Wega native engine final campaign report — 2026-11-17

**Overall: PARTIAL.** The native renderer now paints readable shaped glyphs and
handles the tested CSS, layout, mutation, pointer, resource and child-frame
slices. It is still an experimental renderer. This report supersedes the earlier
A–E acceptance and withdraws the unverified 8.1% performance advantage claim.
No CSS standards compliance, general rendering parity or production website
compatibility is claimed.

Started clean on `feature/dom-rounds-9-12` at `80bf07e7face7af87a04347ad715512a25c3488d`. All implementation and
final checks use `6769d8aedc1a916dab846e6ac6ee74b1c05d0a73`. The final closure commit changes only documentation
and evidence. Seven implementation commits are local; no push was issued.
The stored upstream remains `6b2e9d119a4e662769dbce9163e92e585d020a27`;
no live remote query was made. DevTools D4 was not started; inspector source
paths have no campaign diff.

## Implemented capabilities and limits

COMPLETE below always refers to the explicitly stated slice, not the whole CSS
or browser standard. No remaining capability is classified BLOCKED: unfinished
features are PARTIAL or UNSUPPORTED.

| Capability | Status | Evidence scope / limitation |
|---|---|---|
| Positioned glyph measurement and painting | **COMPLETE** | Supported horizontal text uses Parley glyph IDs/metrics and Skia glyph painting; no text bars remain. |
| Typography overall | **PARTIAL** | System or broker supplied fonts, fallback, weight/style, Unicode/bidi, supported whitespace modes and wrapping; advanced CSS typography/web fonts remain. |
| CSS parsing, matching and cascade | **PARTIAL** | Token-based supported selectors, specificity, important ordering, inherited custom properties and bounded calc; at-rules and full property coverage remain. |
| Authorized linked stylesheets | **PARTIAL** | Ordered linked CSS, dynamic DOM changes, CSP destination checks, MIME/status checks and resource invalidation tested; media/import application and CSSOM edits remain. |
| Block and positioned layout | **PARTIAL** | Common sizing, top/sibling margin collapse, min/max, containing blocks and fixed/absolute slices; full formatting contexts/margin rules remain. |
| Inline layout | **PARTIAL** | Shared shaped line fragments and nested styled runs; advanced inline boxes, transformed inline text and strut details remain. |
| Flexbox | **PARTIAL** | Basis/grow/shrink/min-max redistribution, wrapping/gaps/reverse and descendant relayout; full intrinsic algorithm remains. |
| Grid | **PARTIAL** | Bounded tracks, fractions, minmax, placement, spans/gaps; full intrinsic track algorithm remains. |
| Tables | **PARTIAL** | Content widths, sections, spans and row heights; border collapse and full table sizing remain. |
| Painting, clipping, hit-testing and scrolling | **PARTIAL** | Paint-order regions share transforms/clips, rounded clips, opacity, blur, per-side borders and nested scroll; compound/3D transforms and advanced shadows remain. |
| Authorized raster images | **COMPLETE** | Supported decoded RGBA image content, intrinsic aspect ratio and broker invalidation tested; this status excludes SVG/media. |
| Iframe scenes and input | **PARTIAL** | Actual child scene pixels/clipping and transformed child pointer delivery tested; wider frame/layout combinations remain. |
| Automatic DOM invalidation | **COMPLETE** | Supported DOM/style/text/class mutations, linked/style node changes, resource completion, viewport and scroll updates rebuild synchronously without manual invalidate calls. |
| Geometry and computed-style APIs | **PARTIAL** | Synchronous native rectangles, offset parent, inherited supported resolved values; full serialization/advanced geometry remains. |
| Pointer events | **PARTIAL** | Observable target/bubbling/down/up/click state, transformed/clipped/scrolled elements and frames; full input/default-action behavior remains. |
| Downloaded fonts and runtime font availability invalidation | **UNSUPPORTED** | Only startup supplied/system font collection is supported. |
| Editable control values, caret and selection painting | **UNSUPPORTED** | Basic control geometry and button clicks exist; full native editing display does not. |
| SVG, video and canvas native painting | **UNSUPPORTED** | Raster image content is supported separately. |
| Native accessibility | **UNSUPPORTED** | Returns explicit absence rather than invoking reference layout. |
| DOM adapter independence | **PARTIAL** | Native visible CSS/layout/painting are owned by Wega; authoritative DOM/CSSOM validation/resource decoding/control defaults still use BlitzDocument adapter. |
| Differential capture framework | **COMPLETE** | 12 local fixtures, geometry/pixel metrics and configurable tolerance; four optional sandbox-enabled Chrome comparisons captured. |
| Rendering parity and website compatibility | **PARTIAL** | 7/12 joint Blitz comparisons, 4/4 selected Chrome comparisons; two public pages readable but partial. No general parity claim. |
| Performance and memory evidence | **PARTIAL** | Equivalent-work release fixture, interleaved samples and p50/p95/p99; combined process peak only, no allocation/per-backend peak/process cold-start measurement. |
| Broker/frame/isolate security regression scope | **COMPLETE** | Existing authority/isolation/security suites plus destination/CSP checks and real required-sandbox worker passed; not a full standards/security proof. |
| Native stability bounds | **PARTIAL** | Node/depth/text/CSS/expansion limits and bounded hostile input tests; broader stress/fuzz coverage remains. |

Text uses existing Parley 0.12 infrastructure for shaping, fallback, metrics,
bidi and line formation. Layout stores positioned glyph IDs and font data;
painting sends those same positions through AnyRender to Skia. Nested inline
runs preserve meaningful whitespace across DOM boundaries. Tests cover English,
long wrapping/unbroken words, bold/italic, mixed sizes/families, combining text,
Arabic/Hebrew/CJK, explicit breaks and supported white-space modes. The
[typography capture](baseline/native-engine-2026-11-17/typography-native.png)
was manually inspected and contains readable glyphs.

CSS uses cssparser tokens for nested syntax, strings/comments, escapes and
separators. Supported DOM selector matching includes attribute operators,
sibling/ancestor combinators, :is/:where/:not and structural nth selectors with
specificity/source order. Cascade tests cover important stylesheet declarations
against inline style, inheritance, variables/fallback cycles and supported calc
expressions. Unsupported selectors are rejected. At-rules and many properties
remain unsupported; a successful parse is not a standards conformance claim.

External stylesheets use existing authorized broker loading, URL resolution,
redirect handling and frame authority. Native parses retained raw CSS, rather
than consuming Stylo's computed styles. Destination metadata, CSP nonce/policy
generations, MIME/status failure paths, dynamic DOM link/style ordering and
resource invalidation are tested. Image/font permissions cannot substitute for
stylesheet authority. Raw CSSOM discovery/validation and resource decoding still
use the DOM adapter, so F9 is reduced, not eliminated.

Automatic invalidation uses mutation/resource/policy generations and viewport
state. Synchronous JS geometry shares the native document with View. Tests use
actual JS style/class/text/insert/remove/stylesheet mutations and assert current
rectangles/style/pixels, without manually calling invalidate. Pointer tests
assert JS targets, bubbling and state changes, including transforms/clips/scroll
and child-frame routing. Actual child scene pixels and clipping are tested.

## Regression and security results

| Check | Result |
|---|---|
| workspace | 820 passed, 0 failed, 25 existing ignored |
| native_broker_security | 53 passed, 0 failed, 1 existing ignored |
| required_sandbox | 1 passed, 0 failed, 0 existing ignored |
| inspector_sandbox | 1 passed, 0 failed, 0 existing ignored |
| visual | 4 passed, 0 failed, 0 existing ignored |
| public | 1 passed, 0 failed, 0 existing ignored |
| release_run | 1 passed, 0 failed, 0 existing ignored |
| clippy | PASS |
| format | PASS |

Workspace totals are Cargo-reported and include overlapping suites and existing
opt-in test wrappers; they are not a native conformance count. No assertions
were weakened or tests newly ignored. Public, real-broker and required-sandbox
paths were also explicitly invoked. Existing vendored deno_core deprecation
warnings remain; engine clippy with `-D warnings` passed.

The fresh ad-hoc signed, hardened helper bundle verified with `codesign --verify
--strict` and ran with existing app-sandbox/allow-jit entitlements. The native
required-worker check exercised real glyph ink, JIT, storage/cookies, inherited
grants and frames. The existing inspector sandbox test ran on its default
reference backend. Security regression coverage includes frame-scoped grants,
same-origin/cross-origin access, sandbox flags, CSP destinations, revocation,
iframe routing and V8 isolates. No security controls were relaxed. This tests
the existing enforcement subset, not complete CSP/browser security conformance.

Native bounds: actual node depth 128 on the engine's 8 MiB stack, 30,000 nodes,
4 MiB DOM text, 8 MiB CSS source, plus bounded selector/variable/syntax expansion.
A 252-depth native debug probe timed out during development; bounded subprocess
checks now accept block depth 124 and reject 252/300 before native layout.
Tests also exercise 4,000 rules, malformed/nested syntax, large Unicode text and
repeated invalidation. Exhaustive fuzzing/navigation stress remains future work.

## Visual differential results

Twelve deterministic HTTP fixtures rendered with both backends at 600×420,
scale 1. Each captures native/reference/difference images and DOM geometry.
Tolerance: channel difference >8, at most 1% differing pixels and 0.5px geometry
error, with no masks. **7/12 joint gates and 9/12 pixel gates pass.** Failures are
retained. The capture test hard-asserts the control boxes; other parity values
are reported metrics, not assertions that all twelve match. Separate focused
behavior/paint/typography tests make correctness assertions.

| Fixture | Differing pixels | Max geometry error px | Observed joint gate |
|---|---:|---:|---|
| boxes | 0.005% | 0.000 | PASS |
| article | 0.000% | 0.000 | PASS |
| navigation | 0.272% | 0.000 | PASS |
| login | 1.602% | 0.000 | PARTIAL |
| dashboard | 0.611% | 0.000 | PASS |
| scrolling | 0.000% | 180.000 | PARTIAL |
| positioned | 0.000% | 0.000 | PASS |
| table | 1.310% | 6.000 | PARTIAL |
| image | 4.556% | 0.000 | PARTIAL |
| dynamic | 0.000% | 0.000 | PASS |
| iframe | 0.438% | 0.000 | PASS |
| overlap | 0.000% | 22.474 | PARTIAL |

The article is pixel-identical to Blitz. Login lacks editable input painting;
table height differs by 6px; the image fixture deliberately contains an
unsupported SVG (authorized raster PNG painting has a separate passing test).
Scrolling/overlap have identical native/reference pixels but reference geometry
omits fixed-scroll/transform effects. Native matches Chrome geometry for those
fixtures, so those two Blitz failures are not evidence of a native regression.

Four selected comparisons used Chrome **157.0.8103.79**, a fresh isolated profile,
its sandbox enabled and fixed viewport CDP capture. **4/4 observed gates pass.**
This limited sample does not establish browser parity.

| Chrome fixture | Differing pixels | Max geometry error px | Gate |
|---|---:|---:|---|
| boxes | 0.005% | 0.000000 | PASS |
| article | 0.955% | 0.000000 | PASS |
| scrolling | 0.293% | 0.000000 | PASS |
| overlap | 0.000% | 0.000000 | PASS |

Representative [article](baseline/native-engine-2026-11-17/article-native.png),
[child iframe](baseline/native-engine-2026-11-17/iframe-native.png),
[transformed overlap](baseline/native-engine-2026-11-17/overlap-native.png) and
[remaining form difference](baseline/native-engine-2026-11-17/login-diff.png)
are retained with selected reference/difference PNGs. The machine-readable
[evidence](baseline/wega-native-engine-evidence.json) holds every fixture's
metrics/geometries and checksums for 13 compact screenshots
(444,639 bytes total). Full captures/logs are local
under `/private/tmp/wega-native-evidence` and `/private/tmp/wega-native-*.log`;
those temporary paths are not durable repository artifacts.

## Controlled website compatibility

The deterministic corpus covers articles, navigation, login controls, grid/flex,
long scrolling, positioning, tables, SVG placeholders, dynamic JS, child iframes
and overlap. It establishes only the measured fixture behavior.

Two public pages were explicitly loaded through the real broker in native mode
and their saved screenshots manually inspected:

| Page | Load / readable text / layout | Scroll / interactions | Status |
|---|---|---|---|
| `https://example.com/` | Loaded; 900 positioned glyphs, zero missing; readable multilingual paragraphs, coherent centered layout; SVG logo absent and color-scheme unsupported | 638px document in 600px viewport clamps to ~38px; public interaction not exercised | PARTIAL |
| `http://info.cern.ch/hypertext/WWW/TheProject.html` | Loaded; 961 glyphs, zero missing; readable article and blue links; default decoration/list spacing incomplete | 555px document fits viewport, scroll stays 0; public interaction not exercised | PARTIAL |

Neither crashed or hung. No authentication/CSP/CORS/sandbox bypass was used.
These are two observations of live pages on the capture date, not a general
website compatibility claim. Local JS pointer/mutation assertions supply the
interaction evidence separately.

## Release performance and memory

Host: macOS 27.2.1 arm64, rustc 1.98.0. Release build, five warmups then 50 samples,
alternating backend order. The bounded fixture has **44 native and 44 reference
glyphs**, actual child content, identical pixels and identical probed geometry
before measurement. This replaces the earlier placeholder-text benchmark.

All values below are milliseconds from the final run:

| Stage | p50 | p95 | p99 |
|---|---:|---:|---:|
| native.cold_dom_parse | 23.750125 | 24.899916 | 26.926500 |
| native.cold_style_layout | 0.087541 | 0.141916 | 0.146417 |
| native.css_parse | 0.010542 | 0.017417 | 0.018500 |
| native.iframe_record | 0.007500 | 0.009542 | 0.014958 |
| native.layout_including_shape | 0.028417 | 0.064916 | 0.079583 |
| native.mutation_relayout | 0.066042 | 0.088041 | 0.113500 |
| native.raster | 0.073791 | 0.112875 | 0.152959 |
| native.scroll_record | 0.004000 | 0.005042 | 0.005334 |
| native.shaping_subset | 0.025042 | 0.058041 | 0.069375 |
| native.style_box_tree | 0.047667 | 0.063958 | 0.064958 |
| native.warm_record | 0.007833 | 0.013208 | 0.015375 |
| reference.cold_dom_parse | 23.674333 | 26.464583 | 27.988959 |
| reference.cold_style_layout | 0.204958 | 0.332125 | 0.402542 |
| reference.iframe_record | 0.009792 | 0.012584 | 0.014042 |
| reference.mutation_relayout | 0.056875 | 0.085584 | 0.100292 |
| reference.raster | 0.071916 | 0.095709 | 0.122667 |
| reference.scroll_record | 0.006083 | 0.007625 | 0.008333 |
| reference.warm_record | 0.013542 | 0.026000 | 0.034250 |

“Cold DOM” means a new document inside a warmed process and includes shared DOM
adapter/font construction; it is not process cold-start or isolated HTML token
parsing. Native shaping is a subset of layout. Warm record combines display-list
and scene recording. Reference CSS/style/shape are not independently timed.
These results show different tradeoffs by stage; no aggregate browser speedup
is reported.

The benchmark's sampled RSS high-water is 56,377,344
bytes. A fresh-child `getrusage(RUSAGE_CHILDREN)` wrapper reports actual combined
benchmark-process peak RSS **56,655,872 bytes
(54.03 MiB)**. This includes both backends and the harness;
per-backend peaks and allocations are unmeasured. `/usr/bin/time -l` could not
read a sandboxed kernel clock query, so the own-child rusage measurement replaces
that failed resource wrapper; the benchmark itself passed.

A measured debug hotspot justified caching the parsed native UA stylesheet:
small-fixture style/tree p50 fell from ~1.38ms to ~0.20ms in five-sample debug
probes. This is supporting hotspot evidence, not a release speedup estimate.
Full-tree invalidation and display-list work during geometry/picking still need
profiling before further cache changes.

## Remaining correctness work and next milestone

Full formatting-context rules and bottom-margin collapse, inline box edges and
baseline struts (`line-height:normal` currently uses a 1.25 approximation), flex/
grid/table intrinsic algorithms and border collapsing remain incomplete. So do
compound/3D transforms, transformed inline text, complex nested transformed fixed
positioning, inset/spread/multiple shadows, sophisticated borders, vertical
writing and advanced typography/selection. Viewport length units work for layout;
viewport-relative typography is explicitly unsupported.

CSS @media/@import application/@font-face, downloaded font loads, CSSOM insertRule
synchronization, richer computed values, late-resource lifecycle combinations,
SVG/media/canvas, editable control display/keyboard defaults and native
accessibility need implementation. The BlitzDocument adapter remains. Existing
DevTools inspection stays cache-only, validates document/node identities and
suppresses exact transformed/scrolled overlays it cannot represent.

Next milestone: implement native inline/replaced formatting and editable control
painting, and turn selected WPT-derived geometry/pixel fixtures into pinned
regression gates. Follow with at-rules/web fonts and full common layout rules.
Keep native opt-in for general browsing and retain Blitz for comparison until
those gates support a broader claim.

## Local commits

| Hash | Change |
|---|---|
| `61d4ca8` | feat(native): shape Unicode inline text and paint positioned glyphs |
| `4cbed58` | feat(native): tokenize CSS and integrate authorized linked stylesheets |
| `c7bc16e` | fix(native): size flex grid and table content with real geometry |
| `1bca499` | fix(native): unify paint geometry input and automatic DOM invalidation |
| `ece03e1` | fix(native): harden resource authority layout bounds and rendering behavior |
| `1a794cf` | fix(native): correct constrained sizing and resource destination checks |
| `6769d8a` | test(native): add visual compatibility corpus and comparable-work benchmarks |

## Source and test files changed

- `engines/Cargo.lock`
- `engines/services/security/src/csp.rs`
- `engines/vendor/blitz/blitz-dom/src/document.rs`
- `engines/vendor/blitz/blitz-dom/src/mutator.rs`
- `engines/vendor/blitz/blitz-dom/src/net.rs`
- `engines/wega/Cargo.toml`
- `engines/wega/src/dom.rs`
- `engines/wega/src/frames.rs`
- `engines/wega/src/native_engine/backend.rs`
- `engines/wega/src/native_engine/css.rs`
- `engines/wega/src/native_engine/display_list.rs`
- `engines/wega/src/native_engine/layout.rs`
- `engines/wega/src/native_engine/mod.rs`
- `engines/wega/src/native_engine/syntax.rs`
- `engines/wega/src/native_engine/text.rs`
- `engines/wega/src/view.rs`
- `engines/wega/tests/fixtures/native-typography.html`
- `engines/wega/tests/native_advanced_rendering.rs`
- `engines/wega/tests/native_benchmark.rs`
- `engines/wega/tests/native_browser_behavior.rs`
- `engines/wega/tests/native_css_correctness.rs`
- `engines/wega/tests/native_dynamic_integration.rs`
- `engines/wega/tests/native_images.rs`
- `engines/wega/tests/native_layout_correctness.rs`
- `engines/wega/tests/native_paint_correctness.rs`
- `engines/wega/tests/native_public_compat.rs`
- `engines/wega/tests/native_stability.rs`
- `engines/wega/tests/native_stylesheets.rs`
- `engines/wega/tests/native_typography.rs`
- `engines/wega/tests/native_visual_corpus.rs`
- `engines/wega/tests/support/chrome_capture.mjs`
- `engines/wega/tests/support/native_visual.rs`

## Reproduction commands

Commands below are the exact final validation commands. Broker/network/sandbox
checks need the existing host permissions and macOS helper tooling. The release
binary hash is Cargo-build-specific; build first and use the printed executable.

**workspace**

```sh
cargo test --manifest-path engines/Cargo.toml --workspace --offline -- --test-threads=4
```

**native_broker_security**

```sh
WEGA_RENDER_BACKEND=native WEGA_TEST_BROKER=1 cargo test --manifest-path engines/Cargo.toml -p wega-engine --test native_browser_behavior --test native_stylesheets --test native_images --test iframes --test same_origin_dom --test iframe_wpt --test isolate_safety --test security --test inherited_broker --offline -- --test-threads=4
```

**sandbox_bundle**

```sh
WEGA_CODESIGN_IDENTITY=- engines/target/debug/wega-content-bundle engines/target/debug/wega-engine /private/tmp/WegaNativeCampaign-6769d8a.app
```

**required_sandbox**

```sh
WEGA_RENDER_BACKEND=native WEGA_CONTENT_HELPER=/private/tmp/WegaNativeCampaign-6769d8a.app/Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine cargo test --manifest-path engines/Cargo.toml -p wega-engine --test inherited_broker --offline -- --ignored --exact a_required_worker_runs_jit_storage_cookies_and_frames_through_grants --nocapture
```

**inspector_sandbox**

```sh
WEGA_CONTENT_HELPER=/private/tmp/WegaNativeCampaign-6769d8a.app/Contents/Helpers/WegaContent.app/Contents/MacOS/wega-engine cargo test --manifest-path engines/Cargo.toml -p wega-engine --test inspector_sandbox --offline -- --ignored --nocapture
```

**visual**

```sh
WEGA_NATIVE_EVIDENCE_DIR=/private/tmp/wega-native-evidence WEGA_CHROME_BIN='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' cargo test --manifest-path engines/Cargo.toml -p wega-engine --test native_visual_corpus --test native_typography --offline -- --nocapture
```

**public**

```sh
WEGA_PUBLIC_COMPAT=1 WEGA_TEST_BROKER=1 cargo test --manifest-path engines/Cargo.toml -p wega-engine --test native_public_compat --offline -- --nocapture
```

**release_build**

```sh
cargo test --release --manifest-path engines/Cargo.toml -p wega-engine --test native_benchmark --offline --no-run
```

**release_run**

```sh
engines/target/release/deps/native_benchmark-02ee9e75238e9b3d --exact comparable_native_blitz_stage_benchmark --nocapture
```

**clippy**

```sh
cargo clippy --manifest-path engines/Cargo.toml -p wega-engine --lib --tests --offline -- -D warnings
```

**format**

```sh
cargo fmt --manifest-path engines/Cargo.toml -p wega-engine -p wega-security -- --check
```

Final evidence closure also runs `git diff --check`, verifies documentation-only
changes after the tested implementation head, and commits the report locally.
