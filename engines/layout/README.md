# engines/layout — Layout

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`core`](./core/README.md) — Layout tree, constraints and fragment output shared by all formatting contexts.
- [`block`](./block/README.md) — Block formatting context layout.
- [`flex`](./flex/README.md) — Flexbox layout.
- [`grid`](./grid/README.md) — CSS Grid layout.
- [`inline`](./inline/README.md) — Inline formatting context layout.
- [`text`](./text/README.md) — Text shaping, measurement and line breaking.
- [`invalidation`](./invalidation/README.md) — Layout invalidation and incremental relayout.

Default dependencies for this group: `foundation/*`, `css/values`, `css/style-engine` (computed style input), `dom/core`.

Dependencies to avoid: `graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, Taffy-specific types in public APIs, V8.
