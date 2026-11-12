//! Authoritative SQLite-backed CacheStorage Request/Response implementation.
//!
//! Provides transactional Request/Response pair caching under an origin partition,
//! with SQLite WAL crash recovery, quota checking and deletion semantics.

use crate::cache_api::{CacheOperation, CacheQuery, CacheRecord, CacheRequest};
use rusqlite::{Connection as Database, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

pub const CACHE_STORAGE_QUOTA: usize = 10 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: String,
    pub method: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub struct CacheStorage {
    pub(crate) db: Database,
}

impl CacheStorage {
    pub fn open(partition_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(partition_dir).map_err(|e| e.to_string())?;
        let db_path = partition_dir.join("cache_storage.db");
        let db = Database::open(&db_path).map_err(|e| e.to_string())?;
        Self::from_connection(db)
    }

    pub fn in_memory() -> Result<Self, String> {
        Self::from_connection(Database::open_in_memory().map_err(|error| error.to_string())?)
    }

    fn from_connection(db: Database) -> Result<Self, String> {
        db.busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA mmap_size=0;
             PRAGMA temp_store=memory;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS caches (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name VARCHAR UNIQUE NOT NULL
             );
             CREATE TABLE IF NOT EXISTS entries (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 cache_id INTEGER NOT NULL,
                 url VARCHAR NOT NULL,
                 method VARCHAR NOT NULL,
                 status INTEGER NOT NULL,
                 status_text VARCHAR NOT NULL,
                 headers VARCHAR NOT NULL,
                 body BLOB NOT NULL,
                 FOREIGN KEY(cache_id) REFERENCES caches(id) ON DELETE CASCADE,
                 UNIQUE(cache_id, url)
             );",
        )
        .map_err(|e| e.to_string())?;
        let store = Self { db };
        store.initialize_api()?;
        Ok(store)
    }

    pub fn open_cache(&mut self, name: &str) -> Result<i64, String> {
        self.api_call(CacheOperation::Open { name: name.into() })?
            .as_i64()
            .ok_or_else(|| "Invalid cache identity".into())
    }

    pub fn has_cache(&self, name: &str) -> bool {
        self.db
            .query_row("SELECT 1 FROM caches WHERE name = ?", [name], |_| Ok(()))
            .optional()
            .unwrap_or(None)
            .is_some()
    }

    pub fn delete_cache(&mut self, name: &str) -> Result<bool, String> {
        let affected = self
            .db
            .execute("DELETE FROM caches WHERE name = ?", [name])
            .map_err(|e| e.to_string())?;
        Ok(affected > 0)
    }

    pub fn cache_names(&self) -> Result<Vec<String>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT name FROM caches ORDER BY id")
            .map_err(|e| e.to_string())?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(names)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn put_entry(
        &mut self,
        cache_id: i64,
        url: &str,
        method: &str,
        status: u16,
        status_text: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(), String> {
        self.api_call(CacheOperation::Put {
            id: cache_id,
            records: vec![CacheRecord {
                request: CacheRequest {
                    url: url.into(),
                    method: method.into(),
                    headers: vec![],
                },
                response: CacheEntry {
                    url: url.into(),
                    method: method.into(),
                    status,
                    status_text: status_text.into(),
                    headers: headers
                        .iter()
                        .map(|(name, value)| (name.to_string(), value.to_string()))
                        .collect(),
                    body: body.into(),
                },
            }],
        })?;
        Ok(())
    }

    pub fn match_entry(&self, cache_id: i64, url: &str) -> Result<Option<CacheEntry>, String> {
        crate::cache_api::lookup(&self.db, cache_id, url)
    }

    pub fn delete_entry(&mut self, cache_id: i64, url: &str) -> Result<bool, String> {
        self.api_call(CacheOperation::Delete {
            id: cache_id,
            request: CacheRequest {
                url: url.into(),
                method: "GET".into(),
                headers: vec![],
            },
            options: CacheQuery::default(),
        })?
        .as_bool()
        .ok_or_else(|| "Invalid cache deletion result".into())
    }

    pub fn entry_urls(&self, cache_id: i64) -> Result<Vec<String>, String> {
        Ok(crate::cache_api::records(&self.db, cache_id)?
            .into_iter()
            .map(|(_, record)| record.request.url)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_storage_creates_matches_and_deletes_with_quota_check() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = CacheStorage::open(dir.path()).unwrap();

        let v1 = store.open_cache("v1").unwrap();
        assert!(store.has_cache("v1"));
        assert!(!store.has_cache("v2"));

        store
            .put_entry(
                v1,
                "https://example.test/api/data",
                "GET",
                200,
                "OK",
                &[("content-type", "application/json")],
                b"{\"answer\":42}",
            )
            .unwrap();

        let entry = store
            .match_entry(v1, "https://example.test/api/data")
            .unwrap()
            .unwrap();
        assert_eq!(entry.status, 200);
        assert_eq!(entry.body, b"{\"answer\":42}");
        assert_eq!(
            entry.headers,
            vec![("content-type".into(), "application/json".into())]
        );

        let urls = store.entry_urls(v1).unwrap();
        assert_eq!(urls, vec!["https://example.test/api/data"]);

        // Quota rejection
        let oversized = vec![0u8; 11 * 1024 * 1024];
        let err = store.put_entry(
            v1,
            "https://example.test/huge",
            "GET",
            200,
            "OK",
            &[],
            &oversized,
        );
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("QuotaExceededError"));

        // Delete entry and delete cache
        assert!(
            store
                .delete_entry(v1, "https://example.test/api/data")
                .unwrap()
        );
        assert!(
            store
                .match_entry(v1, "https://example.test/api/data")
                .unwrap()
                .is_none()
        );

        assert!(store.delete_cache("v1").unwrap());
        assert!(!store.has_cache("v1"));
    }
}
