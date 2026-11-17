# Wega native engine campaign — 2026-11-17

This campaign supersedes the A–E completion and 8.1% speedup claims from
`80bf07e`. Native remains an experimental, partial browser renderer.

Verified start: clean `feature/dom-rounds-9-12`, HEAD `80bf07e`.
Frozen implementation/test head: `6769d8a`. No push or DevTools D4 work.

| Local commit | Delivered work |
|---|---|
| `61d4ca8` | Parley Unicode shaping, font fallback, whitespace and glyph painting |
| `4cbed58` | Token-based CSS, DOM selector matching, cascade, authorized linked CSS |
| `c7bc16e` | Block/flex/grid/table sizing and descendant geometry |
| `1bca499` | Shared native geometry, paint-order picking, automatic invalidation, real frame input |
| `ece03e1` | Broker fonts, raster images, CSP/style metadata, layout/parser bounds, visibility |
| `1a794cf` | Constrained tracks/border-box sizing, explicit resource destinations, observable button clicks |
| `6769d8a` | Differential corpus, Chrome capture, public probes, comparable-work benchmark |

Implemented vertical slices: real positioned glyphs; normal/nowrap/pre/pre-wrap/
pre-line/break-spaces; mixed inline font styles; tokenized declarations and
selectors; variables/cycles and affine calc lengths; block positioning and margin
collapse; flex distribution/wrapping; bounded grid tracks/spans; intrinsic table
columns/spans; opacity, blur, per-side borders, transforms/clips; raster images;
mutation/resource/viewport invalidation; synchronous JS geometry; pointer events;
clipped child-frame scenes and transformed input; native/reference switching.

Native CSS/layout/display commands do not consume Stylo/Taffy layout or paint.
The authoritative DOM, CSSOM write validation, resource discovery/decoding and
some control defaults still use the existing Blitz adapter. This coupling has
not been eliminated. Brokered native fonts use supplied font bytes with system
font discovery disabled.

Important limits and partial behavior:

- Native depth limit 128 on the engine's 8 MiB stack; nodes 30,000; DOM text 4 MiB.
  The 252-depth native debug probe timed out; bounded subprocess tests now verify
  accepted depth 124 and refusal beyond the native limit.
- CSS at-rules/media queries, downloaded web fonts, advanced inline boxes,
  vertical typography, CSSOM rule edits, SVG/media painting, editable control
  painting, full flex/grid/table algorithms and accessibility remain partial or
  unsupported. Unsupported selectors are rejected, not broadened.
- Viewport units are supported for layout dimensions; viewport-relative typography
  is reported unsupported. Transformed inline text and advanced typography need
  further work. CSS `line-height: normal` uses the current native approximation.
- DevTools remains cache-only; transformed/scrolled mappings that cannot be
  represented faithfully suppress exact overlay claims.
- No general rendering parity, standards compliance, production website support
  or whole-browser speedup has been established.

Final acceptance completed on frozen implementation `6769d8a`:

- Workspace: 820 passed, 0 failed, 25 existing ignored.
- Native real broker/frame/security selection: 53 passed, 0 failed,
  1 existing ignored; required signed sandbox worker separately passed.
- Existing inspector signed sandbox regression: 1 passed on reference backend.
- Typography/visual capture: 4 tests passed; 7/12 joint Blitz gates,
  9/12 pixel gates and 4/4 selected Chrome gates observed within tolerance.
  Capture success does not assert parity of all fixtures; known failures remain.
- Two public brokered pages loaded with readable glyphs and coherent partial
  layout; public interactions not exercised.
- Equivalent-work release benchmark: 50 interleaved samples after 5 warmups,
  44 glyphs per backend, zero pixel/geometry error on its bounded fixture.
  Combined process peak RSS 56,655,872 bytes; per-backend memory/allocations and
  process cold-start unmeasured. No whole-browser speedup claim.
- Final engine clippy `-D warnings`, formatting and whitespace checks passed.
- Fresh ad-hoc signed helper with existing sandbox/JIT entitlements verified;
  native required-worker JIT/storage/cookie/frame/grant/font paths passed.

See [final report](wega-native-engine-final-report.md) for capability statuses,
commands, measurements and remaining issues, and
[machine-readable evidence](baseline/wega-native-engine-evidence.json) for exact
metrics/geometries, log hashes and compact screenshot checksums. Seven local
implementation commits plus a documentation/evidence closure commit; no push.
No source changes follow the tested implementation head. D4 remains untouched.

Next milestone: native inline/replaced formatting and editable controls with
pinned WPT-derived gates, then at-rules/web fonts and wider layout correctness.
Native remains opt-in for general browsing.

## Recovered continuation: Rounds 3–6

Recovery started at `d909d41` on `feature/dom-rounds-9-12`; nine modified
files, no staged/untracked files, no live build/test processes. Exact originals,
metadata, hashes and binary diffs are in
`/private/tmp/wega-gemini-recovery-20261117-205428`. Round 1 (`22a55b0`) and
Round 2 (`d909d41`) remain intact. No push and no D4 work.

Round 3: supported media width/height/orientation/resolution/color-scheme,
nested media conjunction, broker-loaded imports (depth 8; document resource
request budget 512), authoritative owner-backed CSSOM insertion/deletion and
disabled state, live document stylesheet order, cross-origin CSSOM denial,
and broker/CSP-loaded fonts now affect native glyphs and pixels. Constructed
stylesheets, mutable CSSRule wrappers, cascade layers and full CSSOM remain
unsupported. CSSOM serialization exposed a border-default regression; fixed
in the renderer without changing the acceptance assertion.

Round 3 checkpoint: 33 tests pass across eight targeted suites with
`WEGA_TEST_BROKER=1`; log `/private/tmp/wega-round3-verified.log`. Initial
sandbox socket denial and intermediate border/clippy failures are retained
in `/private/tmp/wega-round3-*.log`; no tests were weakened. Chrome was not
enabled for this checkpoint. Rounds 4–6 and final validation remain pending.

Round 4 checkpoint: SVG shape/path/transform/viewBox/fill/stroke/clipping
rendering for inline SVG and authorized SVG images; canvas Skia surface
presentation, resize/reset and taint-preserving readback; compound 2D matrices;
circle/polygon/inset clip paths shared with picking; positioned background
raster/SVG layers and outer-shadow spread. SVG decoding keeps existing memory,
node, byte and nesting bounds; transient admission queues for at most 2 seconds.
Nested SVGs share their root's aggregate reservation. No unrestricted resolver
was introduced. Fixed positioned replaced-element fragment geometry.

31 graphics/browser/canvas tests passed in `/private/tmp/wega-round4-final.log`;
another 31-test earlier-round checkpoint passed in
`/private/tmp/wega-round4-regression2.log`. Engine clippy passed. New graphics
Chrome gate: 275/252000 differing pixels (0.10913%, channel tolerance 8),
zero geometry error, 1%/0.5px gate passed. Evidence:
`/private/tmp/wega-round4-evidence/`. SVG external nested resources, global
HTML CSS styling of SVG descendants, complete SVG DOM targeting, multilayer
backgrounds, inset/multiple shadows, 3D transforms and decoded video remain
unsupported/partial. Existing video function generates synthetic pixels and
was deliberately not presented as decoded video support.

Round 5 checkpoint: mandatory `scripts/native-compat-gate.py` fails without
Chrome and enforces the original 8-channel / 1%-pixel / 0.5px geometry limits.
Eleven new Chrome fixtures all pass (nine pixel-identical; forms 0.41349%,
scrolling 0.47619%; all zero geometry error), plus the graphics gate and four
legacy Chrome gates. The full gate passed 38 Rust tests including 1685/1685
unmodified pinned DOM WPT assertions and seven frame tests. The unmodified
native overflow reftest passes byte-for-byte. No tolerance was increased.
Root fixes: block controls no longer become inline boxes, iframe body colors
propagate to the viewport, and `overflow:clip` clips without programmatic scroll.
Legacy comparison: 8/12 joint reference gates; remaining login appearance,
table sizing, and reference/native transformed/scrolled geometry differences
are recorded. Chrome confirms the pinned transform and scrolling behavior.

Four public broker probes loaded without authentication or security bypass:
example.com, CERN's original page, rust-lang.org and doc.rust-lang.org/std/.
These remain PARTIAL: manual screenshot inspection found severe overlapping
content on Rust's homepage, and only sidebar content visible initially on std.
Readable glyphs or successful loading do not establish site compatibility.
No public interactions were exercised. Evidence/logs:
`/private/tmp/wega-round5-gate/`, `/private/tmp/wega-round5-public/` and
`/private/tmp/wega-round5-clippy.log`. Round 6 and final validation pending.

Round 6 checkpoint: native AccessKit trees expose supported roles, names,
explicit/implicit/ARIA labels, values (passwords masked), toggles, focus and
transformed/scrolled native bounds. Tree/action updates share the existing DOM;
there is no reference-layout flush. Cross-frame accessibility grafts, complete
ARIA/AccName and text navigation remain PARTIAL/UNSUPPORTED.

Blitz-specific CSSOM/resource/font/control bridges are isolated in
`native_engine/dom_adapter.rs` behind `NativeResources`. CSS, layout, shaping,
graphics/display and accessibility consume Wega/native types. `NativeDocument`
still holds the same `Rc<RefCell<BlitzDocument>>` used by V8; this is reduced
coupling, not complete DOM-adapter independence. Broker, origin/CSP, sandbox,
canvas readback and V8 authority paths remain enforced; no D4 work.

Measured debug fixture (200 nodes/rules, 10 samples): geometry-query p50
35.865ms -> 0.851ms, mutation stylesheet processing 6.433ms -> 0.071ms,
mutation relayout 75.277ms -> 68.845ms. Geometry snapshots and compiled sheets
are cached with mutation/resource/policy/owner/viewport/scroll invalidation.
Focus/selection/toggle state changes also invalidate their native representation.
These are local fixture observations, not whole-browser performance claims.
Evidence: `/private/tmp/wega-round6-before/`, `/private/tmp/wega-round6-after/`.

16 targeted tests passed; native broker/frame/origin/V8 security selection
26 passed, one existing required-worker test pending signed-helper validation.
Logs: `/private/tmp/wega-round6-checkpoint.log`,
`/private/tmp/wega-round6-security.log`, `/private/tmp/wega-round6-clippy-final.log`.
Final workspace/native/visual/security/sandbox/release validation remains pending.

Final-audit fixes: resource redirects now recheck the exact style/font/image
CSP directive before every HTTP hop. The strict regression observes exactly
one request to each permitted redirect endpoint and zero forbidden-origin
requests. Duplicate parser/mutation style notifications no longer refetch
identical sheets; a CSSOM generation change still forces reparsing when style
text is assigned again. Both regressions pass with all exact assertions retained.

The existing 124-depth deadline exposed excessive recursive builder stack use.
Box construction now uses explicit heap traversal frames with top-down style
and bottom-up box assembly. The depth/node/text limits and test deadline are
unchanged; the original depth/stability suite passes. Workspace all-targets
clippy passed. First full-workspace attempt hit disk exhaustion; 94 obsolete
regenerable integration executables (old dependency fingerprints, 18.077GB)
were removed after confirming no active processes used them. No source, logs,
recovery files or current dependency-graph outputs were removed.

Final validation continuation: the full workspace on the default Rust 2 MiB
harness stack reached the pre-existing reference/Taffy depth-121 reliability
case and stalled. A process stack sample was saved before termination. The
unchanged reliability assertions pass on `RUST_MIN_STACK=8388608`, matching
`ENGINE_THREAD_STACK` used by the application. Full-workspace validation uses
that stack setting; explicit killable 2 MiB probes in `layout_depth.rs` remain
unchanged. The default-stack limitation is retained in the evidence.

Final checkpoint at implementation `7d21f5d`: engine workspace 849 passed,
0 failed, 25 existing ignored on the documented 8 MiB engine stack; root
233/0/1; explicit native broker/security 57/0/1; mandatory Chrome/WPT runner
38/0/0 with 16/16 Chrome gates and 1685/1685 pinned WPT assertions; signed
required native worker 1/0/0 and existing inspector sandbox 1/0/0. Four public
pages navigated and captures were reviewed; modern Rust pages remain visually
broken. Both release benchmarks, root/engine all-targets clippy, rustfmt and
diff checks passed. Release pixels/geometry are identical in the comparable
44-glyph fixture; native remains slower than reference in cold style/layout,
mutation and raster, so no general speed advantage is claimed. Final report
and hashed evidence are in the existing architecture report/baseline paths.
