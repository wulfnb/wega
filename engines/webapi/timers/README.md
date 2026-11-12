# webapi/timers

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Timers and scheduling APIs.

## Intended responsibilities

- setTimeout/setInterval/requestAnimationFrame semantics
- Timer clamping and throttling policy

## Expected dependencies

`foundation/*`, `javascript/bindings`, `services/*` through traits. Specifically: `webapi/core`, `runtime/scheduler`.

## Dependencies to avoid

`graphics/*` internals, `layout/*` internals, `adapters/*` directly.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
