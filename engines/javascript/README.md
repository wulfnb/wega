# engines/javascript — JavaScript

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`runtime`](./runtime/README.md) — Engine-neutral JavaScript runtime abstraction.
- [`dom-bindings`](./dom-bindings/README.md) — JavaScript bindings for DOM interfaces.
- [`events`](./events/README.md) — JavaScript-facing event and callback plumbing.
- [`bindings`](./bindings/README.md) — Generated and hand-written binding infrastructure.
- [`webidl`](./webidl/README.md) — WebIDL parsing and code generation.

Default dependencies for this group: `foundation/*`, `dom/core` (through bindings leaves only).

Dependencies to avoid: Direct use of V8 types outside `adapters/v8`, `graphics/*`, `services/*`.
