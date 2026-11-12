# Wega Engine Module Map

**Status: PLANNED — NOT IMPLEMENTED** (Phase 1: directory structure and documentation only)

This map describes the planned modular layout of the Wega browser engine. Nothing listed here under a new directory exists as code yet. No new directory is a Cargo workspace member, and none contains a `Cargo.toml` or Rust source.

## Existing components (unchanged)

| Path | Role |
|---|---|
| `engines/wega` | Current engine crate |
| `engines/network` | Network stack |
| `engines/platform` | Platform abstraction |
| `engines/protocol` | Process/IPC protocol |
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
│   ├── storage/
│   ├── workers/
├── services/
│   ├── http/
│   ├── cache/
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

Default dependencies: `foundation/*`.

Avoid: `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, V8.

| Module | Purpose |
|---|---|
| [`core`](../../engines/dom/core/README.md) | DOM tree data model (nodes, documents, elements, attributes). |
| [`events`](../../engines/dom/events/README.md) | DOM event model (dispatch, propagation, listeners). |
| [`mutation`](../../engines/dom/mutation/README.md) | DOM mutation tracking and observation. |
| [`traversal`](../../engines/dom/traversal/README.md) | DOM traversal and querying utilities. |

### HTML (`engines/html`)

Default dependencies: `foundation/*`, `dom/core`.

Avoid: `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`.

| Module | Purpose |
|---|---|
| [`parser`](../../engines/html/parser/README.md) | HTML tokenizer and input-stream handling. |
| [`treebuilder`](../../engines/html/treebuilder/README.md) | HTML tree construction (insertion modes, fragment parsing). |

### CSS (`engines/css`)

Default dependencies: `foundation/*`, `dom/core` where selectors and style need the tree.

Avoid: `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (including Stylo directly), Blitz, V8.

| Module | Purpose |
|---|---|
| [`values`](../../engines/css/values/README.md) | CSS value types, units and computed-value representations. |
| [`parser`](../../engines/css/parser/README.md) | CSS tokenizer and stylesheet/declaration parser. |
| [`selectors`](../../engines/css/selectors/README.md) | CSS selector parsing and matching. |
| [`style-engine`](../../engines/css/style-engine/README.md) | Cascade, inheritance and computed style resolution. |
| [`cache`](../../engines/css/cache/README.md) | Style sharing and style-related caches. |
| [`invalidation`](../../engines/css/invalidation/README.md) | Style invalidation from DOM and stylesheet changes. |

### Layout (`engines/layout`)

Default dependencies: `foundation/*`, `css/values`, `css/style-engine` (computed style input), `dom/core`.

Avoid: `graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, Taffy-specific types in public APIs, V8.

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

Default dependencies: `foundation/*`, `foundation/geometry`; `layout/core` fragments only for display-list building.

Avoid: `dom/*`, `css/*`, `javascript/*`, `services/*`, direct Skia/GPU types outside the dedicated leaves.

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
| [`storage`](../../engines/webapi/storage/README.md) | Web Storage, IndexedDB and Cache API surfaces. |
| [`workers`](../../engines/webapi/workers/README.md) | Dedicated, shared and service worker APIs. |

### Services (`engines/services`)

Default dependencies: `foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits.

Avoid: `dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`.

| Module | Purpose |
|---|---|
| [`http`](../../engines/services/http/README.md) | HTTP client service. |
| [`cache`](../../engines/services/cache/README.md) | HTTP and resource cache service. |
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

## Layering rules (target)

1. `foundation` depends on nothing else in the engine.
2. `dom`, `html`, `css`, `layout`, `graphics` form a one-directional pipeline: `html → dom → css → layout → graphics`; later stages never import earlier-stage internals in reverse.
3. `javascript` and `webapi` sit above `dom` and reach services only through traits.
4. `services` are independent of the rendering pipeline.
5. `runtime` orchestrates everything and is depended on by nothing in the lower layers.
6. Third-party engines (Blitz, Stylo, Skia, V8) are only referenced from `adapters/*`.
7. `tests` and `tools` are never dependencies of production crates.

These rules are intended to be enforced later by `engines/tools/architecture-check`.

## Naming notes

- Planned `engines/tests` and `engines/tools` are distinct from the existing root `tests/` and `tools/` directories.
- Planned `services/http` overlaps conceptually with the existing `engines/network`; the latter is expected to be consumed, not replaced.
- Planned `runtime/process` overlaps with `engines/protocol` and `engines/platform`.
- Planned `engines/adapters/*` overlaps with code under `engines/vendor/*`.
- `docs/architecture.md` (existing file) coexists with the new `docs/architecture/` directory.
