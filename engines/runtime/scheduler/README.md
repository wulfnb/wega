# runtime/scheduler

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Task and frame scheduling.

## Intended responsibilities

- Task sources, priorities and event loop
- Frame pacing and idle work

## Expected dependencies

`foundation/*`, `services/*`; may orchestrate all other groups through traits. Specifically: `foundation/*`.

## Dependencies to avoid

Being imported by lower-level groups (`dom`, `css`, `layout`, `graphics`, `html`).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
