//! Origin-scoped Cache API operations. The browser, rather than a renderer, owns
//! SQLite paths, transactions, matching rules and admission budgets.
use crate::cache_storage::{CACHE_STORAGE_QUOTA, CacheEntry, CacheStorage};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

pub fn cache_allowed(url: &Url) -> bool {
    url.scheme() == "https"
        || (url.scheme() == "http"
            && match url.host() {
                Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                Some(url::Host::Domain(host)) => {
                    host == "localhost" || host.ends_with(".localhost")
                }
                None => false,
            })
}

pub const CACHE_ENTRY_BYTES: usize = 1024 * 1024;
const MAX_CACHES: usize = 128;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheRecord {
    pub request: CacheRequest,
    pub response: CacheEntry,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CacheQuery {
    pub ignore_search: bool,
    pub ignore_method: bool,
    pub ignore_vary: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheQueryKind {
    First,
    All,
    Keys,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CacheOperation {
    Open {
        name: String,
    },
    Has {
        name: String,
    },
    DeleteCache {
        name: String,
    },
    Names,
    Query {
        id: Option<i64>,
        #[serde(default)]
        name: Option<String>,
        request: Option<CacheRequest>,
        options: CacheQuery,
        kind: CacheQueryKind,
    },
    Put {
        id: i64,
        records: Vec<CacheRecord>,
    },
    Delete {
        id: i64,
        request: CacheRequest,
        options: CacheQuery,
    },
}

fn normalized_url(value: &str, ignore_search: bool) -> Result<Url, String> {
    let mut url = Url::parse(value).map_err(|_| "TypeError: Invalid cache URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("TypeError: Cache requests must use HTTP or HTTPS".into());
    }
    url.set_fragment(None);
    if ignore_search {
        url.set_query(None);
    }
    Ok(url)
}
fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    let values: Vec<_> = headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}
fn matches(record: &CacheRecord, request: &CacheRequest, query: CacheQuery) -> bool {
    if !query.ignore_method && request.method != "GET" {
        return false;
    }
    if normalized_url(&record.request.url, query.ignore_search).ok()
        != normalized_url(&request.url, query.ignore_search).ok()
    {
        return false;
    }
    if !query.ignore_vary
        && let Some(vary) = header(&record.response.headers, "vary")
    {
        for name in vary
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if name == "*"
                || header(&record.request.headers, name) != header(&request.headers, name)
            {
                return false;
            }
        }
    }
    true
}
pub(crate) fn records(db: &Connection, id: i64) -> Result<Vec<(i64, CacheRecord)>, String> {
    let mut statement = db
        .prepare("SELECT id, request, response FROM cache_api_entries WHERE cache_id=? ORDER BY id")
        .map_err(|error| error.to_string())?;
    let raw = statement
        .query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    raw.into_iter()
        .map(|(id, request, response)| {
            Ok((
                id,
                CacheRecord {
                    request: serde_json::from_str(&request).map_err(|error| error.to_string())?,
                    response: serde_json::from_str(&response).map_err(|error| error.to_string())?,
                },
            ))
        })
        .collect()
}
pub(crate) fn lookup(db: &Connection, id: i64, url: &str) -> Result<Option<CacheEntry>, String> {
    let request = CacheRequest {
        url: url.into(),
        method: "GET".into(),
        headers: vec![],
    };
    Ok(records(db, id)?
        .into_iter()
        .find(|(_, record)| matches(record, &request, CacheQuery::default()))
        .map(|(_, record)| record.response))
}
fn check_cache(db: &Connection, id: i64) -> Result<(), String> {
    if db
        .query_row("SELECT 1 FROM caches WHERE id=?", [id], |_| Ok(()))
        .optional()
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err("NotFoundError: Cache has been deleted".into());
    }
    Ok(())
}
impl CacheStorage {
    pub fn open_origin(root: &std::path::Path, origin: &str) -> Result<Self, String> {
        let url = Url::parse(origin).map_err(|_| "SecurityError: Invalid cache origin")?;
        if !cache_allowed(&url) || url.origin().ascii_serialization() != origin {
            return Err("SecurityError: Cache storage requires a secure tuple origin".into());
        }
        let directory = root.join(
            Sha256::digest(origin.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        );
        Self::open(&directory)
    }
    pub(crate) fn initialize_api(&self) -> Result<(), String> {
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS cache_api_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cache_id INTEGER NOT NULL REFERENCES caches(id) ON DELETE CASCADE,
            request TEXT NOT NULL, response TEXT NOT NULL, size INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS cache_api_owner ON cache_api_entries(cache_id);",
            )
            .map_err(|error| error.to_string())?;
        // Preserve the original on-disk response format. Both the Rust helper
        // API and page bindings use the same records after this atomic migration.
        let tx = self
            .db
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        {
            let mut statement = tx.prepare("SELECT cache_id,url,method,status,status_text,headers,body FROM entries ORDER BY id").map_err(|error|error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        CacheEntry {
                            url: row.get(1)?,
                            method: row.get(2)?,
                            status: row.get(3)?,
                            status_text: row.get(4)?,
                            headers: serde_json::from_str(&row.get::<_, String>(5)?)
                                .unwrap_or_default(),
                            body: row.get(6)?,
                        },
                    ))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (id, response) = row.map_err(|error| error.to_string())?;
                let request = CacheRequest {
                    url: response.url.clone(),
                    method: response.method.clone(),
                    headers: vec![],
                };
                for (entry, old) in records(&tx, id)? {
                    if matches(&old, &request, CacheQuery::default()) {
                        tx.execute("DELETE FROM cache_api_entries WHERE id=?", [entry])
                            .map_err(|error| error.to_string())?;
                    }
                }
                let request = serde_json::to_string(&request).map_err(|error| error.to_string())?;
                let response =
                    serde_json::to_string(&response).map_err(|error| error.to_string())?;
                let size = (request.len() + response.len()) as i64;
                tx.execute("INSERT INTO cache_api_entries(cache_id,request,response,size) VALUES (?,?,?,?)",params![id,request,response,size]).map_err(|error|error.to_string())?;
            }
        }
        tx.execute("DELETE FROM entries", [])
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())
    }
    pub fn api_call(&mut self, operation: CacheOperation) -> Result<Value, String> {
        match operation {
            CacheOperation::Open { name } => {
                if name.len() > 4096 {
                    return Err("QuotaExceededError: Cache name exceeds budget".into());
                }
                let tx = self.db.transaction().map_err(|error| error.to_string())?;
                if let Some(id) = tx
                    .query_row("SELECT id FROM caches WHERE name=?", [&name], |row| {
                        row.get::<_, i64>(0)
                    })
                    .optional()
                    .map_err(|error| error.to_string())?
                {
                    return Ok(json!(id));
                }
                let count: i64 = tx
                    .query_row("SELECT COUNT(*) FROM caches", [], |row| row.get(0))
                    .map_err(|error| error.to_string())?;
                if count >= MAX_CACHES as i64 {
                    return Err("QuotaExceededError: Too many named caches".into());
                }
                tx.execute("INSERT INTO caches(name) VALUES (?)", [name])
                    .map_err(|error| error.to_string())?;
                let id = tx.last_insert_rowid();
                tx.commit().map_err(|error| error.to_string())?;
                Ok(json!(id))
            }
            CacheOperation::Has { name } => Ok(json!(
                self.db
                    .query_row("SELECT 1 FROM caches WHERE name=?", [name], |_| Ok(()))
                    .optional()
                    .map_err(|error| error.to_string())?
                    .is_some()
            )),
            CacheOperation::DeleteCache { name } => Ok(json!(self.delete_cache(&name)?)),
            CacheOperation::Names => Ok(json!(self.cache_names()?)),
            CacheOperation::Query {
                id,
                name,
                request,
                options,
                kind,
            } => {
                if let Some(request) = &request {
                    normalized_url(&request.url, false)?;
                }
                let ids = if let Some(id) = id {
                    check_cache(&self.db, id)?;
                    vec![id]
                } else {
                    let mut statement = self
                        .db
                        .prepare("SELECT id FROM caches WHERE (?1 IS NULL OR name=?1) ORDER BY id")
                        .map_err(|error| error.to_string())?;
                    statement
                        .query_map([name], |row| row.get::<_, i64>(0))
                        .map_err(|error| error.to_string())?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| error.to_string())?
                };
                let mut result = Vec::new();
                for id in ids {
                    for (_, record) in records(&self.db, id)? {
                        if request
                            .as_ref()
                            .is_none_or(|request| matches(&record, request, options))
                        {
                            result.push(if matches!(kind, CacheQueryKind::Keys) {
                                json!(record.request)
                            } else {
                                json!(record.response)
                            });
                            if matches!(kind, CacheQueryKind::First) {
                                return Ok(json!(result));
                            }
                        }
                    }
                }
                Ok(json!(result))
            }
            CacheOperation::Put { id, records: added } => {
                if added.len() > 32 {
                    return Err("QuotaExceededError: Cache batch exceeds entry budget".into());
                }
                // Validate the whole batch before mutating. addAll is atomic even
                // when a later response is invalid or the origin quota is hit.
                for (index, record) in added.iter().enumerate() {
                    normalized_url(&record.request.url, false)?;
                    if record.request.method != "GET"
                        || !(200..=599).contains(&record.response.status)
                        || record.response.status == 206
                        || header(&record.response.headers, "vary")
                            .is_some_and(|value| value.split(',').any(|name| name.trim() == "*"))
                    {
                        return Err("TypeError: Response cannot be cached".into());
                    }
                    if record.response.body.len() > CACHE_ENTRY_BYTES {
                        return Err(
                            "QuotaExceededError: Cache response exceeds 1 MiB budget".into()
                        );
                    }
                    if added[..index]
                        .iter()
                        .any(|previous| matches(previous, &record.request, CacheQuery::default()))
                    {
                        return Err("InvalidStateError: Duplicate request in cache batch".into());
                    }
                }
                let tx = self.db.transaction().map_err(|error| error.to_string())?;
                check_cache(&tx, id)?;
                for record in added {
                    for (entry, old) in records(&tx, id)? {
                        if matches(&old, &record.request, CacheQuery::default()) {
                            tx.execute("DELETE FROM cache_api_entries WHERE id=?", [entry])
                                .map_err(|error| error.to_string())?;
                        }
                    }
                    let request = serde_json::to_string(&record.request)
                        .map_err(|error| error.to_string())?;
                    let response = serde_json::to_string(&record.response)
                        .map_err(|error| error.to_string())?;
                    let (used, count): (i64, i64) = tx
                        .query_row(
                            "SELECT COALESCE(SUM(size),0), COUNT(*) FROM cache_api_entries",
                            [],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .map_err(|error| error.to_string())?;
                    let size = request.len().saturating_add(response.len()) as i64;
                    if used.saturating_add(size) > CACHE_STORAGE_QUOTA as i64
                        || count >= MAX_ENTRIES as i64
                    {
                        return Err("QuotaExceededError: Cache storage budget exceeded".into());
                    }
                    tx.execute("INSERT INTO cache_api_entries(cache_id,request,response,size) VALUES (?,?,?,?)", params![id, request, response, size]).map_err(|error| error.to_string())?;
                }
                tx.commit().map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            CacheOperation::Delete {
                id,
                request,
                options,
            } => {
                normalized_url(&request.url, false)?;
                let tx = self.db.transaction().map_err(|error| error.to_string())?;
                check_cache(&tx, id)?;
                let mut removed = false;
                for (entry, record) in records(&tx, id)? {
                    if matches(&record, &request, options) {
                        tx.execute("DELETE FROM cache_api_entries WHERE id=?", [entry])
                            .map_err(|error| error.to_string())?;
                        removed = true;
                    }
                }
                tx.commit().map_err(|error| error.to_string())?;
                Ok(json!(removed))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(path: &str, body: &[u8]) -> CacheRecord {
        CacheRecord {
            request: CacheRequest {
                url: format!("https://cache.test/{path}"),
                method: "GET".into(),
                headers: vec![],
            },
            response: CacheEntry {
                url: format!("https://cache.test/{path}"),
                method: "GET".into(),
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                body: body.to_vec(),
            },
        }
    }
    fn query(store: &mut CacheStorage, id: i64) -> Value {
        store
            .api_call(CacheOperation::Query {
                id: Some(id),
                name: None,
                request: None,
                options: CacheQuery::default(),
                kind: CacheQueryKind::All,
            })
            .unwrap()
    }
    #[test]
    fn cache_batches_rollback_on_late_quota_failure_and_duplicate_requests() {
        let mut store = CacheStorage::in_memory().unwrap();
        let id = store
            .api_call(CacheOperation::Open { name: "v1".into() })
            .unwrap()
            .as_i64()
            .unwrap();
        store
            .api_call(CacheOperation::Put {
                id,
                records: vec![item("original", b"saved")],
            })
            .unwrap();
        let mut batch = vec![item("original", b"replacement")];
        // A non-ASCII byte serializes to four JSON bytes; the aggregate batch
        // exceeds the origin quota while every individual entry is admissible.
        for index in 0..3 {
            batch.push(item(
                &format!("large-{index}"),
                &vec![255; CACHE_ENTRY_BYTES],
            ));
        }
        assert!(
            store
                .api_call(CacheOperation::Put { id, records: batch })
                .unwrap_err()
                .contains("QuotaExceededError")
        );
        let rows = query(&mut store, id);
        assert_eq!(rows.as_array().unwrap().len(), 1);
        assert_eq!(rows[0]["body"], json!(b"saved".to_vec()));
        assert!(
            store
                .api_call(CacheOperation::Put {
                    id,
                    records: vec![item("duplicate", b"1"), item("duplicate#fragment", b"2")]
                })
                .unwrap_err()
                .contains("InvalidStateError")
        );
        assert_eq!(query(&mut store, id), rows);
    }
    #[test]
    fn cache_origin_paths_and_admission_are_not_controlled_by_page_names() {
        let dir = tempfile::tempdir().unwrap();
        assert!(CacheStorage::open_origin(dir.path(), "http://remote.test").is_err());
        assert!(CacheStorage::open_origin(dir.path(), "null").is_err());
        let mut store = CacheStorage::open_origin(dir.path(), "https://cache.test").unwrap();
        let id = store
            .api_call(CacheOperation::Open {
                name: "../../outside".into(),
            })
            .unwrap()
            .as_i64()
            .unwrap();
        store
            .api_call(CacheOperation::Put {
                id,
                records: vec![item("key", b"persisted")],
            })
            .unwrap();
        drop(store);
        let mut reopened = CacheStorage::open_origin(dir.path(), "https://cache.test").unwrap();
        assert_eq!(
            query(&mut reopened, id)[0]["body"],
            json!(b"persisted".to_vec())
        );
        let mut other = CacheStorage::open_origin(dir.path(), "https://other.test").unwrap();
        assert_eq!(other.api_call(CacheOperation::Names).unwrap(), json!([]));
        for index in 1..MAX_CACHES {
            reopened
                .api_call(CacheOperation::Open {
                    name: index.to_string(),
                })
                .unwrap();
        }
        assert!(
            reopened
                .api_call(CacheOperation::Open {
                    name: "overflow".into()
                })
                .unwrap_err()
                .contains("QuotaExceededError")
        );
        assert!(!dir.path().join("outside").exists());
    }
    #[test]
    fn legacy_cache_records_are_migrated_once_and_shared_with_rust_helpers() {
        let root = tempfile::tempdir().unwrap();
        {
            let store = CacheStorage::open(root.path()).unwrap();
            store
                .db
                .execute("INSERT INTO caches(id,name) VALUES(7,'legacy')", [])
                .unwrap();
            store.db.execute("INSERT INTO entries(cache_id,url,method,status,status_text,headers,body) VALUES (7,'https://cache.test/legacy','GET',200,'OK','[]',?1)",[b"retained".as_slice()]).unwrap();
        }
        let mut store = CacheStorage::open(root.path()).unwrap();
        assert_eq!(query(&mut store, 7)[0]["body"], json!(b"retained".to_vec()));
        store
            .api_call(CacheOperation::Put {
                id: 7,
                records: vec![item("from-page", b"shared")],
            })
            .unwrap();
        assert_eq!(
            store
                .match_entry(7, "https://cache.test/from-page")
                .unwrap()
                .unwrap()
                .body,
            b"shared"
        );
        store
            .put_entry(
                7,
                "https://cache.test/from-rust",
                "GET",
                200,
                "OK",
                &[],
                b"same backend",
            )
            .unwrap();
        assert_eq!(query(&mut store, 7).as_array().unwrap().len(), 3);
        drop(store);
        let mut reopened = CacheStorage::open(root.path()).unwrap();
        assert_eq!(query(&mut reopened, 7).as_array().unwrap().len(), 3);
    }
}
