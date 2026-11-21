//! SVG text uses authorized broker bytes. A broker source never discovers paths.
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};
use usvg::fontdb::Database;
type FontBytes = Vec<Vec<u8>>;
type Entry = (Weak<FontBytes>, Weak<Database>);

pub fn database(source: Option<&Arc<FontBytes>>) -> Arc<Database> {
    if let Some(source) = source {
        static CACHE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
        let mut cache = CACHE
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.retain(|(source, database)| source.strong_count() > 0 && database.strong_count() > 0);
        let identity = Arc::downgrade(source);
        if let Some(database) = cache
            .iter()
            .find_map(|(key, database)| key.ptr_eq(&identity).then(|| database.upgrade()).flatten())
        {
            return database;
        }
        let mut database = Database::new();
        for bytes in source.iter() {
            database.load_font_data(bytes.clone());
        }
        let database = Arc::new(database);
        cache.push((identity, Arc::downgrade(&database)));
        database
    } else {
        // Only development embeddings that supply no broker font source use
        // system discovery, matching the existing native font-context policy.
        static SYSTEM: LazyLock<Arc<Database>> = LazyLock::new(|| {
            let mut database = Database::new();
            database.load_system_fonts();
            Arc::new(database)
        });
        SYSTEM.clone()
    }
}
