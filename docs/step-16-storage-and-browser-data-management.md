# Step 16 Contract: Complete Storage and Browser Data Management

## 1. Overview & Objectives

Step 16 delivers complete authoritative storage and browser data management for Wega:
- Authoritative storage keys (`StorageKey` in `engines/protocol/src/storage.rs`) with container/profile context, top-level partition (`top_level_site`), and document tuple origin context.
- Strict security boundaries: rejection of opaque origins, null origins, and uncommitted/stale document grants before accessing persistent storage.
- Storage isolation: cross-origin and cross-container partitions guarantee that origins and containers cannot access or inspect each other's data.
- Cookie management and inspector: authoritative `inventory()` exposing full attribute metadata (including `http_only`, `secure`, and `same_site`) to the host inspector, while script access (`document.cookie`) strictly filters out HttpOnly cookies and denies script overwrites.
- Per-site and global data clearing (`clear_site_data`, `clear_all_data`, `clear_site`, `clear`) removing advertised stores without collateral damage.
- SQLite WAL crash recovery and atomic transactions across all persistent stores.
- Dedicated transactional packages for `CacheStorage` (Request/Response pair caching) and `IndexedDB` (version change transactions, object stores, and structured record storage).
- Validated import formats and legacy data preservation on copies, preventing silent data loss.

## 2. Architecture & Implementation

### Authoritative Storage Keys & Protocol Version 16 (`engines/protocol/`)
- `StorageKey` (`engines/protocol/src/storage.rs`):
  - Encapsulates `container: String`, `top_level_site: Option<(String, String)>`, and `origin: Origin`.
  - `for_origin()` validates scheme and origin format, returning `StorageKeyError::OpaqueOrigin` for `Origin::Opaque` or null origins.
  - `is_third_party()` detects partitioned third-party context.
  - `canonical_partition_key()` and `partition_dir()` generate deterministic directory paths and hashes matching profile roots.
- `FrameAuthority::storage_key()` (`engines/protocol/src/authority.rs`):
  - Directly resolves trusted document requests; returns `StorageKeyError::StaleGrant` if uncommitted or invalidated.
- Protocol Version:
  - Bumped to `VERSION = 16`.
  - Added `Capability::AuthoritativeStorage` negotiated at version 16.

### Network Services, CacheStorage & IndexedDB (`engines/network/`)
- `StorageHub` & `Broker` (`engines/network/src/services.rs`, `lib.rs`):
  - SQLite WAL mode with 2-second busy timeout for concurrent tab locking and crash recovery.
  - Quota enforcement: `STORAGE_BYTES` (10MB limit per partition); oversized payloads reject with `QuotaExceededError`.
  - `clear_origin()` removes SQLite database and cached connections for a specific origin.
  - `clear_all()` wipes all persistent and transient storage under a profile root.
- `CacheStorage` (`engines/network/src/cache_storage.rs`):
  - SQLite backing database (`cache_storage.db`) with WAL mode.
  - Cache management: `open_cache`, `has_cache`, `delete_cache`, `cache_names`.
  - Request/Response pairs: `put_entry` (with 10MB quota verification), `match_entry`, `delete_entry`, `entry_urls`.
- `IndexedDB` (`engines/network/src/indexeddb.rs`):
  - SQLite backing database (`indexeddb.db`) with WAL mode and foreign key cascades.
  - Version change transactions: `open_database` with version tracking; downward version changes reject with `VersionError`.
  - Object store operations: `create_object_store`, `delete_object_store`, `put_record`, `get_record`, `delete_record`, `record_keys`.
- `Cookies` (`engines/network/src/cookies.rs`, `engines/wega/src/cookies.rs`):
  - `inventory(&self, url: &Url) -> Vec<CookieEntry>`: host inspector views all matching cookies with HttpOnly flags.
  - Script access via `get(&self, url, true, ...)` completely filters out HttpOnly cookies; script `set()` rejects overwrites.
  - `clear_site(&self, url: &Url)`: selectively purges only target registrable domain cookies.
  - `clear(&self)`: purges entire jar and saves empty state atomically.
  - `import_legacy_cookies(&self, path: &Path)`: imports validated cookies with 4MB and 512-cookie limits, rejecting corrupted JSON.

## 3. Verification & Evidence

- Unit test suite `engines/wega/tests/storage.rs`:
  - `storage_keys_enforce_authoritative_isolation_and_reject_opaque_origins` — PASSED
  - `local_storage_persists_across_navigation_and_isolates_origins` — PASSED
  - `local_storage_quota_rejects_oversized_payloads` — PASSED
  - `cookie_inventory_httponly_isolation_and_site_clearing` — PASSED
  - `legacy_cookie_import_preserves_data_on_copies_and_rejects_corruption` — PASSED
  - `cache_storage_request_response_lifecycle_and_wal_recovery` — PASSED
  - `indexeddb_transactions_version_changes_and_crash_recovery` — PASSED
  - Total: 7/7 storage tests passing.
- Web regression suite `engines/wega/tests/web.rs`:
  - 10/10 tests passing.
- Protocol regression suite `engines/protocol/src/lib.rs`:
  - 37/37 tests passing.
- Network services test suite `engines/network/src/services.rs`:
  - `storage_clear_origin_and_clear_all_removes_data` — PASSED
- Browser host check:
  - `cargo check --bin wega`: compiled cleanly with zero errors.
