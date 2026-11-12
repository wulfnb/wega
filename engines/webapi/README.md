# engines/webapi — Web APIs

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`core`](./core/README.md) — Shared Web API infrastructure.
- [`fetch`](./fetch/README.md) — Fetch API, Request/Response and CORS semantics.
- [`timers`](./timers/README.md) — Timers and scheduling APIs.
- [`storage`](./storage/README.md) — Web Storage, IndexedDB and Cache API surfaces.
- [`workers`](./workers/README.md) — Dedicated, shared and service worker APIs.

Default dependencies for this group: `foundation/*`, `javascript/bindings`, `services/*` through traits.

Dependencies to avoid: `graphics/*` internals, `layout/*` internals, `adapters/*` directly.
