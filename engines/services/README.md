# engines/services — Services

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`http`](./http/README.md) — HTTP client service.
- [`cache`](./cache/README.md) — HTTP and resource cache service.
- [`cookies`](./cookies/README.md) — Cookie jar and cookie policy.
- [`downloads`](./downloads/README.md) — Download management.
- [`security`](./security/README.md) — Security policy enforcement.
- [`permissions`](./permissions/README.md) — Permission model and prompts.
- [`navigation`](./navigation/README.md) — Navigation controller.
- [`history`](./history/README.md) — Session history.
- [`bookmarks`](./bookmarks/README.md) — Bookmark storage and organization.

Default dependencies for this group: `foundation/*`; platform and network crates in `engines/platform` and `engines/network` through narrow traits.

Dependencies to avoid: `dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`.
