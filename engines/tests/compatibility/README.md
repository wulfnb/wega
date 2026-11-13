# tests/compatibility

**Status: PLANNED — NOT IMPLEMENTED**

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

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
