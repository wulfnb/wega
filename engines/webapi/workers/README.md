# webapi/workers

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Dedicated, shared and service worker APIs.

## Intended responsibilities

- Worker lifecycle and messaging
- Service worker registration and fetch interception hooks

## Expected dependencies

`foundation/*`, `javascript/bindings`, `services/*` through traits. Specifically: `webapi/core`, `runtime/process`, `javascript/runtime`.

## Dependencies to avoid

`graphics/*` internals, `layout/*` internals, `adapters/*` directly.

## Current status

PLANNED — NOT IMPLEMENTED. This directory is a placeholder created in migration Phase 1. It contains no code, no `Cargo.toml`, and is not part of any Cargo workspace. See `docs/architecture/module-map.md`.
