# runtime/tab-manager

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Tab lifecycle and movable-tab management.

## Intended responsibilities

- Tab creation, discard and restore
- Tab movement between windows

## Expected dependencies

`foundation/*`, `services/*`; may orchestrate all other groups through traits. Specifically: `runtime/process`, `services/navigation`.

## Dependencies to avoid

Being imported by lower-level groups (`dom`, `css`, `layout`, `graphics`, `html`).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
