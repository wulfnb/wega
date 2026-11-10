//! Authoritative SQLite-backed CacheStorage Request/Response implementation.
//!
//! Provides transactional Request/Response pair caching under an origin partition,
//! with SQLite WAL crash recovery, quota checking and deletion semantics.

use rusqlite::{Connection as Database, OptionalExtension, params};
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
    db: Database,
}

impl CacheStorage {
    pub fn open(partition_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(partition_dir).map_err(|e| e.to_string())?;
        let db_path = partition_dir.join("cache_storage.db");
        let db = Database::open(&db_path).map_err(|e| e.to_string())?;
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
        Ok(Self { db })
    }

    pub fn open_cache(&mut self, name: &str) -> Result<i64, String> {
        let existing: Option<i64> = self
            .db
            .query_row("SELECT id FROM caches WHERE name = ?", [name], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;

        if let Some(id) = existing {
            return Ok(id);
        }

        self.db
            .execute("INSERT INTO caches (name) VALUES (?)", [name])
            .map_err(|e| e.to_string())?;
        Ok(self.db.last_insert_rowid())
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
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let total_size: i64 = tx.query_row(
            "SELECT COALESCE(SUM(length(body) + length(headers) + length(url)), 0) FROM entries",
            [],
            |row| row.get(0)
        ).map_err(|e| e.to_string())?;

        let old_size: Option<i64> = tx.query_row(
            "SELECT length(body) + length(headers) + length(url) FROM entries WHERE cache_id = ? AND url = ?",
            params![cache_id, url],
            |row| row.get(0)
        ).optional().map_err(|e| e.to_string())?;

        let headers_json = serde_json::to_string(headers).map_err(|e| e.to_string())?;
        let entry_size = body.len() + headers_json.len() + url.len();

        let new_total = total_size
            .saturating_sub(old_size.unwrap_or(0))
            .saturating_add(entry_size as i64);

        if new_total > CACHE_STORAGE_QUOTA as i64 {
            return Err("QuotaExceededError: cache storage budget exceeded".into());
        }

        tx.execute(
            "INSERT INTO entries (cache_id, url, method, status, status_text, headers, body)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(cache_id, url) DO UPDATE SET
                 method=excluded.method,
                 status=excluded.status,
                 status_text=excluded.status_text,
                 headers=excluded.headers,
                 body=excluded.body",
            params![
                cache_id,
                url,
                method,
                status as i32,
                status_text,
                headers_json,
                body
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn match_entry(&self, cache_id: i64, url: &str) -> Result<Option<CacheEntry>, String> {
        let entry = self.db.query_row(
            "SELECT method, status, status_text, headers, body FROM entries WHERE cache_id = ? AND url = ?",
            params![cache_id, url],
            |row| {
                let method: String = row.get(0)?;
                let status: i32 = row.get(1)?;
                let status_text: String = row.get(2)?;
                let headers_raw: String = row.get(3)?;
                let body: Vec<u8> = row.get(4)?;
                Ok((method, status, status_text, headers_raw, body))
            }
        ).optional().map_err(|e| e.to_string())?;

        let Some((method, status, status_text, headers_raw, body)) = entry else {
            return Ok(None);
        };

        let headers: Vec<(String, String)> = serde_json::from_str(&headers_raw).unwrap_or_default();
        Ok(Some(CacheEntry {
            url: url.to_string(),
            method,
            status: status as u16,
            status_text,
            headers,
            body,
        }))
    }

    pub fn delete_entry(&mut self, cache_id: i64, url: &str) -> Result<bool, String> {
        let affected = self
            .db
            .execute(
                "DELETE FROM entries WHERE cache_id = ? AND url = ?",
                params![cache_id, url],
            )
            .map_err(|e| e.to_string())?;
        Ok(affected > 0)
    }

    pub fn entry_urls(&self, cache_id: i64) -> Result<Vec<String>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT url FROM entries WHERE cache_id = ? ORDER BY id")
            .map_err(|e| e.to_string())?;
        let urls = stmt
            .query_map([cache_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(urls)
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
