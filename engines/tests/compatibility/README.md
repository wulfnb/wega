# tests/compatibility

**Status: PARTIAL** — `wpt-runner` is implemented.

## Purpose

Web-platform compatibility suites.

## Intended responsibilities

- WPT-style and reftest runners
- Compatibility scoreboard inputs

## Expected dependencies

Public APIs of the crates under test. Specifically: `tests/fixtures`.

## Dependencies to avoid

Reaching into private internals; production crates depending on test crates.

## Current status

- [`wpt-runner`](./wpt-runner/README.md) (crate `wega-wpt-runner`, member of the `engines` workspace): runs upstream WPT reftests and testharness tests against the engine and gates on `baseline.json`.
- Compatibility scoreboard: planned. See `docs/architecture/module-map.md`.
