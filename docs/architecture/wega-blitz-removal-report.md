# Wega Blitz removal campaign

**Decision: PARTIAL MIGRATION.** Production Wega still depends on Blitz. The
independent native algorithm crate and Wega tree are implemented and tested, but
production DOM/CSSOM/resource/control/V8/frame ownership has not been switched.
No Blitz runtime/vendor code was deleted. Standards completeness is separate
and remains partial.

Started on clean `feature/dom-rounds-9-12` at
`8523150487a953394920e69459ad7f374e57297c`. Frozen implementation for final
validation: `d4d209834ad88db268787010d0670905fced1d08`. The documentation closure
commit is identified by Git history rather than an embedded self-referential
hash. Nothing was pushed. No DevTools D4 or inspector implementation was changed.

| Local commit | Milestone |
|---|---|
| `fd65c63` | Initial dependency graphs, source inventory and recovery checkpoint before production edits |
| `98529ae` | Independent Wega tree and direct html5ever ownership tests; dependency independence checker |
| `9ca7822` | Public-page flex/inline/font sizing defects reduced against Chrome and fixed |
| `0aa6412` | Independent CSS/layout/shaping/display-list crate; owner security hardening and independent release benchmark |
| `d4d2098` | Correct constrained-auto-width block centering found in final example.com review |

## Delivered architecture

Production native rendering now calls algorithms in `engines/native`:

```text
production html5ever parser -> Blitz-backed authoritative document
                            -> Wega document read contract
                            -> wega-native CSS/layout/Parley/display list
                            -> AnyRender/Skia
V8, frames, forms, CSSOM and decoded resources still use that same Blitz owner.
```

The verified independent test slice uses:

```text
existing html5ever parser -> wega-dom::Document
                         -> wega-native CSS/layout/Parley/display list
                         -> picking and real AnyRender/Skia pixels
```

The default backend remains the reference backend; native rendering is still an
explicit selection. Selecting it does not change the document owner.

The second slice is not a hidden mirror: it is constructed separately in tests;
no production view owns both trees. The engine uses the extracted algorithms,
not copied alternate implementations. Inline SVG serialization/geometry is in
the algorithm crate; its actual bounded decoder stays at the existing resource
bridge. Browser resource fetching/security was not replaced.

The Wega tree owns a flat map of nodes. Keys are process-unique u64 values,
allocated monotonically with checked exhaustion and never reused. Node edges
hold keys rather than reference-counted nodes. Validation precedes mutation;
clone, deletion and destruction avoid recursive node ownership. Fragment
insertion moves contents, and template ownership participates in cycle checks.
Mutation epochs and coalesced structure/style/layout invalidations are explicit;
collection does not retain unbounded stale invalidation keys. Namespaced
attributes, detached nodes, template cloning, malformed HTML, foster parenting,
fragments, script pauses/document.write and parser budgets have dedicated tests.
See [design](wega-owned-dom-design.md).

This owner does not yet implement the production stylesheet/resource/control/VM
surfaces. It must not replace a view's owner until those operations preserve the
existing behavior. XML construction and production JavaScript object lifetime
integration remain on the old owner. No production JS API migration is claimed.

## Public layout fixes

Chrome reductions confirmed and fixed:

- percentage flex bases (including `flex:1`'s `0%`) in indefinite columns;
- overflow-dependent automatic flex minima and preferred-width minimum caps;
- variable flex/font shorthands retained until substitution, with cascade order;
- root `rem` font metrics and border-box handling of relative lengths;
- intrinsic inline-block content width and last-line baselines;
- flex padding/border contributions to intrinsic and basis sizes;
- replaced elements nested inside inline links and the owning block's line strut;
- auto margin centering after max-width constrains a block's auto width.

All nine new reduced fixtures use identical original standards-mode HTML in
Chrome and Wega, unchanged 0.5px geometry and 1% pixel gates. Public-page results,
captures and remaining visual failures are in
[the compatibility report](wega-native-compatibility-report.md).

Normative sizing references used during diagnosis:
[automatic flex minima](https://www.w3.org/TR/css-flexbox-1/#min-size-auto),
[percentage flex bases](https://www.w3.org/TR/css-flexbox-1/#valdef-flex-basis-percentage).
The fixes are implementation corrections, not a general Flexbox conformance claim.

## Remaining removal barriers

The browser runtime graph still contains `blitz-dom`, `blitz-html`, `blitz-paint`,
`blitz-traits` and `wega-adapter-blitz`. The production independence checker must
return failure. A successful `--no-default-features` worker build still compiles
Blitz and is not a native-only browser build. The native component's graph/build
success must not be substituted for the browser gate.

Concrete owners still requiring migration:

| Owner | Remaining responsibilities |
|---|---|
| `view.rs` | Blank/navigation constructors; shared authoritative document; concrete node/control/resource/event calls; selectable reference backend |
| `dom.rs`, `script.rs` | V8 node IDs and host operations, CSSOM, input values, selection, form state and synchronous native geometry integration |
| `html_report.rs` | Production parser type is `Parser<BlitzDocument, HtmlReport>` |
| `frames.rs` | Child document ownership, weak references, reference paint composition and event/frame integration |
| `native_engine/backend.rs`, `dom_adapter.rs` | Shared Blitz owner; sheet ownership/generations, broker-decoded images/SVG/fonts and editable control decorations |
| Adapter/vendor | One-shot HTML/XML, authoritative mutation side effects, Stylesheet/CSSOM metadata, resources/editors and reference layout/paint/accessibility |
| Comparison tooling | Parser/style/geometry/visual/benchmark verification still requires a reference backend; preserve this value before deleting it |

The migration strategy and individual source references are in
[the dependency inventory](wega-blitz-dependency-inventory.md). No safe deletion
is available while these production edges remain. This is remaining integration
work, not a claim that the independent algorithm component is impossible.

## Validation and recovery

The final command ledger, results/counts, logs, dependency graphs, artifact hashes
and implementation commit are recorded in
[the evidence JSON](baseline/wega-blitz-removal-evidence.json).
Final validation runs sequentially to avoid fixture deadline contention.
Opt-in Chrome, public network, broker and signed-sandbox paths are enabled
explicitly; an early-return wrapper is not counted as an exercised integration.

Original CSP checks per redirect hop, forbidden-endpoint zero request assertions,
duplicate-resource counts, origins, signed sandbox restrictions, exact 32-frame
limit, 128-level native depth policy and other budgets are unchanged.
The historical reference renderer needed an 8 MiB stack for its deep-layout
acceptance test. The required workspace command retains that historical setting;
it is not proof that a native-only browser is tested. The new flat-storage
clone/drop test handles 10,000 levels on 128 KiB, while native layout still refuses
excessive browser depth.

The crate split changed Cargo fingerprints. To permit the full rebuild on a
nearly full disk, 90 obsolete generated test executables from old dependency
fingerprints were inventoried, checked not running and removed (17,197,188,768
bytes). Current binaries, sources and evidence were retained; the explicit
manifest is archived. One independent Skia build initially hit restricted DNS;
its pinned prebuilt downloaded on the approved network retry. Neither event is
reported as a passing test. Vendored Deno deprecation warnings remain separate.

Recovery details and active command are in
[the checkpoint](wega-blitz-removal-checkpoint.md). Required follow-up is to
migrate the concrete production ownership boundaries, rerun the production
independence gate and native runtime acceptance, and only then retire the
reference runtime/development tooling. Performance findings have their own
[report](wega-native-performance-report.md); no whole-browser speedup is claimed.

Validation is host-specific: macOS arm64 with Rust 1.98. Linux/Windows, full
upstream WPT and long-duration whole-browser stability were not run here.

## Final validation on `d4d2098`

| Check | Passed | Failed | Existing ignored |
|---|---:|---:|---:|
| Engine workspace | 879 | 0 | 25 |
| Root package | 233 | 0 | 1 |
| Native broker/security selection | 57 | 0 | 1 |
| Explicit Chrome/WPT acceptance runner | 47 | 0 | 0 |
| Required signed native worker | 1 | 0 | 0 |
| Existing inspector signed worker | 1 | 0 | 0 |
| Release comparable/stage profile | 2 | 0 | 0 |
| Release independent pipeline | 1 | 0 | 0 |

Engine/root strict Clippy, formatting and whitespace checks passed. Counts overlap;
they are Cargo test counts, not standards coverage. Of the Chrome coverage, 25
comparisons enforce the unchanged full pixel/geometry gates. Two older Chrome
smoke tests retain their original looser width checks and are not counted among
those 25. The selected DOM WPT subset passed 1685/1685 assertions, and the existing
frame and overflow checks passed. Existing ignores were neither added nor changed.

The independent `wega-native` no-default-features build and dependency test pass.
The actual worker no-default-features build succeeds **with Blitz still compiled**;
the production independence gate exits 1. No native-only production build or
post-migration production DOM security/JS validation is claimed.
