# engines/css — CSS

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`values`](./values/README.md) — CSS value types, units and computed-value representations.
- [`parser`](./parser/README.md) — CSS tokenizer and stylesheet/declaration parser.
- [`selectors`](./selectors/README.md) — CSS selector parsing and matching.
- [`style-engine`](./style-engine/README.md) — Cascade, inheritance and computed style resolution.
- [`cache`](./cache/README.md) — Style sharing and style-related caches.
- [`invalidation`](./invalidation/README.md) — Style invalidation from DOM and stylesheet changes.

Default dependencies for this group: `std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*` or `layout/*`; element access goes through an abstract view that an adapter implements.

Dependencies to avoid: `dom/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (Stylo is reached only through a future `adapters/stylo`); Blitz, V8.
