# runtime/process

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Process model and sandbox supervision.

## Intended responsibilities

- Process spawn, IPC and crash recovery
- Site-isolation assignment

## Expected dependencies

`foundation/*`, `services/*`; may orchestrate all other groups through traits. Specifically: `engines/protocol` (existing), `engines/platform` (existing).

## Dependencies to avoid

Being imported by lower-level groups (`dom`, `css`, `layout`, `graphics`, `html`).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
