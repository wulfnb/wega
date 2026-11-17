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
