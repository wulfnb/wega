# Wega engine vendored sources

## Deno core (`deno/`)

`deno_core` 0.413.0, `deno_ops` 0.289.0, `serde_v8` 0.322.0 and `deno_v8` 0.5.0 are
the published crates.io sources, **unmodified**. Deno's MIT license is added as
`LICENSE.md` because the published crates do not include it. Cloudflare's acquisition
of Deno (October 2026) ends upstream runtime development in about a year, so Wega keeps
its own copy. The reasons and the update procedure are in
[docs/v8-engine-plan.md](../../docs/v8-engine-plan.md).

They are used through `[patch.crates-io]`, currently only in `engines/v8-spike`.
The V8 engine CI workflow (`.github/workflows/v8-engine.yml`) fails if any file differs
from the crates.io release. When a file has to change, add `Modified by Wega
contributors, 2026` near its top, keep the original header, document the change
here, and update the workflow's comparison.

`deno/V8-PREBUILT-SHA256SUMS` pins the prebuilt V8 static libraries and generated
bindings (`v8` 150.4.0, `simdutf_release`, 5 targets) that the `v8` crate downloads.
The workflow checks that they are still published by `denoland/rusty_v8` and keeps a
verified copy in this repository's `deno-v8-mirror-150.4.0` pre-release. To build from
that copy, download the assets into `<dir>/v150.4.0/` and set `RUSTY_V8_MIRROR=<dir>`
(a plain path, not `file://`).
