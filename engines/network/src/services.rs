//! Privileged resources use service-bound operations, never renderer profile paths.
use super::*;
use rusqlite::{Connection as Database, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
const STORAGE_BYTES: usize = 10 * 1024 * 1024;
const STORAGE_KEYS: usize = 4096;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ResourceRequest {
    WebSocketOpen {
        url: String,
        protocols: String,
    },
    WebSocketIo {
        id: String,
        operation: String,
    },
    Font {
        generic: String,
    },
    File {
        url: String,
        limit: usize,
    },
    Storage {
        origin: String,
        persistent: bool,
        operation: String,
        key: String,
        index: u32,
    },
}
#[derive(Default)]
pub(crate) struct StorageHub {
    persistent: HashMap<(PathBuf, String), Database>,
    transient: HashMap<([u8; 16], String), (Weak<Session>, Database)>,
}
struct StorageCall<'a> {
    origin: &'a str,
    persistent: bool,
    operation: &'a str,
    key: &'a str,
    index: u32,
}
impl StorageHub {
    fn call(
        &mut self,
        session: &Arc<Session>,
        call: StorageCall<'_>,
        value: &str,
    ) -> Result<Value, String> {
        let StorageCall {
            origin,
            persistent,
            operation,
            key,
            index,
        } = call;
        if origin == "null"
            || origin.len() > 4096
            || key.len() > MAX_HEADER
            || value.len() > MAX_BODY
        {
            return Err("Invalid storage scope or value".into());
        }
        let map = if persistent {
            &mut self.persistent
        } else {
            // Session storage is isolated to a process session, including same-origin tabs.
            self.transient.retain(|_, (owner, _)| {
                owner
                    .upgrade()
                    .is_some_and(|session| !session.revoked.load(Ordering::Acquire))
            });
            let id = (session.identity.service.0, origin.to_owned());
            if !self.transient.contains_key(&id) {
                if self.transient.len() >= 64 {
                    return Err("Session storage admission limit reached".into());
                }
                let db = Database::open_in_memory().map_err(|error| error.to_string())?;
                Self::initialize(&db)?;
                self.transient
                    .insert(id.clone(), (Arc::downgrade(session), db));
            }
            return Self::operate(
                &mut self.transient.get_mut(&id).unwrap().1,
                operation,
                key,
                value,
                index,
            );
        };
        let root = PathBuf::from(&session.scope);
        let id = (root.clone(), origin.to_owned());
        if !map.contains_key(&id) {
            // Connections are a cache, not authority; closing one preserves its DB.
            if map.len() >= 32 {
                map.clear();
            }
            let directory = root.join(format!("{:x}", Sha256::digest(origin.as_bytes())));
            std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            // Preserve the existing Deno SQLite schema and origin-directory hash.
            let db = Database::open(directory.join("local_storage"))
                .map_err(|error| error.to_string())?;
            Self::initialize(&db)?;
            map.insert(id.clone(), db);
        }
        Self::operate(map.get_mut(&id).unwrap(), operation, key, value, index)
    }
    fn initialize(db: &Database) -> Result<(), String> {
        db.busy_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA mmap_size=0; PRAGMA temp_store=memory; PRAGMA cache_size=-512; CREATE TABLE IF NOT EXISTS data (key VARCHAR UNIQUE, value VARCHAR);").map_err(|error| error.to_string())
    }
    fn operate(
        db: &mut Database,
        operation: &str,
        key: &str,
        value: &str,
        index: u32,
    ) -> Result<Value, String> {
        let result: Result<Value, rusqlite::Error> = (|| {
            Ok(match operation {
                "length" => json!(
                    db.query_row("SELECT COUNT(*) FROM data", [], |row| row.get::<_, u32>(0))?
                ),
                "key" => json!(
                    db.query_row(
                        "SELECT key FROM data ORDER BY rowid LIMIT 1 OFFSET ?",
                        [index],
                        |row| row.get::<_, String>(0)
                    )
                    .optional()?
                ),
                "get" => json!(
                    db.query_row("SELECT value FROM data WHERE key = ?", [key], |row| row
                        .get::<_, String>(
                        0
                    ))
                    .optional()?
                ),
                "set" => {
                    let tx = db.transaction()?;
                    let (size, count): (i64, i64) = tx.query_row("SELECT COALESCE(SUM(length(CAST(key AS BLOB))+length(CAST(value AS BLOB))),0), COUNT(*) FROM data", [], |row| Ok((row.get(0)?,row.get(1)?)))?;
                    let old: Option<i64> = tx.query_row("SELECT length(CAST(key AS BLOB))+length(CAST(value AS BLOB)) FROM data WHERE key=?", [key], |row| row.get(0)).optional()?;
                    if size
                        .saturating_sub(old.unwrap_or(0))
                        .saturating_add(key.len() as i64)
                        .saturating_add(value.len() as i64)
                        > STORAGE_BYTES as i64
                        || (old.is_none() && count >= STORAGE_KEYS as i64)
                    {
                        return Ok(json!({"quota": true}));
                    }
                    // UPDATE preserves insertion order, as required by Storage.key().
                    if old.is_some() {
                        tx.execute("UPDATE data SET value=? WHERE key=?", params![value, key])?;
                    } else {
                        tx.execute(
                            "INSERT INTO data (key,value) VALUES (?,?)",
                            params![key, value],
                        )?;
                    }
                    tx.commit()?;
                    Value::Null
                }
                "remove" => {
                    db.execute("DELETE FROM data WHERE key=?", [key])?;
                    Value::Null
                }
                "clear" => {
                    db.execute("DELETE FROM data", [])?;
                    Value::Null
                }
                "keys" => {
                    let mut statement = db.prepare("SELECT key FROM data ORDER BY rowid")?;
                    let keys = statement
                        .query_map([], |row| row.get::<_, String>(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    json!(keys)
                }
                _ => return Ok(json!({"invalid":true})),
            })
        })();
        let value = result.map_err(|error| error.to_string())?;
        if value.get("quota").is_some() {
            return Err("QuotaExceededError: storage budget exceeded".into());
        }
        if value.get("invalid").is_some() {
            return Err("Unknown storage operation".into());
        }
        Ok(value)
    }
    pub(crate) fn revoke(&mut self, identity: ServiceSessionId) {
        self.transient
            .retain(|(service, _), _| *service != identity.0);
    }
}
impl Broker {
    pub(crate) fn resource(
        &self,
        session: &Arc<Session>,
        request: ResourceRequest,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        if session.revoked.load(Ordering::Acquire) {
            return Err("Revoked resource session".into());
        }
        match request {
            ResourceRequest::WebSocketOpen { url, protocols } => {
                if !body.is_empty() {
                    return Err("Unexpected upgrade body".into());
                }
                self.websocket_open(session, &url, &protocols)
            }
            ResourceRequest::WebSocketIo { id, operation } => {
                self.websocket_io(session, &id, &operation, &body)
            }
            ResourceRequest::Font { generic } => {
                if !body.is_empty() {
                    return Err("Unexpected font body".into());
                }
                static FONTS: OnceLock<Mutex<fontdb::Database>> = OnceLock::new();
                let fonts = FONTS.get_or_init(|| {
                    let mut db = fontdb::Database::new();
                    db.load_system_fonts();
                    Mutex::new(db)
                });
                let family = match generic.as_str() {
                    "sans-serif" => fontdb::Family::SansSerif,
                    "serif" => fontdb::Family::Serif,
                    "monospace" => fontdb::Family::Monospace,
                    _ => return Err("Unapproved generic font".into()),
                };
                let db = fonts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let id = db
                    .query(&fontdb::Query {
                        families: &[family],
                        ..Default::default()
                    })
                    .ok_or("No matching host font")?;
                let (source, _) = db.face_source(id).ok_or("Missing host font source")?;
                match source {
                    fontdb::Source::Binary(bytes) => {
                        let bytes: &[u8] = bytes.as_ref().as_ref();
                        if bytes.len() > MAX_BODY {
                            return Err("Font exceeds byte budget".into());
                        }
                        Ok(bytes.to_vec())
                    }
                    fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => {
                        bounded_file(&path, MAX_BODY)
                    }
                }
            }
            ResourceRequest::File { url, limit } => {
                if !body.is_empty() || limit == 0 || limit > MAX_BODY {
                    return Err("Invalid selected-file request".into());
                }
                let requested = Url::parse(&url).map_err(|error| error.to_string())?;
                let receipt = session
                    .navigation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let approved = receipt
                    .allowed
                    .first()
                    .filter(|url| url.scheme() == "file")
                    .ok_or("No browser-selected file grant")?;
                let root = approved
                    .to_file_path()
                    .map_err(|_| "Invalid file grant")?
                    .canonicalize()
                    .map_err(|error| error.to_string())?;
                let root = root.parent().ok_or("Invalid selected-file directory")?;
                let path = requested
                    .to_file_path()
                    .map_err(|_| "Not a file resource")?
                    .canonicalize()
                    .map_err(|error| error.to_string())?;
                if !path.starts_with(root) {
                    return Err("Selected file is outside the granted directory".into());
                }
                // The worker has no filesystem access. The root is selected by the
                // browser, not a renderer-supplied base directory.
                bounded_file(&path, limit)
            }
            ResourceRequest::Storage {
                origin,
                persistent,
                operation,
                key,
                index,
            } => {
                let receipt = session
                    .navigation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let document = receipt
                    .committed
                    .as_ref()
                    .or(receipt.last.as_ref())
                    .ok_or("No committed document resource grant")?;
                if !matches!(document.scheme(), "http" | "https")
                    || document.origin().ascii_serialization() != origin
                {
                    return Err("Storage document origin mismatch".into());
                }
                let value = String::from_utf8(body).map_err(|_| "Invalid storage encoding")?;
                let result = self
                    .storage
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .call(
                        session,
                        StorageCall {
                            origin: &origin,
                            persistent,
                            operation: &operation,
                            key: &key,
                            index,
                        },
                        &value,
                    )?;
                serde_json::to_vec(&result).map_err(|error| error.to_string())
            }
        }
    }
}
fn bounded_file(path: &std::path::Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err("Resource is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("Resource exceeds byte budget".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document(session: &SessionHandle, address: &str) -> Remote {
        let url = Url::parse(address).unwrap();
        session.begin_navigation(&url);
        session.session.navigation.lock().unwrap().committed = Some(url);
        let (address, token) = session.endpoint();
        Remote::bound(&address, token, session.identity()).unwrap()
    }
    fn storage(
        remote: &Remote,
        origin: &str,
        persistent: bool,
        operation: &str,
        key: &str,
        value: &str,
    ) -> Result<Value, String> {
        let reply = remote.resource(
            ResourceRequest::Storage {
                origin: origin.into(),
                persistent,
                operation: operation.into(),
                key: key.into(),
                index: 0,
            },
            value.as_bytes(),
        )?;
        serde_json::from_slice(&reply).map_err(|error| error.to_string())
    }
    #[test]
    fn storage_preserves_sqlite_data_and_partitions_session_lifetime_and_origins() {
        let broker = Broker::new().unwrap();
        let root = tempfile::tempdir().unwrap();
        let origin = "https://storage.example";
        let directory = root
            .path()
            .join(format!("{:x}", Sha256::digest(origin.as_bytes())));
        std::fs::create_dir_all(&directory).unwrap();
        let old = Database::open(directory.join("local_storage")).unwrap();
        old.execute_batch("CREATE TABLE data (key VARCHAR UNIQUE,value VARCHAR);INSERT INTO data VALUES ('old','retained');").unwrap();
        drop(old);
        let first = broker
            .open_bound_session(root.path().into(), Policy::default())
            .unwrap();
        let a = document(&first, &format!("{origin}/"));
        assert_eq!(
            storage(&a, origin, true, "get", "old", "").unwrap(),
            json!("retained")
        );
        storage(&a, origin, false, "set", "private", "first").unwrap();
        storage(&a, origin, true, "set", "shared", "persisted").unwrap();
        let second = broker
            .open_bound_session(root.path().into(), Policy::default())
            .unwrap();
        let b = document(&second, &format!("{origin}/"));
        assert_eq!(
            storage(&b, origin, true, "get", "shared", "").unwrap(),
            json!("persisted")
        );
        assert_eq!(
            storage(&b, origin, false, "get", "private", "").unwrap(),
            Value::Null
        );
        assert!(storage(&a, "https://foreign.example", true, "get", "old", "").is_err());
        first.begin_navigation(&Url::parse("https://foreign.example/").unwrap());
        assert!(storage(&a, origin, true, "get", "old", "").is_err());
        second.revoke();
        assert!(storage(&b, origin, true, "get", "shared", "").is_err());
    }
    #[test]
    fn bound_cookie_calls_cannot_export_http_only_or_foreign_origin_credentials() {
        let broker = Broker::new().unwrap();
        let root = tempfile::tempdir().unwrap();
        let session = broker
            .open_bound_session(root.path().into(), Policy::default())
            .unwrap();
        let remote = document(&session, "https://active.example/");
        let active = Url::parse("https://active.example/").unwrap();
        let foreign = Url::parse("https://foreign.example/").unwrap();
        session.cookie_jar().set(
            &active,
            "secret=private; Secure; HttpOnly",
            false,
            SiteContext::SameSite,
        );
        session.cookie_jar().set(
            &foreign,
            "foreign=private; Secure",
            false,
            SiteContext::SameSite,
        );
        assert_eq!(remote.cookie_get(&active, false).unwrap(), "");
        assert_eq!(remote.cookie_get(&active, true).unwrap(), "");
        assert!(remote.cookie_get(&foreign, true).is_err());
        assert!(remote.cookie_set(&foreign, "forged=yes", true).is_err());
        remote
            .cookie_set(&active, "visible=yes; Secure", true)
            .unwrap();
        assert_eq!(remote.cookie_get(&active, true).unwrap(), "visible=yes");
    }
    #[test]
    fn selected_files_cannot_read_sibling_directories_or_unapproved_fonts() {
        let broker = Broker::new().unwrap();
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("selected");
        std::fs::create_dir(&selected).unwrap();
        std::fs::write(selected.join("index.html"), "selected").unwrap();
        std::fs::write(root.path().join("secret"), "private").unwrap();
        let session = broker
            .open_bound_session(root.path().join("profile"), Policy::default())
            .unwrap();
        session.begin_navigation(&Url::from_file_path(selected.join("index.html")).unwrap());
        let (address, token) = session.endpoint();
        let remote = Remote::bound(&address, token, session.identity()).unwrap();
        let file = |path| ResourceRequest::File {
            url: Url::from_file_path(path).unwrap().into(),
            limit: 1024,
        };
        assert_eq!(
            remote
                .resource(file(selected.join("index.html")), &[])
                .unwrap(),
            b"selected"
        );
        assert!(
            remote
                .resource(file(root.path().join("secret")), &[])
                .is_err()
        );
        assert!(
            remote
                .resource(
                    ResourceRequest::Font {
                        generic: "../../secret".into()
                    },
                    &[]
                )
                .is_err()
        );
    }
}
