# Wega browser vendored sources

## Patched SQLite for Servo networking and storage

`servo-net/` and `servo-storage/` 0.7.0 (MPL-2.0), and `sea-query-rusqlite/` 0.8.0
(MIT/Apache-2.0) are published crates.io sources. Their only changes are the
manifest patches in `patches/`, upgrading rusqlite from 0.38 to 0.41.1.
All consumers must move together because Cargo permits only one native
SQLite library in the browser dependency graph. Wega networking and Deno
WebStorage use the same release, with bundled libsqlite3-sys 0.39.1 fixing
RUSTSEC-2026-0459. No advisory exemption is needed for SQLite.

`sources.json` records the upstream archive checksums and exact patches for
these three crates. Run `python3 scripts/verify-engine-vendor.py --vendor vendor`
to verify them. Remove these patches when upstream's SQLite requirements
include the fixed release.

## epaint 0.37.0 compatibility patch

`epaint/` is the published epaint 0.37.0 crate from crates.io, with its original
MIT/Apache license files. It is used through `[patch.crates-io]`.

Only `src/text/font_face.rs` is changed: both glyph rasterization contexts use
`RenderContext::new_with` with `num_threads: 0` instead of `RenderContext::new`.

Servo enables vello_cpu 0.2's `multithreading` feature. Cargo unifies that feature
with epaint's dependency, changing the default render context to multithreaded.
epaint's glyph path assumes single-threaded behavior and doesn't flush before
rendering, causing `attempted to rasterize before flushing` on the first glyph.
Explicit single-threaded glyph contexts preserve epaint's intended behavior and
avoid creating a worker pool for every tiny glyph. Servo's canvas renderer keeps
its own multithreaded contexts. The browser includes a font-rasterization regression
test under the combined dependency graph.

Remove this patch when the upstream epaint glyph context explicitly sets its
threading mode or correctly handles the multithreaded API. No global Cargo cache
or installed dependency sources are modified.

## Servo 0.7.0 native developer-tool hook

`servo/` is the published MPL-2.0 Servo 0.7.0 crate. One change
is in `servo.rs`: `ServoBuilder::devtools_event_sender` accepts an optional native
consumer of the existing `DevtoolsControlMsg` stream. When supplied, it replaces
the TCP developer server. The browser enables the engine's developer machinery
and feeds events to its native toolbox without exposing a listening port.
The parser, networking, rendering, script engine and protocol types are unchanged.
Remove this patch when upstream exposes an equivalent embedder hook.

Every modified file carries a `Modified by Wega contributors, 2026` comment near
the top; the original license headers are unchanged.

## GStreamer SDK discovery

On macOS, `servo.rs` first loads its fixed plugin list from the app executable’s
`lib` directory. Windows uses the executable directory, then the explicit override
or `GSTREAMER_1_0_ROOT_MSVC_X86_64` SDK plugin path for development. Discovery checks
for the actual core plugin rather than the existence of an empty directory. For unbundled development builds it also accepts
`SERVO_GSTREAMER_PLUGIN_DIR`, then the standard Apple Silicon/Intel Homebrew
GStreamer plugin locations. The packaged media build copies the plugin list and
native dependency closure into the app; no installed SDK libraries are modified.

## No GPL or patent-encumbered codecs (gst-libav removed)

`gstreamer_plugin_lists/common.rs.in` no longer lists `gstlibav`. Servo uses the
common list (plus `macos.rs.in` or `windows.rs.in`) on macOS and Windows; Linux
always uses the distribution's plugin registry instead. `scripts/bundle-macos-media.py`
copies exactly these plugins and their dependency closure into `Wega.app`. With
`gstlibav` listed, Homebrew's FFmpeg (built with `--enable-gpl`) and its x264/x265
dependencies were copied into the app, making the bundle GPL-3 and shipping
patent-encumbered encoders.

macOS keeps H.264/HEVC/AAC/MP3 playback through `gstapplemedia` in `macos.rs.in`,
which decodes with Apple's system VideoToolbox (`vtdec`) and AudioToolbox
(`atdec`). VP8/VP9 (`gstvpx`), Opus, Vorbis and Theora still use their BSD-licensed
plugins. The bundler aborts if any `libx264*`, `libx265*`, `libav*`, `libswscale*`,
`libswresample*`, `libpostproc*`, `libfaac*` or `libgstlibav*` library would be
copied. Windows CI/release builds now enable media and bundle selected plugins
and their exact x64 PE import closure from a pinned, checksum-verified SDK using
`scripts/bundle-windows-media.py`. Required Microsoft runtime DLLs come from
Visual Studio’s release x64 CRT REDIST directory, with a separate notice and
per-file hashes; developer-installed system copies cannot satisfy that closure.
The Windows plugin list adds D3D11 and Media
Foundation alongside WASAPI. Licenses and provenance are retained; actual Windows
execution still needs CI validation. Restoring `gstlibav` would require removing that guard and
re-reviewing the licensing of every release artifact.


## Servo 0.7.0 DOM memory-report allocation fix

`servo-script-bindings/` is the published `servo-script-bindings` 0.7.0 crate,
including its MPL-2.0 source headers and BSD-licensed bundled parser sources.
`mem.rs` and `codegen/codegen.py` are modified. No installed Cargo sources are
changed.

The DOM vtable generator previously used the Box allocation-size callback for
all interfaces, including weak-referenceable interfaces reflected from `Rc<T>`.
`Rc::into_raw` points at the value after its reference-count header, so querying
an allocator with that pointer is invalid. Linux ARM64 debug toolbox measurement
aborted with an arithmetic overflow; release arithmetic can obscure such errors.

Generated vtables now select the allocation kind. Rc-backed values acquire a
short-lived strong reference and delegate to the existing Rc measurement trait,
which measures the allocation base and contained values. Dropping the temporary
reference preserves the DOM owner's original count. Box-backed values retain
the direct callback. The patch does not clamp sizes or disable overflow checks.

`cargo test --features test-harness --test servo-dom-memory` runs regressions for
Box and aligned Rc allocations and ownership preservation. A portable Python
regression executes the actual vtable generator for both interface kinds. The
real native toolbox suite remains the end-to-end memory-report check. Remove
this patch when the pinned upstream crate makes the same allocation distinction.
