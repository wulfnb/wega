use crate::{error_auth, error_report_model::Report};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
pub struct Store {
    db: Connection,
    retention_seconds: i64,
}
#[derive(Debug, PartialEq)]
pub enum Acceptance {
    New,
    Duplicate,
    Replay,
    Conflict,
    RateLimited,
}
impl Store {
    pub fn open(path: &Path, retention_days: i64) -> rusqlite::Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS clients(id TEXT PRIMARY KEY,public_key BLOB NOT NULL,revoked INTEGER NOT NULL DEFAULT 0);
          CREATE TABLE IF NOT EXISTS errors(client_id TEXT NOT NULL,event_id TEXT NOT NULL,received_at INTEGER NOT NULL,category TEXT NOT NULL,body_hash TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(client_id,event_id));
          CREATE INDEX IF NOT EXISTS errors_received_at ON errors(received_at);
          CREATE TABLE IF NOT EXISTS nonces(client_id TEXT NOT NULL,nonce TEXT NOT NULL,expires_at INTEGER NOT NULL,PRIMARY KEY(client_id,nonce));")?;
        Ok(Self {
            db,
            retention_seconds: retention_days.clamp(1, 365) * 86400,
        })
    }
    pub fn enroll(&self, id: &str, key: &[u8; 32]) -> rusqlite::Result<()> {
        self.db.execute("INSERT INTO clients(id,public_key,revoked) VALUES(?1,?2,0) ON CONFLICT(id) DO UPDATE SET public_key=excluded.public_key,revoked=0",params![id,key.as_slice()])?;
        Ok(())
    }
    pub fn revoke(&self, id: &str) -> rusqlite::Result<()> {
        self.db
            .execute("UPDATE clients SET revoked=1 WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn key(&self, id: &str) -> rusqlite::Result<Option<[u8; 32]>> {
        let key: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT public_key FROM clients WHERE id=?1 AND revoked=0",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(key.and_then(|k| k.try_into().ok()))
    }
    pub fn healthy(&self) -> bool {
        self.db
            .query_row("SELECT 1", [], |r| r.get::<_, i32>(0))
            .is_ok()
    }
    pub fn accept(
        &mut self,
        client: &str,
        nonce: &str,
        now: i64,
        report: &Report,
        body: &[u8],
    ) -> rusqlite::Result<Acceptance> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM nonces WHERE expires_at < ?1", [now])?;
        tx.execute(
            "DELETE FROM errors WHERE received_at < ?1",
            [now - self.retention_seconds],
        )?;
        if tx.execute(
            "INSERT OR IGNORE INTO nonces(client_id,nonce,expires_at) VALUES(?1,?2,?3)",
            params![client, nonce, now + 600],
        )? == 0
        {
            return Ok(Acceptance::Replay);
        }
        let hash = error_auth::body_hash(body);
        let previous: Option<String> = tx
            .query_row(
                "SELECT body_hash FROM errors WHERE client_id=?1 AND event_id=?2",
                params![client, report.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            tx.commit()?;
            return Ok(if previous == hash {
                Acceptance::Duplicate
            } else {
                Acceptance::Conflict
            });
        }
        let recent: i64 = tx.query_row(
            "SELECT COUNT(*) FROM errors WHERE client_id=?1 AND received_at>=?2",
            params![client, now - 60],
            |r| r.get(0),
        )?;
        if recent >= 6000 {
            return Ok(Acceptance::RateLimited);
        }
        let category = serde_json::to_string(&report.diagnostic.category).unwrap_or_default();
        tx.execute("INSERT INTO errors(client_id,event_id,received_at,category,body_hash,payload) VALUES(?1,?2,?3,?4,?5,?6)",params![client,report.id,now,category,hash,String::from_utf8_lossy(body)])?;
        tx.commit()?;
        Ok(Acceptance::New)
    }
    pub fn export(&self, limit: usize) -> rusqlite::Result<Vec<String>> {
        let mut statement = self.db.prepare(
            "SELECT client_id,received_at,payload FROM errors ORDER BY received_at DESC LIMIT ?1",
        )?;
        statement
            .query_map([limit.min(10000) as i64], |r| {let payload:String=r.get(2)?;Ok(serde_json::json!({"client_id":r.get::<_,String>(0)?,"received_at":r.get::<_,i64>(1)?,"report":serde_json::from_str::<serde_json::Value>(&payload).unwrap_or(serde_json::Value::Null)}).to_string())})?
            .collect()
    }
}
