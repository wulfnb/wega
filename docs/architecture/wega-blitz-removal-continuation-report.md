# Blitz removal continuation: 18 November 2026

**PARTIAL MIGRATION. M1–M5 are not complete.** The production browser still
constructs and shares `BlitzDocument`. The production independence checker still
fails. No native-only production browser or post-owner-migration security result
is claimed.

The clean starting checkout was `2d067b2` on `feature/dom-rounds-9-12`. The current
source checkpoint is `f13d365` (XML rejection cancellation follow-up to `2d38821`). Nothing was pushed. Inspector/D4 implementation,
Chrome tolerances, CSP/CORS/origin rules and frame/depth limits were not changed.

## Implemented production integration

| Local checkpoint | Change |
|---|---|
| `7fe5ccc` | Blank/HTML/XML construction uses the existing Wega parser sink; production `blitz-html` edge removed, reference oracle retained |
| `ec05d28` | Tree-only V8 reads, serialization, form association and detached-node accounting use document contracts; comment writes update the mutation clock |
| `64ce6ec` | V8 wrapper retention, activation/script requests, script queues and meta-policy bookkeeping use Wega `NodeKey`; concrete traversal removed from host/view counting |
| `2d38821` | Production raster/SVG payloads, decoders and the shared admission budget moved to `wega-native::images`; reference types forward to those payloads |
| `f13d365` | XML budget rejection aborts resources discovered during construction; the Chrome runner continues every suite while retaining overall failure |

XML uses the pinned xml5ever tokenizer/tree builder through the existing Wega
mutation sink, including namespaces, case, entities, malformed-input recovery
and scripts scheduled after construction. Navigation now enforces its node
budget during XML construction. Tests exercise the Wega owner directly and
compare the production adapter's output with the retained reference parser.

The generic content algorithms operate on the single borrowed tree. Iterative
serialization/text traversal supports deep storage without increasing browser
depth limits. Comment-data writes now preserve identity, count effective changes
and leave the mutation clock unchanged on no-ops or rejection.

The host's production `NodeKey` values still wrap reference node identities.
Process-unique Wega tree identities become the runtime authority only when the
production owner switches. This refactoring alone does not complete M2.

The decoder extraction preserves one process budget and the original pixel
allocation's Arc owner. Fetching, cancellation, stylesheet/resource ownership,
broker font options and CSP decisions remain at their existing boundaries.
The independent decoder enables exactly PNG/JPEG/GIF/WebP, matching the observed
production feature set. SVG resolvers cannot open external files or URLs.
Original upstream licenses and exact vendor patches are retained. The vendor
verifier uses `patch -E` so /dev/null deletions are represented accurately before
its unchanged exact-file comparison.

## Milestone status and remaining work

| Milestone | Status | Remaining production boundary |
|---|---|---|
| M1 | Incomplete | `view.rs` blank/navigation constructors, parser production type and shared document lifetime still use `BlitzDocument` |
| M2 | Incomplete | V8 query/CSSOM/editor/control calls, focus/default actions and geometry-owner integration still reach that document |
| M3 | Incomplete | Ordered stylesheet registry/CSSOM, resource discovery/completion/cancellation, fonts, control state and iframe document ownership remain there; decoded image payloads/algorithms have moved |
| M4 | Incomplete | Browser graph retains `blitz-dom`, `blitz-paint`, `blitz-traits`, `wega-adapter-blitz`; runtime-selectable reference rendering remains |
| M5 | Incomplete | Regression validation of this partial integration is recorded separately; native-only browser validation requires the missing ownership switch and a passing production graph gate |

Concrete evidence: `view.rs` owns `Rc<RefCell<BlitzDocument>>` and constructs it
with `from_html`/`empty`; `html_report.rs` instantiates
`Parser<BlitzDocument, HtmlReport>`; `script.rs::HostContext` and `dom.rs::Dom`
carry the same owner; `frames.rs::FrameContext`/frame access use strong/weak
references to it; `native_engine::NativeDocument` consumes its sheets, controls,
font context and decoded resource registry. Replacing these references before
their operations migrate would drop behavior or require a forbidden mirror.
No such replacement, fallback tree or hidden mirror was introduced.

The default rendering backend remains the reference backend. Explicit native
rendering and a successful worker `--no-default-features` build do not establish
production independence. The native component's independent graph/build only
establishes that component's boundary. `blitz-html` remains in the lockfile and
reference tests intentionally; no useful comparison test was deleted.

## Validation and recovery

Full validation is running sequentially against source checkpoint `f13d365`.
The final ledger, failed attempts, captures, dependency trees and release
observations will be archived under `baseline/blitz-removal-2026-11-18/`.
Historical evidence in `baseline/wega-blitz-removal-evidence.json` belongs to the
prior campaign and must not be relabeled as validation of this continuation.

Initial runtime fixtures were denied local sockets by the outer tool sandbox;
the authorized broker-fixture reruns passed. An intermediate NodeKey regression
rebuild hit linker ENOSPC. The recovery manifest records 86 obsolete generated
executables (17,241,038,848 bytes), with no active processes/open files, removed
before a successful rerun. A second manifest records nine obsolete executables
(2,182,551,472 bytes) removed after the decoder graph changed. Sources, current
executables, useful comparison coverage and evidence were preserved.

The explicit browser frame limit is still 32, frame nesting 8, native layout
depth 128 and native edit depth 121. Decoder limits remain 8192 pixels per side,
8 Mi-pixels, 64 MiB raster allocation, 8 MiB SVG decompression, 16,384 aggregate
SVG nodes, nested SVG depth 8, 2-second decode admission, 128 MiB reservations,
256 MiB retained budget and 4096 images. No test deadline or security/resource
assertion was relaxed.

The outstanding work is implementation, not an identified external prerequisite.
No claim is made that the requested autonomous M1–M5 objective has been achieved.
