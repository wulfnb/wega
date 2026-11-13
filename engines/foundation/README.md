# engines/foundation — Foundation

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`types`](./types/README.md) — Shared primitive and newtype definitions used across the engine.
- [`ids`](./ids/README.md) — Strongly typed identifiers for engine entities.
- [`errors`](./errors/README.md) — Common error types and error-reporting conventions.
- [`geometry`](./geometry/README.md) — Geometry primitives (points, sizes, rects, transforms).
- [`config`](./config/README.md) — Engine configuration schema and feature flags.
- [`events`](./events/README.md) — Engine-internal event and message primitives.
- [`metrics`](./metrics/README.md) — Metrics, tracing and profiling hooks.

Default dependencies for this group: Rust `std` and small, widely used utility crates only.

Dependencies to avoid: Any other engine group, `adapters/*`, Blitz, Stylo, Skia, V8, Servo, UI toolkits, OS APIs.
