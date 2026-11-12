//! Persistent origin storage: Cache Storage, the Cache API and IndexedDB.
//! Extracted from `wega-network`, which re-exports these items for compatibility.
//! Storage never depends on the HTTP crate.
#![forbid(unsafe_code)]

pub mod cache_api;
pub mod cache_storage;
pub mod indexeddb;
pub use cache_storage::{CacheEntry, CacheStorage};
pub use indexeddb::IndexedDB;
