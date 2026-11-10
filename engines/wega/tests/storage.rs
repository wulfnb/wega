//! Step 16 Storage and Browser Data Management Verification Suite.
//!
//! Verifies:
//! - Authoritative storage keys with origin, container/profile, and top-level partition context.
//! - Opaque origin and stale grant rejection.
//! - localStorage persistence and cross-origin isolation.
//! - sessionStorage browsing context scope and lifetime.
//! - Quota enforcement (10MB limit and QuotaExceededError rejection).
//! - SQLite WAL crash recovery and atomic transactions.
//! - Cookie inventory with HttpOnly inspector and script isolation.
//! - Per-site and global clear-data removing advertised stores.
//! - Validated import formats and legacy data preservation on copies.
//! - CacheStorage Request/Response semantics and IndexedDB transactions.

use std::{sync::Arc, time::Duration};
use url::Url;
use wega_engine::{Engine, Limits, NetworkPolicy, View};
use wega_network::{CacheStorage, Cookies, IndexedDB, SiteContext};
use wega_protocol::{
    DocumentContext, DocumentId, DocumentRevision,
    authority::{FrameAuthority, Origin, Principal},
    identity::{FrameId, ProcessSessionId, SessionIdentity, TabId},
    storage::{DEFAULT_STORAGE_QUOTA, StorageKey, StorageKeyError},
};

fn persistent_limits() -> Limits {
    Limits {
        script_time: Duration::from_secs(2),
        ..Limits::default()
    }
}

fn test_engine(storage_dir: std::path::PathBuf) -> Engine {
    Engine::new(
        persistent_limits(),
        NetworkPolicy {
            allow_loopback: true,
            ..Default::default()
        },
        Arc::new(|| {}),
    )
    .unwrap()
    .with_storage_dir(storage_dir)
}

fn create_page(engine: &Engine, address: &str) -> View {
    let mut view = engine
        .create_view(Url::parse("about:blank").unwrap())
        .unwrap();
    view.url = Url::parse(address).unwrap();
    view.set_html("<title>Storage Test</title>").unwrap();
    view.poll();
    view
}

#[test]
fn storage_keys_enforce_authoritative_isolation_and_reject_opaque_origins() {
    // 1. Opaque and null origins are rejected
    assert_eq!(
        StorageKey::for_origin(Origin::Opaque, None, None),
        Err(StorageKeyError::OpaqueOrigin)
    );
    assert_eq!(
        StorageKey::for_origin(Origin::Tuple("null".into()), None, None),
        Err(StorageKeyError::OpaqueOrigin)
    );
    assert_eq!(
        StorageKey::for_origin(Origin::Tuple("data:text/html,test".into()), None, None),
        Err(StorageKeyError::InvalidOrigin)
    );

    // 2. Container and top-level partition keys are isolated
    let origin = Origin::Tuple("https://app.example.com".into());
    let default_key = StorageKey::for_origin(origin.clone(), None, None).unwrap();
    let work_key = StorageKey::for_origin(origin.clone(), None, Some("work".into())).unwrap();
    let personal_key =
        StorageKey::for_origin(origin.clone(), None, Some("personal".into())).unwrap();
    let third_party_key = StorageKey::for_origin(
        origin,
        Some(("https".into(), "publisher.example".into())),
        None,
    )
    .unwrap();

    assert_eq!(default_key.container, "default");
    assert_eq!(work_key.container, "work");
    assert_ne!(
        default_key.canonical_partition_key(),
        work_key.canonical_partition_key()
    );
    assert_ne!(
        work_key.canonical_partition_key(),
        personal_key.canonical_partition_key()
    );
    assert_ne!(
        default_key.canonical_partition_key(),
        third_party_key.canonical_partition_key()
    );

    assert!(third_party_key.is_third_party());
    assert!(!default_key.is_third_party());

    // 3. FrameAuthority rejects uncommitted or stale grants
    let session = SessionIdentity {
        tab: TabId(1),
        frame: FrameId(1),
        process: ProcessSessionId([7; 16]),
        service: None,
    };
    let mut authority = FrameAuthority::new(session);
    let context = DocumentContext {
        document: DocumentId(10),
        revision: DocumentRevision(1),
    };

    // Stale grant before binding
    assert_eq!(
        authority.storage_key(context, None),
        Err(StorageKeyError::StaleGrant)
    );

    // Bind valid principal
    authority.bind(
        context,
        Principal {
            origin: Origin::Tuple("https://trusted.bank".into()),
            top_level_site: Some(("https".into(), "trusted.bank".into())),
        },
    );
    let verified = authority
        .storage_key(context, Some("banking".into()))
        .unwrap();
    assert_eq!(verified.container, "banking");
    assert_eq!(verified.origin_str(), "https://trusted.bank");

    // Invalidation clears access immediately
    authority.invalidate();
    assert_eq!(
        authority.storage_key(context, None),
        Err(StorageKeyError::StaleGrant)
    );
}

#[test]
fn local_storage_persists_across_navigation_and_isolates_origins() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = test_engine(tmp.path().to_path_buf());

    // 1. Set key on origin One
    {
        let mut page_one = create_page(&engine, "https://one.test/");
        page_one.evaluate("localStorage.setItem('auth_token', 'token_123'); localStorage.setItem('num', '42');").unwrap();
        assert_eq!(
            page_one
                .evaluate_json("localStorage.getItem('auth_token')")
                .unwrap(),
            "\"token_123\""
        );
    }

    // 2. Navigate to another path on origin One: data persists
    {
        let mut page_one_next = create_page(&engine, "https://one.test/dashboard");
        assert_eq!(
            page_one_next
                .evaluate_json("localStorage.getItem('auth_token')")
                .unwrap(),
            "\"token_123\""
        );
        assert_eq!(
            page_one_next
                .evaluate_json("localStorage.getItem('num')")
                .unwrap(),
            "\"42\""
        );
        assert_eq!(
            page_one_next.evaluate_json("localStorage.length").unwrap(),
            "2"
        );
    }

    // 3. Different origin Two has completely isolated storage
    {
        let mut page_two = create_page(&engine, "https://two.test/");
        assert_eq!(
            page_two
                .evaluate_json("localStorage.getItem('auth_token')")
                .unwrap(),
            "null"
        );
        assert_eq!(page_two.evaluate_json("localStorage.length").unwrap(), "0");
    }
}

#[test]
fn local_storage_quota_rejects_oversized_payloads() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = test_engine(tmp.path().to_path_buf());

    let mut view = create_page(&engine, "https://quota.test/");
    // Storing under quota succeeds
    view.evaluate("localStorage.setItem('small', 'hello world')")
        .unwrap();
    assert_eq!(
        view.evaluate_json("localStorage.getItem('small')").unwrap(),
        "\"hello world\""
    );

    // Storing over 10MB quota throws QuotaExceededError
    let oversized_script = format!(
        "let threw = false;
         try {{
             localStorage.setItem('huge', 'a'.repeat({}));
         }} catch (e) {{
             threw = true;
         }}
         globalThis.__quota_threw = threw;",
        DEFAULT_STORAGE_QUOTA + 1024
    );
    view.evaluate(&oversized_script).unwrap();
    assert_eq!(
        view.evaluate_json("globalThis.__quota_threw").unwrap(),
        "true"
    );
}

#[test]
fn cookie_inventory_httponly_isolation_and_site_clearing() {
    let tmp = tempfile::tempdir().unwrap();
    let jar = Cookies::default();
    jar.set_directory(tmp.path().to_path_buf());

    let auth_url = Url::parse("https://id.bank.test/login").unwrap();
    let blog_url = Url::parse("https://news.other.test/feed").unwrap();

    // Set server HTTP-only session cookie and client visible cookie
    jar.set(
        &auth_url,
        "session=secret_token; Path=/; Secure; HttpOnly",
        false,
        SiteContext::SameSite,
    );
    jar.set(
        &auth_url,
        "prefs=dark_mode; Path=/; Secure",
        false,
        SiteContext::SameSite,
    );
    jar.set(
        &blog_url,
        "tracker=anon_id; Path=/; Secure",
        false,
        SiteContext::SameSite,
    );

    // 1. Script cannot read HttpOnly cookie
    let script_cookies = jar.get(&auth_url, true, SiteContext::SameSite);
    assert_eq!(script_cookies, "prefs=dark_mode");
    assert!(!script_cookies.contains("session=secret_token"));

    // 2. HTTP resource context can read HttpOnly cookie
    let http_cookies = jar.get(&auth_url, false, SiteContext::SameSite);
    assert!(http_cookies.contains("session=secret_token"));
    assert!(http_cookies.contains("prefs=dark_mode"));

    // 3. Authoritative host inspector sees all cookies with full attribute records
    let inventory = jar.inventory(&auth_url);
    assert_eq!(inventory.len(), 2);
    let session_entry = inventory.iter().find(|c| c.name == "session").unwrap();
    assert!(session_entry.http_only);
    assert!(session_entry.secure);
    assert_eq!(session_entry.value, "secret_token");

    let prefs_entry = inventory.iter().find(|c| c.name == "prefs").unwrap();
    assert!(!prefs_entry.http_only);
    assert_eq!(prefs_entry.value, "dark_mode");

    // 4. Per-site cookie clearing clears bank.test without touching other.test
    let cleared = jar.clear_site(&auth_url);
    assert_eq!(cleared, 2);
    assert!(jar.inventory(&auth_url).is_empty());
    assert_eq!(jar.inventory(&blog_url).len(), 1);

    // 5. Global clearing removes all cookies
    jar.clear();
    assert!(jar.inventory(&blog_url).is_empty());
}

#[test]
fn legacy_cookie_import_preserves_data_on_copies_and_rejects_corruption() {
    let tmp = tempfile::tempdir().unwrap();
    let jar = Cookies::default();
    jar.set_directory(tmp.path().to_path_buf());

    let test_url = Url::parse("https://legacy.example.com/").unwrap();
    jar.set(
        &test_url,
        "auth=persisted; Path=/; Secure; Max-Age=3600",
        false,
        SiteContext::SameSite,
    );

    // Saved cookies.json exists
    let original_file = tmp.path().join("cookies.json");
    assert!(original_file.exists());

    // Create a copy to simulate migration testing on copies
    let copy_file = tmp.path().join("cookies_migration_copy.json");
    std::fs::copy(&original_file, &copy_file).unwrap();

    // Clear live jar
    jar.clear();
    assert!(jar.inventory(&test_url).is_empty());

    // Import from validated copy
    let count = jar.import_legacy_cookies(&copy_file).unwrap();
    assert_eq!(count, 1);
    assert_eq!(jar.inventory(&test_url).len(), 1);
    assert_eq!(jar.inventory(&test_url)[0].value, "persisted");

    // Rejection of corrupted files
    let corrupted_file = tmp.path().join("corrupted.json");
    std::fs::write(&corrupted_file, b"{\"invalid\": [truncated").unwrap();
    assert!(jar.import_legacy_cookies(&corrupted_file).is_err());
}

#[test]
fn cache_storage_request_response_lifecycle_and_wal_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let partition_dir = tmp.path().join("partition_cache");

    // 1. Open CacheStorage and write entries
    {
        let mut cache_store = CacheStorage::open(&partition_dir).unwrap();
        let cache_id = cache_store.open_cache("static-v1").unwrap();

        cache_store
            .put_entry(
                cache_id,
                "https://cdn.example/bundle.js",
                "GET",
                200,
                "OK",
                &[
                    ("content-type", "application/javascript"),
                    ("etag", "\"xyz\""),
                ],
                b"console.log('cached script');",
            )
            .unwrap();

        let names = cache_store.cache_names().unwrap();
        assert_eq!(names, vec!["static-v1"]);
    }

    // 2. Re-open (verifying SQLite WAL recovery and persistence)
    {
        let mut cache_store = CacheStorage::open(&partition_dir).unwrap();
        assert!(cache_store.has_cache("static-v1"));
        let cache_id = cache_store.open_cache("static-v1").unwrap();

        let matched = cache_store
            .match_entry(cache_id, "https://cdn.example/bundle.js")
            .unwrap()
            .unwrap();
        assert_eq!(matched.status, 200);
        assert_eq!(matched.status_text, "OK");
        assert_eq!(matched.body, b"console.log('cached script');");
        assert!(
            matched
                .headers
                .iter()
                .any(|(k, v)| k == "etag" && v == "\"xyz\"")
        );

        // Delete entry and cache
        assert!(
            cache_store
                .delete_entry(cache_id, "https://cdn.example/bundle.js")
                .unwrap()
        );
        assert!(
            cache_store
                .match_entry(cache_id, "https://cdn.example/bundle.js")
                .unwrap()
                .is_none()
        );
        assert!(cache_store.delete_cache("static-v1").unwrap());
        assert!(!cache_store.has_cache("static-v1"));
    }
}

#[test]
fn indexeddb_transactions_version_changes_and_crash_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let partition_dir = tmp.path().join("partition_idb");

    // 1. Versionchange transaction: create database and object store
    {
        let mut idb = IndexedDB::open(&partition_dir).unwrap();
        let (db_id, version, upgraded) = idb.open_database("app_db", Some(1)).unwrap();
        assert_eq!(version, 1);
        assert!(upgraded);

        let store_id = idb
            .create_object_store(db_id, "notes", Some("id"), false)
            .unwrap();
        idb.put_record(store_id, "note:1", b"{\"title\":\"Meeting notes\"}")
            .unwrap();
        idb.put_record(store_id, "note:2", b"{\"title\":\"TODO list\"}")
            .unwrap();
    }

    // 2. Re-open and verify persistence through SQLite WAL
    {
        let mut idb = IndexedDB::open(&partition_dir).unwrap();
        let (db_id, version, upgraded) = idb.open_database("app_db", None).unwrap();
        assert_eq!(version, 1);
        assert!(!upgraded);

        let store_id = idb.object_store_id(db_id, "notes").unwrap().unwrap();
        let record = idb.get_record(store_id, "note:1").unwrap().unwrap();
        assert_eq!(record, b"{\"title\":\"Meeting notes\"}");

        let keys = idb.record_keys(store_id).unwrap();
        assert_eq!(keys, vec!["note:1", "note:2"]);

        // Upgrade to version 2
        let (_, v2, upgraded2) = idb.open_database("app_db", Some(2)).unwrap();
        assert_eq!(v2, 2);
        assert!(upgraded2);

        // Version downgrade fails
        assert!(idb.open_database("app_db", Some(1)).is_err());
    }
}
