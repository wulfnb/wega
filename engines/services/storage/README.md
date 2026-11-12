# services/storage

**Status: PLANNED — NOT IMPLEMENTED**

## Purpose

Persistent origin storage backing Cache Storage, the Cache API and IndexedDB. Renamed from `services/cache` (Phase 2 decision): HTTP caching is a separate concern and stays with `services/http`.

## Intended responsibilities

- Cache Storage and Cache API persistence (quota, records, lookup)
- IndexedDB persistence
- Per-origin storage roots and quota accounting

## Expected dependencies

`foundation/*` (when those exist); `wega-protocol` types; `rusqlite`, `serde`, `serde_json`, `sha2`, `url`.

## Dependencies to avoid

`wega-engine`, `services/http` (storage must not depend on HTTP), `dom/*`, `css/*`, `layout/*`, `graphics/*`, `javascript/*`, `adapters/*`.

## Current status

PLANNED — NOT IMPLEMENTED. Planned package `wega-storage`. Candidate sources: `engines/network/src/cache_storage.rs`, `cache_api.rs`, `indexeddb.rs`. See `docs/architecture/crate-initialization-plan.md`.
