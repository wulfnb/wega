# engines/dom — DOM

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`core`](./core/README.md) — DOM tree data model (nodes, documents, elements, attributes).
- [`events`](./events/README.md) — DOM event model (dispatch, propagation, listeners).
- [`mutation`](./mutation/README.md) — DOM mutation tracking and observation.
- [`traversal`](./traversal/README.md) — DOM traversal and querying utilities.

Default dependencies for this group: `foundation/*`.

Dependencies to avoid: `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`, Blitz, V8.
