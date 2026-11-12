# engines/adapters — Adapters

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`blitz`](./blitz/README.md) — Adapter around the vendored Blitz components (`engines/vendor/blitz`).
- [`stylo`](./stylo/README.md) — Adapter around Stylo for CSS style resolution.
- [`skia`](./skia/README.md) — Adapter around Skia bindings.
- [`v8`](./v8/README.md) — Adapter around V8 (via vendored Deno `deno_core`/`deno_v8`).

Default dependencies for this group: The wrapped third-party engine plus the Wega-owned trait it implements.

Dependencies to avoid: Leaking third-party types into non-adapter crates; reverse dependencies from the wrapped library into Wega internals.
