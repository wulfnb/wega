//! Authoritative SQLite-backed IndexedDB transactional storage engine.
//!
//! Implements version change transactions, object stores, and structured record storage
//! with ACID rollbacks and WAL crash recovery.

use rusqlite::{Connection as Database, OptionalExtension, params};
use std::path::Path;
use std::time::Duration;

pub const INDEXEDDB_QUOTA: usize = 10 * 1024 * 1024;

pub struct IndexedDB {
    db: Database,
}

impl IndexedDB {
    pub fn open(partition_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(partition_dir).map_err(|e| e.to_string())?;
        let db_path = partition_dir.join("indexeddb.db");
        let db = Database::open(&db_path).map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA mmap_size=0;
             PRAGMA temp_store=memory;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS databases (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name VARCHAR UNIQUE NOT NULL,
                 version INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE IF NOT EXISTS object_stores (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 db_id INTEGER NOT NULL,
                 name VARCHAR NOT NULL,
                 key_path VARCHAR,
                 auto_increment INTEGER NOT NULL DEFAULT 0,
                 FOREIGN KEY(db_id) REFERENCES databases(id) ON DELETE CASCADE,
                 UNIQUE(db_id, name)
             );
             CREATE TABLE IF NOT EXISTS records (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 store_id INTEGER NOT NULL,
                 key VARCHAR NOT NULL,
                 value BLOB NOT NULL,
                 FOREIGN KEY(store_id) REFERENCES object_stores(id) ON DELETE CASCADE,
                 UNIQUE(store_id, key)
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { db })
    }

    /// Open or upgrade a database. Returns (db_id, version, upgraded).
    pub fn open_database(
        &mut self,
        name: &str,
        requested_version: Option<u64>,
    ) -> Result<(i64, u64, bool), String> {
        let existing: Option<(i64, u64)> = self
            .db
            .query_row(
                "SELECT id, version FROM databases WHERE name = ?",
                [name],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)),
            )
            .optional()
            .map_err(|e| e.to_string())?;

        match (existing, requested_version) {
            (None, None) => {
                self.db
                    .execute(
                        "INSERT INTO databases (name, version) VALUES (?, 1)",
                        [name],
                    )
                    .map_err(|e| e.to_string())?;
                Ok((self.db.last_insert_rowid(), 1, true))
            }
            (None, Some(req)) => {
                if req == 0 {
                    return Err("VersionError: The requested version must be positive".into());
                }
                self.db
                    .execute(
                        "INSERT INTO databases (name, version) VALUES (?, ?)",
                        params![name, req as i64],
                    )
                    .map_err(|e| e.to_string())?;
                Ok((self.db.last_insert_rowid(), req, true))
            }
            (Some((id, current)), None) => Ok((id, current, false)),
            (Some((id, current)), Some(req)) => {
                if req < current {
                    return Err(format!(
                        "VersionError: The requested version ({req}) is less than existing version ({current})"
                    ));
                }
                if req > current {
                    self.db
                        .execute(
                            "UPDATE databases SET version = ? WHERE id = ?",
                            params![req as i64, id],
                        )
                        .map_err(|e| e.to_string())?;
                    Ok((id, req, true))
                } else {
                    Ok((id, current, false))
                }
            }
        }
    }

    pub fn delete_database(&mut self, name: &str) -> Result<bool, String> {
        let affected = self
            .db
            .execute("DELETE FROM databases WHERE name = ?", [name])
            .map_err(|e| e.to_string())?;
        Ok(affected > 0)
    }

    pub fn databases(&self) -> Result<Vec<(String, u64)>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT name, version FROM databases ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    }

    pub fn create_object_store(
        &mut self,
        db_id: i64,
        name: &str,
        key_path: Option<&str>,
        auto_increment: bool,
    ) -> Result<i64, String> {
        self.db.execute(
            "INSERT INTO object_stores (db_id, name, key_path, auto_increment) VALUES (?, ?, ?, ?)",
            params![db_id, name, key_path, if auto_increment { 1 } else { 0 }],
        ).map_err(|e| e.to_string())?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn delete_object_store(&mut self, db_id: i64, name: &str) -> Result<bool, String> {
        let affected = self
            .db
            .execute(
                "DELETE FROM object_stores WHERE db_id = ? AND name = ?",
                params![db_id, name],
            )
            .map_err(|e| e.to_string())?;
        Ok(affected > 0)
    }

    pub fn object_store_id(&self, db_id: i64, name: &str) -> Result<Option<i64>, String> {
        self.db
            .query_row(
                "SELECT id FROM object_stores WHERE db_id = ? AND name = ?",
                params![db_id, name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn put_record(&mut self, store_id: i64, key: &str, value: &[u8]) -> Result<(), String> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let total_size: i64 = tx
            .query_row(
                "SELECT COALESCE(SUM(length(value) + length(key)), 0) FROM records",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        let old_size: Option<i64> = tx
            .query_row(
                "SELECT length(value) + length(key) FROM records WHERE store_id = ? AND key = ?",
                params![store_id, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;

        let record_size = value.len() + key.len();
        let new_total = total_size
            .saturating_sub(old_size.unwrap_or(0))
            .saturating_add(record_size as i64);

        if new_total > INDEXEDDB_QUOTA as i64 {
            return Err("QuotaExceededError: indexedDB storage budget exceeded".into());
        }

        tx.execute(
            "INSERT INTO records (store_id, key, value) VALUES (?, ?, ?)
             ON CONFLICT(store_id, key) DO UPDATE SET value=excluded.value",
            params![store_id, key, value],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_record(&self, store_id: i64, key: &str) -> Result<Option<Vec<u8>>, String> {
        self.db
            .query_row(
                "SELECT value FROM records WHERE store_id = ? AND key = ?",
                params![store_id, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn delete_record(&mut self, store_id: i64, key: &str) -> Result<bool, String> {
        let affected = self
            .db
            .execute(
                "DELETE FROM records WHERE store_id = ? AND key = ?",
                params![store_id, key],
            )
            .map_err(|e| e.to_string())?;
        Ok(affected > 0)
    }

    pub fn record_keys(&self, store_id: i64) -> Result<Vec<String>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT key FROM records WHERE store_id = ? ORDER BY id")
            .map_err(|e| e.to_string())?;
        let keys = stmt
            .query_map([store_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexeddb_version_change_object_stores_and_quota() {
        let dir = tempfile::tempdir().unwrap();
        let mut idb = IndexedDB::open(dir.path()).unwrap();

        // 1. Initial open creates database with version 1
        let (db_id, version, upgraded) = idb.open_database("test_db", None).unwrap();
        assert_eq!(version, 1);
        assert!(upgraded);

        // 2. Create object store
        let store_id = idb
            .create_object_store(db_id, "users", Some("id"), false)
            .unwrap();
        assert_eq!(idb.object_store_id(db_id, "users").unwrap(), Some(store_id));

        // 3. Put and get record
        idb.put_record(store_id, "user:1", b"{\"name\":\"Alice\"}")
            .unwrap();
        let record = idb.get_record(store_id, "user:1").unwrap().unwrap();
        assert_eq!(record, b"{\"name\":\"Alice\"}");

        // 4. Upgrade database to version 2
        let (db_id2, version2, upgraded2) = idb.open_database("test_db", Some(2)).unwrap();
        assert_eq!(db_id2, db_id);
        assert_eq!(version2, 2);
        assert!(upgraded2);

        // 5. Downward version change must fail
        let err = idb.open_database("test_db", Some(1));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("VersionError"));

        // 6. Quota check
        let huge = vec![0u8; 11 * 1024 * 1024];
        let quota_err = idb.put_record(store_id, "user:huge", &huge);
        assert!(quota_err.is_err());
        assert!(quota_err.unwrap_err().contains("QuotaExceededError"));

        // 7. Delete record and store
        assert!(idb.delete_record(store_id, "user:1").unwrap());
        assert!(idb.get_record(store_id, "user:1").unwrap().is_none());

        assert!(idb.delete_object_store(db_id, "users").unwrap());
        assert!(idb.object_store_id(db_id, "users").unwrap().is_none());

        // 8. Delete database
        assert!(idb.delete_database("test_db").unwrap());
        assert!(idb.databases().unwrap().is_empty());
    }
}
