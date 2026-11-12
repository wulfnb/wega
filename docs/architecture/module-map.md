# Wega Engine Module Map

**Status: PARTIALLY IMPLEMENTED** (Wave 1 crates exist; all other leaves are placeholders; Wave 2 decisions in [`wave2-gate-report.md`](./wave2-gate-report.md)). 72 planned leaf modules. Per-module classification, package names and migration sources: [`crate-initialization-plan.md`](./crate-initialization-plan.md).

This map describes the modular layout of the Wega browser engine. **Implemented (Wave 1):** `services/cookies` (`wega-cookies`), `services/storage` (`wega-storage`), `services/security` (`wega-security`), `services/permissions` (`wega-permissions`) are members of the `engines` workspace; `tools/dependency-graph` and `tools/architecture-check` are members of the standalone `engines/tools` workspace. `adapters/skia` (`wega-adapter-skia`, raster only) and `adapters/v8` (`wega-adapter-v8`) are implemented; the Wega-owned document view and mutation contract is `dom/core` (`wega-dom`), the incremental HTML parser is `html/parser` (`wega-html`) and the Blitz mapping is `adapters/blitz` (`wega-adapter-blitz`, partial: document, nodes and mutation only) (see [`dom-round7-report.md`](./dom-round7-report.md), [`dom-mutation-report.md`](./dom-mutation-report.md) and [`dom-migration-report.md`](./dom-migration-report.md)), all members of the engines workspace (see [`architecture-refactor-progress.md`](./architecture-refactor-progress.md)). **Every other leaf is still a documentation-only placeholder** with no `Cargo.toml` or Rust source and is not a workspace member.

## Existing components (unchanged)

| Path | Role |
|---|---|
| `engines/wega` | Current engine crate |
| `engines/network` | Network stack |
| `engines/platform` | Platform abstraction |
| `engines/protocol` | Process/IPC protocol |
| `engines/services/{cookies,storage,security,permissions}` | Wave 1 crates (`wega-cookies`, `wega-storage`, `wega-security`, `wega-permissions`) |
| `engines/tools` | Standalone dev-tools workspace (`wega-dependency-graph`, `wega-architecture-check`), excluded from `engines` |
| `engines/vendor` | Vendored third-party code (Blitz, Stylo, Skia bindings, Deno/V8) |
| `engines/v8-spike` | V8 experiment (excluded from workspace) |
| `src/`, `tests/`, `tools/`, `vendor/` | Root `wega` application crate and its tests, tools and vendored Servo crates |

## Planned layout under `engines/`

```
engines/
├── foundation/
│   ├── types/
│   ├── ids/
│   ├── errors/
│   ├── geometry/
│   ├── config/
│   ├── events/
│   ├── metrics/
├── dom/
│   ├── core/
│   ├── events/
│   ├── mutation/
│   ├── traversal/
├── html/
│   ├── parser/
│   ├── treebuilder/
├── css/
│   ├── values/
│   ├── parser/
│   ├── selectors/
│   ├── style-engine/
│   ├── cache/
│   ├── invalidation/
├── layout/
│   ├── core/
│   ├── block/
│   ├── flex/
│   ├── grid/
│   ├── inline/
│   ├── text/
│   ├── invalidation/
├── graphics/
│   ├── display-list/
│   ├── paint/
│   ├── raster-skia/
│   ├── compositor/
│   ├── gpu/
│   ├── image/
│   ├── fonts/
├── javascript/
│   ├── runtime/
│   ├── dom-bindings/
│   ├── events/
│   ├── bindings/
│   ├── webidl/
├── webapi/
│   ├── core/
│   ├── fetch/
│   ├── timers/
│   ├── canvas/
│   ├── webgl/
│   ├── media/
│   ├── storage/
│   ├── workers/
├── services/
│   ├── http/
│   ├── storage/
│   ├── cookies/
│   ├── downloads/
│   ├── security/
│   ├── permissions/
│   ├── navigation/
│   ├── history/
│   ├── bookmarks/
├── runtime/
│   ├── tab-manager/
│   ├── process/
│   ├── scheduler/
│   ├── memory/
│   ├── orchestration/
├── adapters/
│   ├── blitz/
│   ├── stylo/
│   ├── skia/
│   ├── v8/
├── tests/
│   ├── fixtures/
│   ├── integration/
│   ├── compatibility/
│   ├── performance/
├── tools/
│   ├── dependency-graph/
│   ├── build-benchmark/
│   ├── memory-benchmark/
│   ├── architecture-check/
├── wega/ network/ platform/ protocol/ vendor/ v8-spike/   (existing)
```

## Groups

### Foundation (`engines/foundation`)

Default dependencies: Rust `std` and small, widely used utility crates only.

Avoid: Any other engine group, `adapters/*`, Blitz, Stylo, Skia, V8, Servo, UI toolkits, OS APIs.

| Module | Purpose |
|---|---|
| [`types`](../../engines/foundation/types/README.md) | Shared primitive and newtype definitions used across the engine. |
| [`ids`](../../engines/foundation/ids/README.md) | Strongly typed identifiers for engine entities. |
| [`errors`](../../engines/foundation/errors/README.md) | Common error types and error-reporting conventions. |
| [`geometry`](../../engines/foundation/geometry/README.md) | Geometry primitives (points, sizes, rects, transforms). |
| [`config`](../../engines/foundation/config/README.md) | Engine configuration schema and feature flags. |
| [`events`](../../engines/foundation/events/README.md) | Engine-internal event and message primitives. |
| [`metrics`](../../engines/foundation/metrics/README.md) | Metrics, tracing and profiling hooks. |

### DOM (`engines/dom`)

Default dependencies: `std` only for now (`foundation/*` deferred). Independent contract: no dependency on `html/*`, `css/*`, `layout/*` or `graphics/*`.

Avoid: `html/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; third-party engines (Blitz, V8) belong only in future `adapters/*`.

| Module | Purpose |
|---|---|
| [`core`](../../engines/dom/core/README.md) | DOM tree data model (nodes, documents, elements, attributes). |
| [`events`](../../engines/dom/events/README.md) | DOM event model (dispatch, propagation, listeners). |
| [`mutation`](../../engines/dom/mutation/README.md) | DOM mutation tracking and observation. |
| [`traversal`](../../engines/dom/traversal/README.md) | DOM traversal and querying utilities. |

### HTML (`engines/html`)

Default dependencies: `std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*`, `css/*` or `layout/*`; tree construction targets a sink trait that an adapter implements.

Avoid: `dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz types in public APIs.

| Module | Purpose |
|---|---|
| [`parser`](../../engines/html/parser/README.md) | HTML tokenizer and input-stream handling. |
| [`treebuilder`](../../engines/html/treebuilder/README.md) | HTML tree construction (insertion modes, fragment parsing). |

### CSS (`engines/css`)

Default dependencies: `std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements.

Avoid: `dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8.

| Module | Purpose |
|---|---|
| [`values`](../../engines/css/values/README.md) | CSS value types, units and computed-value representations. |
| [`parser`](../../engines/css/parser/README.md) | CSS tokenizer and stylesheet/declaration parser. |
| [`selectors`](../../engines/css/selectors/README.md) | CSS selector parsing and matching. |
| [`style-engine`](../../engines/css/style-engine/README.md) | Cascade, inheritance and computed style resolution. |
| [`cache`](../../engines/css/cache/README.md) | Style sharing and style-related caches. |
| [`invalidation`](../../engines/css/invalidation/README.md) | Style invalidation from DOM and stylesheet changes. |

### Layout (`engines/layout`)

Default dependencies: `std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz).

Avoid: `css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.

| Module | Purpose |
|---|---|
| [`core`](../../engines/layout/core/README.md) | Layout tree, constraints and fragment output shared by all formatting contexts. |
| [`block`](../../engines/layout/block/README.md) | Block formatting context layout. |
| [`flex`](../../engines/layout/flex/README.md) | Flexbox layout. |
| [`grid`](../../engines/layout/grid/README.md) | CSS Grid layout. |
| [`inline`](../../engines/layout/inline/README.md) | Inline formatting context layout. |
| [`text`](../../engines/layout/text/README.md) | Text shaping, measurement and line breaking. |
| [`invalidation`](../../engines/layout/invalidation/README.md) | Layout invalidation and incremental relayout. |

### Graphics (`engines/graphics`)

Default dependencies: `std` only for now (`foundation/*` deferred). The display-list contract is independent of `layout/*`, `dom/*` and `css/*` (it may mirror `anyrender::PaintScene`).

Avoid: `dom/*`, `css/*`, `layout/*`, `javascript/*`, `services/*`; Skia/GPU types outside `adapters/skia` and the dedicated GPU leaf.

| Module | Purpose |
|---|---|
| [`display-list`](../../engines/graphics/display-list/README.md) | Backend-neutral display list representation. |
| [`paint`](../../engines/graphics/paint/README.md) | Paint-tree traversal producing display lists. |
| [`raster-skia`](../../engines/graphics/raster-skia/README.md) | Skia-based rasterization of display lists. |
| [`compositor`](../../engines/graphics/compositor/README.md) | Layer tree and frame compositing. |
| [`gpu`](../../engines/graphics/gpu/README.md) | GPU abstraction and resource management. |
| [`image`](../../engines/graphics/image/README.md) | Image decoding, resizing and caching. |
| [`fonts`](../../engines/graphics/fonts/README.md) | Font discovery, loading and glyph data. |

### JavaScript (`engines/javascript`)

Default dependencies: `foundation/*`, `dom/core` (through bindings leaves only).

Avoid: Direct use of V8 types outside `adapters/v8`, `graphics/*`, `services/*`.

| Module | Purpose |
|---|---|
| [`runtime`](../../engines/javascript/runtime/README.md) | Engine-neutral JavaScript runtime abstraction. |
| [`dom-bindings`](../../engines/javascript/dom-bindings/README.md) | JavaScript bindings for DOM interfaces. |
| [`events`](../../engines/javascript/events/README.md) | JavaScript-facing event and callback plumbing. |
| [`bindings`](../../engines/javascript/bindings/README.md) | Generated and hand-written binding infrastructure. |
| [`webidl`](../../engines/javascript/webidl/README.md) | WebIDL parsing and code generation. |

### Web APIs (`engines/webapi`)

Default dependencies: `foundation/*`, `javascript/bindings`, `services/*` through traits.

Avoid: `graphics/*` internals, `layout/*` internals, `adapters/*` directly.

| Module | Purpose |
|---|---|
| [`core`](../../engines/webapi/core/README.md) | Shared Web API infrastructure. |
| [`fetch`](../../engines/webapi/fetch/README.md) | Fetch API, Request/Response and CORS semantics. |
| [`timers`](../../engines/webapi/timers/README.md) | Timers and scheduling APIs. |
| [`canvas`](../../engines/webapi/canvas/README.md) | Canvas 2D API semantics (Skia stays in `adapters/skia`). |
| [`webgl`](../../engines/webapi/webgl/README.md) | WebGL API semantics and resources. |
| [`media`](../../engines/webapi/media/README.md) | Media API semantics. |
| [`storage`](../../engines/webapi/storage/README.md) | Web Storage, IndexedDB and Cache API surfaces. |
| [`workers`](../../engines/webapi/workers/README.md) | Dedicated, shared and service worker APIs. |

### Services (`engines/services`)

Default dependencies: `foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits.

Avoid: `dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`.

| Module | Purpose |
|---|---|
| [`http`](../../engines/services/http/README.md) | HTTP client service. |
| [`storage`](../../engines/services/storage/README.md) | Persistent origin storage (Cache Storage, Cache API, IndexedDB). HTTP caching stays in `http`. |
| [`cookies`](../../engines/services/cookies/README.md) | Cookie jar and cookie policy. |
| [`downloads`](../../engines/services/downloads/README.md) | Download management. |
| [`security`](../../engines/services/security/README.md) | Security policy enforcement. |
| [`permissions`](../../engines/services/permissions/README.md) | Permission model and prompts. |
| [`navigation`](../../engines/services/navigation/README.md) | Navigation controller. |
| [`history`](../../engines/services/history/README.md) | Session history. |
| [`bookmarks`](../../engines/services/bookmarks/README.md) | Bookmark storage and organization. |

### Runtime (`engines/runtime`)

Default dependencies: `foundation/*`, `services/*`; may orchestrate all other groups through traits.

Avoid: Being imported by lower-level groups (`dom`, `css`, `layout`, `graphics`, `html`).

| Module | Purpose |
|---|---|
| [`tab-manager`](../../engines/runtime/tab-manager/README.md) | Tab lifecycle and movable-tab management. |
| [`process`](../../engines/runtime/process/README.md) | Process model and sandbox supervision. |
| [`scheduler`](../../engines/runtime/scheduler/README.md) | Task and frame scheduling. |
| [`memory`](../../engines/runtime/memory/README.md) | Memory budgeting and pressure handling. |
| [`orchestration`](../../engines/runtime/orchestration/README.md) | Top-level engine assembly and pipeline orchestration. |

### Adapters (`engines/adapters`)

Default dependencies: The wrapped third-party engine plus the Wega-owned trait it implements.

Avoid: Leaking third-party types into non-adapter crates; reverse dependencies from the wrapped library into Wega internals.

| Module | Purpose |
|---|---|
| [`blitz`](../../engines/adapters/blitz/README.md) | Adapter around the vendored Blitz components (`engines/vendor/blitz`). |
| [`stylo`](../../engines/adapters/stylo/README.md) | Adapter around Stylo for CSS style resolution. |
| [`skia`](../../engines/adapters/skia/README.md) | Adapter around Skia bindings. |
| [`v8`](../../engines/adapters/v8/README.md) | Adapter around V8 (via vendored Deno `deno_core`/`deno_v8`). |

### Tests (`engines/tests`)

Default dependencies: Public APIs of the crates under test.

Avoid: Reaching into private internals; production crates depending on test crates.

| Module | Purpose |
|---|---|
| [`fixtures`](../../engines/tests/fixtures/README.md) | Shared test inputs (HTML, CSS, JS, expected outputs). |
| [`integration`](../../engines/tests/integration/README.md) | Cross-subsystem integration tests. |
| [`compatibility`](../../engines/tests/compatibility/README.md) | Web-platform compatibility suites. |
| [`performance`](../../engines/tests/performance/README.md) | Performance and regression benchmarks. |

### Tools (`engines/tools`)

Default dependencies: Standard library, `cargo metadata` output, and file-system access.

Avoid: Being depended on by production engine crates.

| Module | Purpose |
|---|---|
| [`dependency-graph`](../../engines/tools/dependency-graph/README.md) | Generate and visualize the crate dependency graph. |
| [`build-benchmark`](../../engines/tools/build-benchmark/README.md) | Measure and track build times. |
| [`memory-benchmark`](../../engines/tools/memory-benchmark/README.md) | Measure and track memory use. |
| [`architecture-check`](../../engines/tools/architecture-check/README.md) | Enforce architectural dependency rules. |

## Third-party engines: current state vs adapter-only target

Today `wega-engine` depends **directly** on third-party engines; the adapter-only arrangement is a *future target*, not the current state.

| Engine | Current (direct deps of `wega-engine`) | Future target |
|---|---|---|
| Blitz (`blitz-dom`, `blitz-html`, `blitz-paint`, `blitz-traits`) | `engines/wega/src/{dom,view,parser,script,raster}.rs` | only in `adapters/blitz` |
| Stylo (`style`) | `dom.rs` (media-query parsing) | only in `adapters/stylo` |
| Skia (`skia-safe`, `anyrender_skia`, `anyrender`) | `raster.rs`, `canvas/*`, `webgl/context.rs` | only in `adapters/skia` |
| V8 / Deno (`deno_core`, `deno_*`) | `v8.rs`, `native_ops.rs`, `script.rs`, `module_loader.rs`, `web*.rs`, `workers/runtime.rs` | only in `adapters/v8` |
| `html5ever` | `parser.rs` | `html/parser` or an adapter (decided at gate G2-HTML) |

The independent-contract rule (`html`, `dom`, `css`, `layout`, `graphics/display-list` do not depend on one another) applies to **planned** crates; the current monolith does not satisfy it and is not expected to until Waves 2–3 complete.

## Layering rules (target, updated for Phase 2)

1. Only `wega-engine` (`runtime/orchestration`) may depend on adapters (composition root).
2. `html`, `dom`, `css`, `layout` and `graphics/display-list` are **independent contracts** with no dependencies on one another. Integration happens in `adapters/*` (e.g. a Stylo adapter depends on both style and DOM contracts). The earlier draft chain `html → dom → css → layout → graphics` is withdrawn.
3. A contract crate exists only if it passes its validation gate against existing code; otherwise the module stays documentation-only.
4. Third-party engines (Blitz, Stylo, Skia, V8) appear only in `adapters/*`. `wega-adapter-skia` is the sole Skia backend owner; Canvas API semantics live in `webapi/canvas`.
5. `services/*` are independent of the rendering pipeline. `services/storage` does not depend on `services/http`; HTTP caching stays in `services/http`.
6. `wega-platform` is the OS process/IPC implementation and `wega-protocol` the wire contract; `runtime/process` is documentation-only.
7. `foundation/*` crates are deferred until stable interfaces have multiple real consumers.
8. New crates use `wega-adapter-*` / `wega-js-*` naming where applicable and default to `#![forbid(unsafe_code)]`.
9. `tests` and `tools` are never dependencies of production crates; `cargo_metadata` is permitted only in development tools.

These rules are intended to be enforced by `engines/tools/architecture-check` (report-only until Phase 9).

## Naming notes

- `engines/tests` and `engines/tools` are distinct from the root `tests/` and `tools/` directories.
- `services/http` **is** the existing `engines/network` crate (`wega-network`).
- `runtime/process` is documentation-only; `engines/platform` and `engines/protocol` keep their roles.
- `adapters/*` hold Wega-owned wrapper code; `engines/vendor/*` holds patched third-party source and is never a migration target.
- `graphics/raster-skia` is documentation-only and merged into `adapters/skia`.
- `javascript/webidl` keeps its existing location (`tools/webidl`).
- `docs/architecture.md` (existing file) coexists with the `docs/architecture/` directory.
