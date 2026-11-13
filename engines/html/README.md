# engines/html — HTML

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`parser`](./parser/README.md) — HTML tokenizer and input-stream handling.
- [`treebuilder`](./treebuilder/README.md) — HTML tree construction (insertion modes, fragment parsing).

Default dependencies for this group: `std` only for now (`foundation/*` deferred). Independent contract: no dependency on `dom/*`, `css/*` or `layout/*`; tree construction targets a sink trait that an adapter implements.

Dependencies to avoid: `dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `services/*`, `adapters/*`; Blitz types in public APIs.
