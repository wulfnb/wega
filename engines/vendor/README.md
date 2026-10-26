# Wega engine vendored sources

## Deno core (`deno/`)

`deno_core` 0.413.0, `deno_ops` 0.289.0, `serde_v8` 0.322.0 and `deno_v8` 0.5.0 are
the published crates.io sources, with documented patches to `deno_core`. Deno's MIT license is added as
`LICENSE.md` because the published crates do not include it. Cloudflare's acquisition
of Deno (October 2026) ends upstream runtime development in about a year, so Wega keeps
its own copy. The reasons and the update procedure are in
[docs/v8-engine-plan.md](../../docs/v8-engine-plan.md).

They are used through `[patch.crates-io]`, in the `engines/` workspace and `engines/v8-spike`.
The V8 engine CI workflow (`.github/workflows/v8-engine.yml`) compares every file with crates.io after applying the checked-in patches. When a file has to change, add `Modified by Wega
contributors, 2026` near its top, keep the original header, document the change
here, and update the workflow's comparison.

`deno/V8-PREBUILT-SHA256SUMS` pins the prebuilt V8 static libraries and generated
bindings (`v8` 150.4.0, `simdutf_release`, 5 targets) that the `v8` crate downloads.
The workflow checks that they are still published by `denoland/rusty_v8` and keeps a
verified copy in this repository's `deno-v8-mirror-150.4.0` pre-release. To build from
that copy, download the assets into `<dir>/v150.4.0/` and set `RUSTY_V8_MIRROR=<dir>`
(a plain path, not `file://`).

### Embed extension sources (26 October 2026)

`deno/patches/embed-extension-sources.patch` makes `include_js_files!` and
`include_lazy_loaded_js_files!` embed file bytes in the binary. Upstream 0.413
records absolute build-machine paths because it assumes a startup snapshot. P0 had no startup snapshot; those paths caused packaged Linux workers to
show "No such file or directory" on every page. No extension implementation or
permission behavior changes. CI applies the documented patches to a fresh crates.io
copy before comparing; any other source change still fails integrity validation.
P4 snapshots the trusted Wega factories. Residual lazy Deno sources still need embedding.

### Bound the V8 pool (P4)

`deno/patches/two-platform-workers.patch` caps Deno's existing custom platform at
2 workers (including the unavailable-parallelism fallback). It preserves Deno's
foreground queue, Tokio integration and CppGC support.

## Blitz, Stylo and Skia adapter (`blitz/`, P4–P5)

Published `blitz-dom`/`blitz-paint` 0.3.0-beta.4, `stylo` 0.22.0 and
`anyrender_skia` 0.12.0 are pinned by archive SHA-256 in `sources.json`.
`blitz/patches/` is the exact reviewable delta from those releases:

- `blitz-dom.patch`: two-thread Stylo pool, sequential traversal below the
  embedder's 512-node threshold, redundant image-cache release, scheme-aware UA
  text color, content-box container sizes and bounded query restyle convergence,
  visual sticky offsets without cumulative drift, and clamped line-box height.
- `blitz-paint.patch`: CSS blend compositing, inherited text/shadow colors,
  back-to-front text shadows, and font-measured horizontal ellipsis/clamp painting.
- `stylo.patch`: expose the existing line-clamp/-webkit-line-clamp parsers and
  size-container rule parser to the Servo/Blitz configuration.
- `anyrender_skia.patch`: multiply color and glyph-brush alpha instead of
  overwriting the color's alpha (transparent text was otherwise black).

These patches are ready to submit upstream; no upstream PR has been sent.
Original source headers remain intact. Original license texts, Skia's BSD notice,
and their exact source URLs/checksums are in `blitz/licenses/`.

Run `python3 scripts/verify-engine-vendor.py` to download and verify every source
archive and compare every file after applying the patches. For offline validation,
pass `--cache <Cargo registry archive directory>`. CI runs the same verifier; an
unrecorded source change fails even if it builds. The code and pixel/WPT tests
remain the authority for the supported subset; this is not full CSS conformance.
