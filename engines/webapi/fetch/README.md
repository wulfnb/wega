# webapi/fetch

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Fetch API, Request/Response and CORS semantics.

## Intended responsibilities

- Fetch algorithm and request/response models
- Bridging to the HTTP service

## Expected dependencies

`foundation/*`, `javascript/bindings`, `services/*` through traits. Specifically: `webapi/core`, `services/http`, `services/cookies`.

## Dependencies to avoid

`graphics/*` internals, `layout/*` internals, `adapters/*` directly.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
