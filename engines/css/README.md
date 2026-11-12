# engines/css — CSS

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`values`](./values/README.md) — CSS value types, units and computed-value representations.
- [`parser`](./parser/README.md) — CSS tokenizer and stylesheet/declaration parser.
- [`selectors`](./selectors/README.md) — CSS selector parsing and matching.
- [`style-engine`](./style-engine/README.md) — Cascade, inheritance and computed style resolution.
- [`cache`](./cache/README.md) — Style sharing and style-related caches.
- [`invalidation`](./invalidation/README.md) — Style invalidation from DOM and stylesheet changes.

Default dependencies for this group: `foundation/*`, `dom/core` where selectors and style need the tree.

Dependencies to avoid: `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*` (including Stylo directly), Blitz, V8.
