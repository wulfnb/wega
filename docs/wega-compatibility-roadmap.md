# Wega compatibility roadmap

Status: 27 October 2026. This lists what the Wega engine (Blitz + V8 worker) still
lacks compared with a full browser engine, after the web-correctness work described
in [engine-strategy.md](engine-strategy.md#what-the-first-worker-actually-supports).
Each item says how we intend to build it and roughly how large it is. Sizes are
person-weeks of focused work, not commitments. P4–P6 in
[v8-engine-plan.md](v8-engine-plan.md) (speed and RAM, CSS, rollout) are not repeated
here.

Order of work follows what breaks the most real sites first: sandboxing before wider
use, then iframes and components, then storage and workers.

## Security and isolation

| Item | Approach | Size |
| --- | --- | --- |
| OS sandbox for the worker | macOS: Seatbelt profile (`sandbox_init`) allowing only the stdin/stdout pipes, the profile storage directory and outbound TCP. Linux: landlock for the filesystem plus a seccomp filter; Docker keeps its own isolation. Windows: AppContainer with a restricted token. Apply before V8 starts. | 3–4 |
| Site isolation for frames | One worker per site once iframes exist (below); frames talk through the browser process. | with iframes |
| HSTS and certificate errors | A shared HSTS store (preload list + `Strict-Transport-Security`), upgrade before connect; certificate errors become an interstitial instead of a network error. Belongs in the P4 shared network broker. | 2 |
| Third-party cookies and storage partitioning | Key cookies and storage by top-level site as well as origin once frames exist; block third-party cookies by default. | 1 |
| CSP reporting, `frame-ancestors`, Trusted Types | `report-to`/`report-uri` via the network queue; `frame-ancestors` with iframes; Trusted Types later. | 1–2 |
| Safe Browsing | Out of scope unless a privacy-preserving list is chosen. | — |

## Web platform

| Item | Approach | Size |
| --- | --- | --- |
| iframes and `postMessage` | Blitz already has sub-document support (`iframe.rs`). Give each frame its own document and script host in the worker (same-site) or its own worker (cross-site), route input by hit testing, and implement `postMessage`/`MessageChannel` over the existing structured clone. | 6–8 |
| Shadow DOM and custom elements | Shadow roots need Blitz/Stylo support for scoped styles and slots (upstream work). `customElements.define`, upgrade and lifecycle callbacks can be done in the bindings once shadow roots exist. | 6+ (upstream) |
| Canvas 2D | Implement `CanvasRenderingContext2D` on Skia (the P5 paint backend) into a Blitz custom widget or image node. | 4–6 |
| WebGL / WebGPU | wgpu-based; only after the GPU compositor. | 8+ |
| Media (`<video>`, `<audio>`, MSE) | A separate media worker using the platform decoders; frames as textures. EME/DRM stays on Servo. | 8+ |
| Web Workers | A second V8 isolate per worker in the same process with its own event loop and `postMessage`; SharedWorker and Service Workers after that (Service Workers need the shared network broker for interception). | 3 (Workers), 6+ (SW) |
| IndexedDB, Cache API | Deno has no IndexedDB extension; build on SQLite in the per-origin storage directory with quotas. | 4 |
| Selection, Range, clipboard, find in page | Blitz has text selection only inside inputs. Page selection needs Blitz support (upstream); `Selection` and the remaining Range editing/geometry methods in the bindings; clipboard through the browser with a permission check. | 4–6 (upstream) |
| `contentEditable` / rich editing | After selection and Range. | 4+ |
| Generated bindings | WebIDL-to-JS generation for the remaining DOM interfaces (the strategy's plan); grow the WPT subset as interfaces land. | ongoing |

## Rendering

| Item | Approach | Size |
| --- | --- | --- |
| GPU compositor and partial repaint | P4 now has an owned display list, a separate raster thread and acknowledged shared-memory frames. Remaining: GPU compositing, damage-only repaint and compositor-driven async scrolling. | 6–8 |
| CSS gaps (sticky, filters, multicol, 3D transforms, container queries, line-clamp) | P5 implements tested subsets of sticky, filters, container queries, clamp/ellipsis, shadows and blend modes. Multicol, 3D transforms, border-image and broader conformance remain upstream work. | P5 |
| Responsive images and lazy loading | `srcset`/`<picture>` selection, `loading=lazy` via the intersection logic, decoded-image budgets. | 2 |

## Tooling and accessibility

| Item | Approach | Size |
| --- | --- | --- |
| Accessibility | Blitz builds an AccessKit tree; send updates over the worker protocol and attach them to the native window's tree. | 3 |
| Developer tools | Expose the V8 inspector over a local WebSocket for the console/debugger first; DOM inspection later. | 3+ |
| WebDriver | Map the existing harness commands to the worker protocol (navigation, evaluate, input, screenshot). | 2 |
| Crash reports | Collect worker exit status and V8 fatal messages; minidumps later. | 1 |

## Known limits of the current implementation

- CSP for Blitz-loaded subresources is decided by file extension, because Blitz
  requests do not say what they are for; unknown extensions pass if any of
  `img-src`, `style-src` or `font-src` allow them.
- `document.write` after the document finished loading is ignored with a notice
  (browsers would replace the document).
- Module imports are fetched without a CSP check of their own (the entry script is
  checked).
- Popups open as normal tabs in the browser's default engine; the opener gets a
  stub window object.
- Script-held wrappers keep replaced DOM text charged against the budget until the
  wrapper is garbage collected.

## Trak.in compatibility regression (27 October 2026)

The bindings now provide live Range boundary points, contextual HTML/SVG fragments,
text insertion and selection-to-string. Contextual fragment scripts execute on
insertion under CSP; innerHTML scripts remain inert. Document visibility follows
tab freezing, and legacy document.domain assignment cannot relax the origin.
Document.namespaceURI is absent, as in browsers, and is no longer misreported as a
missing API during feature detection. document.scripts is a live HTMLCollection.
Fallback diagnostics only identify missing methods named by the failing call;
unrelated TypeErrors no longer inherit earlier jQuery/private-property probes.

Live collections cache a query within each script step until a DOM mutation.
Sibling and child indexing avoid materializing whole child lists. Synchronous
style and geometry reads reuse layout until a mutation; the next script step
invalidates both caches so parser, input, viewport and resource changes are seen.
Script deadlines and DOM allocation limits remain enforced. Regressions cover
wide trees, live collection mutations, layout invalidation, contextual fragment
CSP and budgets, and event-loop deadline diagnostics.

Local validation: 126 workspace tests and 26 broker integration checks passed,
plus strict workspace Clippy and formatting. A direct 60-second trak.in worker
run completed with three successful periodic script probes, 28 headings and no
compatibility fallback, createRange error or script deadline. Optional third-party
ad scripts still report ordinary exceptions; their success is not claimed. The
local macOS development bundle was updated and its signature verified; this does
not validate a new distributed or notarized DMG.

Range cloneContents, extractContents, deleteContents, surroundContents and geometry
methods, plus window Selection and rich editing, remain unsupported. These changes
do not imply full DOM Range conformance or complete third-party widget support.
