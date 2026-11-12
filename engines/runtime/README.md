# engines/runtime — Runtime

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`tab-manager`](./tab-manager/README.md) — Tab lifecycle and movable-tab management.
- [`process`](./process/README.md) — Process model and sandbox supervision.
- [`scheduler`](./scheduler/README.md) — Task and frame scheduling.
- [`memory`](./memory/README.md) — Memory budgeting and pressure handling.
- [`orchestration`](./orchestration/README.md) — Top-level engine assembly and pipeline orchestration.

Default dependencies for this group: `foundation/*`, `services/*`; may orchestrate all other groups through traits.

Dependencies to avoid: Being imported by lower-level groups (`dom`, `css`, `layout`, `graphics`, `html`).
