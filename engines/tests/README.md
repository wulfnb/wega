# engines/tests — Tests

**Status: PLANNED — NOT IMPLEMENTED**

Group directory. Planned leaf modules:

- [`fixtures`](./fixtures/README.md) — Shared test inputs (HTML, CSS, JS, expected outputs).
- [`integration`](./integration/README.md) — Cross-subsystem integration tests.
- [`compatibility`](./compatibility/README.md) — Web-platform compatibility suites.
- [`performance`](./performance/README.md) — Performance and regression benchmarks.

Default dependencies for this group: Public APIs of the crates under test.

Dependencies to avoid: Reaching into private internals; production crates depending on test crates.
