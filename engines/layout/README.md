# engines/layout — Layout

**Status: IMPLEMENTED** (except `invalidation`)

Group directory. Crates are layered with no cycles: `core` → `text` → `inline` → `block` → `flex`/`grid`/`table` → `engine` (`wega-layout`, the entry point the rest of the engine depends on).

- [`core`](./core/README.md) — Layout tree, constraints and fragment output shared by all formatting contexts.
- [`block`](./block/README.md) — Block formatting context layout.
- [`flex`](./flex/README.md) — Flexbox layout.
- [`grid`](./grid/README.md) — CSS Grid layout.
- [`inline`](./inline/README.md) — Inline formatting context layout and intrinsic sizing.
- [`text`](./text/README.md) — Text shaping, measurement and line breaking.
- [`table`](./table/README.md) — Table layout.
- [`engine`](./engine/README.md) — Layout entry point; wires the formatting contexts together (`wega-layout`).
- [`invalidation`](./invalidation/README.md) — Layout invalidation and incremental relayout (planned).

Default dependencies for this group: `std` only for now (`foundation/*` deferred). Independent contract with plain input/output types: no dependency on `css/*`, `dom/*` or `graphics/*`; an adapter converts style and DOM data (precedent: `stylo_taffy` inside Blitz).

Dependencies to avoid: `css/*`, `dom/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz, Taffy or Parley types in public APIs.
