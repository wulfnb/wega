# runtime/orchestration

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Top-level engine assembly and pipeline orchestration.

## Intended responsibilities

- Wiring subsystems into a page-load pipeline
- Lifecycle of document load, style, layout, paint

## Expected dependencies

`foundation/*`, `services/*`; may orchestrate all other groups through traits. Specifically: All engine groups via public APIs.

## Dependencies to avoid

Being imported by lower-level groups (`dom`, `css`, `layout`, `graphics`, `html`).

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
