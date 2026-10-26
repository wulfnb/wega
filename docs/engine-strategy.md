# Engine strategy: a low-memory Wega engine

Status: **experimental P4–P6 iteration**, updated 26 October 2026.
The Blitz-based Wega engine now renders real pages in a private worker process;
the native footer selects Servo or Wega for the active tab. Servo remains the
general default, with a narrow validated static-site allowlist. The original design below includes future work; it does not describe a
finished compatible browser engine. See the implementation checkpoint and the
[engine recommendation at the bottom](#which-engine-should-we-use-now).

## Why

On a home server, Wega in Docker used about 500 MB per busy page, returned only
part of it after tabs closed, peaked at 1.998 GB against a 2 GB limit, and kept
both of its CPUs busy with software rendering. Firefox on the same site used
about as much memory, so this is not a Wega bug, but it is the cost of an
engine designed for maximum compatibility and speed, not for small memory.

Goal: an engine Wega owns, that is **low on memory and CPU first**, **supports
JavaScript**, and grows towards **real-site compatibility**, without breaking
the sites that work today while it grows.

## What we found

### Servo's DOM cannot be reused without SpiderMonkey

Reading Servo 0.7's `script`, `script-bindings` and `layout-api` crates:

- Every DOM node is a SpiderMonkey object. `Node` starts with an `EventTarget`
  holding a `Reflector`, and the Rust struct is freed only when SpiderMonkey's
  GC finalizes its JS object. There is no layer that abstracts the JS engine;
  691 of 883 `script` source files use `js::` types directly.
- Reusing it means carrying about 296,000 lines of `script`, 574 WebIDL files
  with a 43,000-line Python binding generator, and SpiderMonkey itself. Only
  Servo's layout and WebRender painting could be dropped.
- Servo's layout boundary (`LayoutFactory` / `Layout`, about 40 methods) is
  pluggable in principle, but its configuration bakes in Servo's fonts, image
  cache and WebRender paint API, and the real implementation reads the DOM
  through raw node addresses from the `script` crate.
- Servo runs SpiderMonkey with incremental and compacting GC **off** (both
  marked as unsafe with Servo's DOM) and all JIT tiers on. This plausibly
  explains part of the slow, partial memory release seen in practice.

Decision: **no SpiderMonkey and no Servo DOM in the new engine.** Servo stays
only as the existing, full-compatibility engine.

### Blitz is the right base

[Blitz](https://github.com/DioxusLabs/blitz) (DioxusLabs, `0.3.0-beta.4`,
`MIT OR Apache-2.0`) already assembles the Rust parts a modern engine needs:

| Part | Blitz | Notes |
| --- | --- | --- |
| HTML parsing | `blitz-html` on html5ever 0.40 | same family as Servo 0.7 |
| DOM | `blitz-dom` (29,000 lines) | nodes in a `SlotMap` with generational ids; `DocumentMutator` for changes |
| CSS | Stylo 0.22 (pinned fork of upstream main) | the same style engine as Servo and Firefox |
| Layout | Taffy 0.15 (block, flex, grid, positioned), Parley inline text; tables emulated on grid | floats and vertical writing modes behind optional features |
| Paint | `blitz-paint` onto the `anyrender` trait | backends: Vello (wgpu), Vello CPU, Vello hybrid, Skia |
| Network | `NetProvider` trait; `blitz-net` on reqwest | images, SVG (usvg), web fonts |
| Input, forms, accessibility | `UiEvent`, Parley text editing, AccessKit | |
| JavaScript | `blitz-vibey-script` (9,500 lines, unpublished) on Boa 0.22 | runs Preact; DOM in Rust, JS wrappers hold node ids |

The README says it "can already render many popular no-JS websites (Wikipedia,
(Old) Reddit…)". There are no published memory figures.

Other projects already bind JavaScript to `blitz-dom` the same way (nodes owned
by Rust, JS wrappers holding ids): kiln and blitsen use QuickJS (kiln through
`rquickjs`), oxibrowser uses Boa. kiln reports about 26 MB peak RSS headless
and about 105 MB with an idle window (author's measurement on an M3 Pro, not
verified here).

### JavaScript engine candidates

No engine in the low-memory tier has a JIT; script-heavy sites will be slower
than in Servo. test262 scores are from test262.fyi (22 October 2026).

| Engine | Language / license | test262 | Heap cap / interrupt / bytecode cache | Embedding |
| --- | --- | --- | --- | --- |
| **QuickJS-ng** via `rquickjs` 0.14 | C / MIT | 83.5% | yes / yes / yes | mature Rust crate, one runtime per tab gives a per-tab cap |
| **Boa** 0.22 | Rust / MIT | 95.6% | no / no / no (only loop and recursion limits) | native Rust; Blitz's own JS layer uses it |
| Nova | Rust / MPL-2.0 | 76.9% | not found | young; watch |
| Brimstone | Rust / MIT | not listed | not documented | no embedding API yet; watch |
| Duktape, JerryScript, MicroQuickJS | C | ES5 to partial modern | small fixed heaps | no maintained Rust crate, or too limited for modern sites |

Not chosen: V8 and SpiderMonkey (fast and compatible, but heavy); DOMs written
in JavaScript (linkedom, happy-dom) inside the VM, because they would duplicate
the DOM Blitz already uses for style and layout.

## Implementation checkpoint — 24 October 2026

The agreed base is **Blitz**, with Wega owning the engine integration, JavaScript
bindings, limits, network policy and lifecycle. Published upstream crates are
pinned and remain unmodified; copying Servo would retain its SpiderMonkey/DOM
architecture and would not implement this design.

### Two independent builds, one browser window

The first build attempt found an actual dependency conflict: Servo's pinned
`mozjs_sys` requires ICU C API 2.1.2 and ICU 2.1, whereas Blitz 0.3.0-beta.4 requires
ICU 2.3. They cannot resolve together in one Cargo dependency graph. We therefore
build `engines/` with its own Cargo.lock and run one `wega-engine` worker per Wega
tab. The browser shares a bounded, versioned private-pipe protocol with that
worker, rather than forcing incompatible crates into one process.

- `src/servo_host.rs` initializes Servo lazily. A Wega-only startup can render
  without initializing Servo or SpiderMonkey. After Servo is first used it stays
  process-scoped; closing its last tab does not promise to unload the engine.
- `src/wega_page.rs` and `src/wega_ui.rs` connect actual CPU-rendered RGBA frames
  to the existing native chrome. Navigation uses the existing NavigationPort.
  The UI receives only the newest frame; it does not block waiting for scripts.
- The **bottom-right engine selector** reloads the active tab in **Servo** or
  **Wega (experimental)**. It explains that unsaved state is lost. Open-tab engine
  choices persist alongside URL/title data, including normalization and failed
  save handling. Internal browser pages use Servo. Per-site preferences and
  automatic fallback are not implemented.
- Closing or discarding a Wega tab terminates its worker. Background tabs suspend
  scripts/timers and release CPU paint surfaces and frontend textures. Native
  tests cover engine switching in both directions, a real page click, persistence,
  freezing, discard/reload and closing. A stopped worker has Restart Wega and
  Reload in Servo actions.

### What the first worker actually supports

Blitz 0.3.0-beta.4 supplies HTML/CSS, Stylo/Taffy/Parley and Vello CPU painting.
Wega now uses V8 through deno_core as its only worker JavaScript runtime. QuickJS
was removed on 26 October 2026. The bindings operate on the Rust DOM through checked
generational node IDs. The P0–P3 migration adds DOM classes, scoped selectors,
fragments/namespaces/live collections, native styles/layout, DOM events and
observers, frame callbacks, history and native keyboard/IME editing. V8 also
supplies Deno's Web APIs, bounded fetch/XHR, modules, origin-partitioned storage
and cookie_store-backed HTTP/document cookies. The six offline framework fixtures
and selected 31-case DOM WPT subset pass; see [v8-engine-plan.md](v8-engine-plan.md)
for exact evidence and limits. Servo remains the everyday default.

The native capture reports `engine: wega` and `servoInitialized: false`.
Its reported `jsHeapBytes` is the VM's own accounting, **not process RSS or total
page memory**. No memory target in the table below has been certified.

Limits currently include a 32 MiB JS heap, 100 ms script interruption, a bounded
microtask batch, 30,000 DOM nodes, a conservative 16 MiB DOM text allocation
budget, 4 MiB HTML and 8 MiB resource responses, a bounded four-worker network
queue and bounded frame storage. Rendering is capped at 30 fps; idle workers wait
for actual input/resource/timer deadlines. These are component limits, not a
hard total RSS limit or an OS sandbox. Decoded-image/cache budgets, total memory
pressure and representative-site benchmarks remain work to do.

**Still requires Servo:** broad DOM/Web API compatibility, complete forms and
FormData integration, CORS/CSP completeness, website sign-in validation, media,
website accessibility and developer tools. Wega workers now share a profile cookie jar through the bounded browser broker;
live DOM and sign-in state do not transfer between engines. Unsupported APIs report errors; unsupported WebDriver operations
return UnsupportedOperation. Full WPT coverage and real-site compatibility remain
open; passing the selected subset does not certify a complete browser.

### Build and verification

```sh
python3 scripts/build-wega-engine.py --debug
cargo run --locked -- --engine wega https://example.org/
# Or start normally, then use the footer to switch the selected page.
cargo test --locked --manifest-path engines/Cargo.toml --target-dir target/wega-engine
cargo build --locked --features test-harness
python3 scripts/native-tests.py --suite engine
```

The build script installs the helper beside the browser. Release packaging now
includes the helper and a separate `WEGA-ENGINE-THIRD-PARTY-NOTICES.html`; Docker
and Windows build the worker from its independent lockfile. CI checks the worker
and the real native engine round trip. Windows checks include a packaged Wega
capture without SDK PATH. Local macOS and Linux checks do not certify unrun
Windows or remote release jobs.

Current native evidence is under `target/remaining/wega-native/` (page, complete
window capture and report) and `target/remaining/wega-ui.log` (native round trip).
Local verification on 24 October 2026:

| Check | Result |
| --- | --- |
| Core regression gate | 81 tests; 1691/1774 lines, 95.32% (95% required) |
| Desktop unit tests | 83 library, 34 desktop and 2 binding-memory tests passed |
| Real native integration | All nine suites passed, including both footer switches; macOS Launch Services passed |
| Independent worker | Six page/limit/policy tests and the shared protocol test passed |
| Tooling | 62 Python tests, gateway 443/443 lines, existing JavaScript gate and strict Rust lints passed |
| Linux | Both executables built; authentication, smoke, toolbox, downloads, gateway, engine round trip and shell contracts passed in disposable containers |
| macOS package | Both executables bundled, ad-hoc signature verified, Wega rendered with developer PATH/worker override removed |
| Dependency notices | Separate Servo and Wega notices generated successfully with cargo-about 0.9.2 |

The desktop line measurement is 82.77%; whole-project 100% coverage is still
incomplete. Worker tests execute real code but currently have no separate line
coverage export; the inventory lists those files as unmeasured. Windows runtime
and fresh remote CI/release results remain unverified.

The original phases below remain the roadmap: the process boundary and first
static/JS spike are implemented, while full compatibility, benchmark-driven
selection, per-site fallback and release certification remain outstanding.

## Design

The sections below describe the target architecture and roadmap. The checkpoint
above records the implemented subset; per-site choice and a common PageEngine
trait are still future work.

### Two engines behind one boundary

The target design keeps Servo and adds the Wega engine. The browser chooses per site, shows
which engine a tab uses, and offers *Open in full engine* when a page needs
something the Wega engine does not have yet. Over time the Wega engine becomes
the default and Servo the fallback.

At the original design checkpoint the desktop layer called Servo directly
(about 7,300 of 15,300 non-test lines were Servo glue). The new
[architecture decision](architecture-decisions.md) introduces the worker
boundary; a common in-process engine trait remains a later refactor. The
engine-free core (`src/lib.rs` modules, `NavigationPort`, `DownloadPort`,
`PermissionPort`, `Session<T>`, `lifecycle::Activity`) stays as it is.

```rust
/// One per browser window. Servo and the Wega engine each implement it.
pub trait PageEngine {
    type View: PageView;
    fn create_view(&mut self, config: ViewConfig, events: EventSink) -> Self::View;
    fn capabilities(&self) -> Capabilities;
    fn memory_report(&self) -> MemoryReport;
    fn memory_pressure(&mut self, level: Pressure);
}

/// One per tab.
pub trait PageView {
    fn load(&mut self, url: &Url);
    fn reload(&mut self);
    fn stop(&mut self);
    fn traverse(&mut self, delta: i32);
    fn resize(&mut self, size: PhysicalSize, scale: f32);
    fn input(&mut self, event: InputEvent);
    fn paint(&mut self, target: &mut dyn PaintTarget);
    fn set_color_scheme(&mut self, scheme: ColorScheme);
    fn set_zoom(&mut self, factor: f32);
    fn find(&mut self, query: &FindQuery);
    /// Background tabs: drop surfaces and caches, pause timers and scripts.
    fn freeze(&mut self);
    fn thaw(&mut self);
    /// Optional capability; scripting tools (console, reader view) check it.
    fn evaluate_script(&mut self, source: &str, done: ScriptCallback) -> Result<(), Unsupported>;
}

/// What a view reports back to the chrome.
pub enum PageEvent {
    LoadStarted, LoadFinished, TitleChanged(String), UrlChanged(Url),
    FaviconChanged(Image), CursorChanged(Cursor), RepaintNeeded,
    NewWindowRequested(Url), DownloadRequested(Url), Permission(PermissionRequest),
    Dialog(DialogRequest), Unsupported(Feature), Crashed(String),
}
```

`PaintTarget` hides how pixels reach the egui chrome. Servo keeps its shared GL
surface. The Wega engine starts with `anyrender` Vello CPU into a buffer
uploaded as a GL texture, which is simple and works with the existing glow
context, and moves to a GPU path (Skia GL or wgpu interop) once measured.
Devtools, WebDriver and the network panel stay Servo-only at first and are
enabled through `Capabilities`.

### Inside the Wega engine

This is the target design. The first worker uses bounded reqwest networking and
handwritten basic bindings; shared cookies and generated WebIDL are not implemented.

```text
Wega chrome ──PageView──▶ WegaView
                           ├─ blitz-dom Document (single DOM: style, layout, events)
                           ├─ ScriptHost ── V8 isolate (one per tab, capped)
                           ├─ NetProvider ── Wega networking (async, cookies shared)
                           └─ anyrender ── PaintTarget
```

```rust
/// Between the DOM and the JS engine, so the VM can be swapped after the spike.
pub trait ScriptHost {
    fn set_limits(&mut self, limits: ScriptLimits); // heap bytes, GC threshold, max run time
    fn run_script(&mut self, source: ScriptSource, document: &mut DocumentMutator);
    fn dispatch_event(&mut self, target: NodeId, event: &DomEvent) -> DefaultAction;
    fn run_timers_and_microtasks(&mut self, now: Instant);
    fn interrupt(&self);
    fn memory_usage(&self) -> usize;
}
```

- **One DOM.** `blitz-dom` owns all nodes. JS wrappers hold a generational node
  id and are cached per runtime, so identity (`===`) holds and a removed node
  can be collected by both sides.
- **Bindings are generated.** A WebIDL-to-Rust generator targets the
  `ScriptHost` API, as NetSurf does with nsgenbind. NetSurf still lists about
  1,540 unimplemented bindings after a decade of hand work; generation and
  priorities matter more than the VM.
- **Async networking.** `blitz-vibey-script` fetches synchronously; the Wega
  engine uses an async `NetProvider` on Wega's own stack, sharing cookies and
  the download manager.
- **Use, don't fork, where possible.** Depend on published Blitz crates and
  send fixes upstream. Copy `blitz-vibey-script` patterns (MIT/Apache-2.0
  allows it, with attribution) rather than depending on an unpublished crate.

### Memory design

Targets to verify in the spikes, not claims:

| Page type | Servo today | Wega engine target |
| --- | --- | --- |
| Article or documentation page | measure in phase 0 | under 100 MB |
| Interactive app page | about 500 MB observed | under 250 MB |
| Frozen background tab | most of its live memory | under 10 MB |

Techniques built into the engine:

- **Hard per-tab limits:** one V8 isolate per tab with
  a page heap budget, an OOM callback and watchdog termination, so a heavy
  page is stopped and reported instead of taking the container down.
- **Freeze and discard:** background tabs drop paint surfaces, decoded images
  and layout caches; long-idle tabs are serialized and discarded (extending the
  rules in `src/lifecycle.rs`).
- **Images at display size,** decoded lazily and dropped when off-screen.
- **Compact DOM:** measure `blitz-dom`'s per-node cost (estimated at roughly
  1–1.5 KB per laid-out element, mostly Taffy layout caches) and shrink the
  largest parts, such as caches only kept for elements that change.
- **Bytecode caching** of scripts across tabs and restarts.
- **Content blocking** of ads and trackers, often the largest single saving on
  news sites.
- **Allocator purge** after a tab closes, so freed memory goes back to the OS.

Servo-side quick wins come first (phase 0) and help measure: an `about:memory`
page from `servo.create_memory_report` (already used in `src/toolbox.rs`),
trying `servo/multiprocess` so closing a tab returns memory, freeing background
tab surfaces, an option to turn off the JIT, and memory-pressure tab
discarding.

## Phases

| Phase | Work | Exit criteria |
| --- | --- | --- |
| 0. Measure | `about:memory`; Servo quick wins above; a reference-site memory benchmark | numbers for 10 reference pages; known split between JS heap, DOM/style/layout, images and surfaces |
| 1. Boundary | New ADR; `PageEngine`/`PageView` with a Servo adapter; no behaviour change | all native suites pass on the Servo adapter |
| 2. Static spike | `WegaView` on `blitz-dom` without JS, behind a developer flag | reference article pages render; memory and CPU measured against Servo |
| 3. JS runtime | V8 through `deno_core`, with six offline framework fixtures and a DOM WPT subset | V8 selected; compatibility and memory measurements remain separate gates |
| 4. First release | per-site engine choice, engine badge in the address bar, *Open in full engine*, automatic fallback on unsupported features | the Wega engine is the default for an allowlist of sites |
| 5. Grow | WebIDL generator, async fetch, XHR, storage, `MutationObserver`; WPT subset in CI; compatibility dashboard | sites move from the allowlist to default-on as tests pass |

Reference pages for every measurement: a Wikipedia article, Old Reddit, Hacker
News, an MDN page, a docs.rs page, a GitHub repository page, a news site,
Preact TodoMVC, Google search and YouTube (the last two expected to stay on
Servo for a long time).

## Risks

- **Web API breadth is the real cost,** not the VM or layout. Without
  prioritization by real sites, the engine stays a demo.
- **Script-heavy sites:** V8 supplies a JIT, but DOM/Web API gaps can still
  require Servo.
- **Two engines to maintain** until the Wega engine is the default; the
  boundary keeps that cost contained.
- **Security of a new engine:** parsing and scripting untrusted content. One
  process per tab and the existing Docker isolation are the mitigations; a
  sandbox is a later phase.
- **Upstream pace:** Blitz is beta and changes quickly; pin versions and
  upstream fixes.

## Licensing

Blitz is `MIT OR Apache-2.0`; Stylo and `stylo_taffy` are MPL-2.0; V8 is
BSD-3-Clause, and rusty_v8/Deno crates are MIT. These are compatible with Wega's
MPL-2.0.
Copied Blitz code keeps its notices; [licensing.md](licensing.md) and the
third-party notices cover the new dependencies.

## Open decisions

1. The engine's name and whether it lives in this repository or its own.
2. JavaScript runtime decision resolved: V8 through deno_core; QuickJS removed
   on 26 October 2026.
3. How the per-site engine choice looks to users: automatic, visible badge,
   manual switch.
4. GPU path for the Wega engine: Skia on the existing GL context, or move the
   chrome to wgpu.
5. Whether to also contribute memory fixes to Servo while it remains the
   fallback.

## Sources

- Servo 0.7 crates: `servo-script`, `servo-script-bindings`, `servo-layout-api`,
  `servo-config` (`prefs.rs` GC and JIT preferences).
- Blitz at commit `1c5265a` (23 October 2026): `blitz-dom/src/document.rs`,
  `node/node.rs`, `layout/mod.rs`, `blitz-paint/src/lib.rs`,
  `blitz-traits/src/net.rs`, `blitz-vibey-script/`.
- Projects on Blitz: <https://github.com/DioxusLabs/blitz/issues/613>, kiln
  <https://github.com/marshallshelly/kiln>.
- JS conformance: <https://test262.fyi>; `rquickjs` limits in
  `core/src/runtime/base.rs`; Boa limits in `core/engine/src/vm/runtime_limits.rs`.
- NetSurf JavaScript bindings: `docs/jsbinding.md`, `docs/UnimplementedJavascript.md`.


## Which engine should we use now?

The [25 October reliability baseline](reliability-baseline.md) now records
repeatable compatibility probes, optimized process-tree RSS/CPU measurements,
live reference-page captures and refreshed release-validation evidence. On its
fixed article workload Wega uses more summed RSS as tabs increase; these results
do not justify changing the default engine.

V8 through deno_core is mandatory; QuickJS has been removed. The combined
performance, CSS and rollout implementation and its remaining gates are in
[p4-p6-iteration.md](p4-p6-iteration.md).

**Use Servo as the default for everyday browsing.** It remains the supported
choice for interactive sites, website sign-in, forms, media, developer tools and
website accessibility. It is a compatibility fallback, not a guarantee that every
site or browser API works.

**Choose Wega (experimental) for testing article/static pages and the initial
JavaScript integration.** Use the address-bar badge, bottom-right selector or `--engine wega`, and
switch back to Servo if the page needs unsupported APIs. Outside the three exact HTTPS example domains, Wega remains opt-in. Per-origin
preferences take priority; the automatic allowlist and compatibility fallback can
be disabled in the engine menu. A broader default must wait for compatibility
and whole-process memory/CPU evidence. A separate worker and a small JS heap alone do not establish
that the new engine uses less total memory on real sites.
