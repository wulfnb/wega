# webapi/core

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Shared Web API infrastructure.

## Intended responsibilities

- Common interface scaffolding and feature gating
- Cross-API utilities (abort signals, streams primitives)

## Expected dependencies

`foundation/*`, `javascript/bindings`, `services/*` through traits. Specifically: `foundation/*`.

## Dependencies to avoid

`graphics/*` internals, `layout/*` internals, `adapters/*` directly.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
