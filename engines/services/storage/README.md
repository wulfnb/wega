# services/storage

**Status: IMPLEMENTED (Wave 1) — package `wega-storage`**

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

IMPLEMENTED. Member of the `engines` workspace. Extracted verbatim from `engines/network/src/{cache_storage,cache_api,indexeddb}.rs`; database schemas and file layout are unchanged. `wega-network` re-exports `cache_api`, `cache_storage`, `indexeddb`, `CacheEntry`, `CacheStorage` and `IndexedDB`. This crate has no dependency on `wega-network`. See `docs/architecture/crate-initialization-plan.md`.
