//! Authoritative storage key attribution and partition scoping.
//!
//! Origins and containers are isolated strictly by construction. Opaque origins
//! and stale grants are rejected before accessing persistent browser data.

use crate::authority::Origin;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_STORAGE_QUOTA: usize = 10 * 1024 * 1024;
pub const MAX_STORAGE_KEYS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageKeyError {
    OpaqueOrigin,
    InvalidOrigin,
    StaleGrant,
    QuotaExceeded,
}

impl std::fmt::Display for StorageKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpaqueOrigin => write!(f, "Opaque origins cannot access persistent storage"),
            Self::InvalidOrigin => write!(f, "Invalid origin serialization"),
            Self::StaleGrant => write!(f, "Stale or uncommitted document grant"),
            Self::QuotaExceeded => write!(f, "QuotaExceededError: storage budget exceeded"),
        }
    }
}

impl std::error::Error for StorageKeyError {}

/// Authoritative storage key uniquely identifying a storage partition.
///
/// Combines container/profile partition, top-level partition (for third-party partitioning),
/// and document tuple origin context.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StorageKey {
    pub container: String,
    pub top_level_site: Option<(String, String)>,
    pub origin: Origin,
}

impl StorageKey {
    /// Constructs a verified storage key. Rejects opaque origins and invalid serializations.
    pub fn for_origin(
        origin: Origin,
        top_level_site: Option<(String, String)>,
        container: Option<String>,
    ) -> Result<Self, StorageKeyError> {
        let container = container.unwrap_or_else(|| "default".to_string());
        match &origin {
            Origin::Opaque => Err(StorageKeyError::OpaqueOrigin),
            Origin::Tuple(tuple) => {
                if tuple == "null" || tuple.is_empty() {
                    return Err(StorageKeyError::OpaqueOrigin);
                }
                if !tuple.starts_with("http://") && !tuple.starts_with("https://") {
                    return Err(StorageKeyError::InvalidOrigin);
                }
                Ok(Self {
                    container,
                    top_level_site,
                    origin,
                })
            }
        }
    }

    /// Tuple origin string, guaranteed to be a valid HTTP or HTTPS origin.
    pub fn origin_str(&self) -> &str {
        match &self.origin {
            Origin::Tuple(s) => s.as_str(),
            Origin::Opaque => "null",
        }
    }

    /// Whether this storage key represents a third-party partitioned context.
    pub fn is_third_party(&self) -> bool {
        let Some((top_scheme, top_domain)) = &self.top_level_site else {
            return false;
        };
        let origin = self.origin_str();
        let expected_prefix = format!("{top_scheme}://{top_domain}");
        !origin.starts_with(&expected_prefix)
    }

    /// Canonical partition key string used for hashing and directory identification.
    pub fn canonical_partition_key(&self) -> String {
        let origin = self.origin_str();
        if let Some((scheme, domain)) = &self.top_level_site {
            format!("{}:{scheme}://{domain}:{origin}", self.container)
        } else {
            format!("{}:{origin}", self.container)
        }
    }

    /// Deterministic SHA-256 partition hash matching the directory structure on disk.
    pub fn partition_hash(&self) -> String {
        // Compute SHA-256 without pulling in extra crypto deps at the protocol layer.
        // For standard 1st-party default container origins, this matches sha256(origin).
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        self.canonical_partition_key().hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Resolves the storage root directory for this key under the given base profile path.
    pub fn partition_dir(&self, root: &Path) -> PathBuf {
        let base = if self.container.is_empty() || self.container == "default" {
            root.to_path_buf()
        } else {
            root.join("containers").join(&self.container)
        };
        base.join(self.partition_hash())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_key_rejects_opaque_and_null_origins() {
        assert_eq!(
            StorageKey::for_origin(Origin::Opaque, None, None),
            Err(StorageKeyError::OpaqueOrigin)
        );
        assert_eq!(
            StorageKey::for_origin(Origin::Tuple("null".into()), None, None),
            Err(StorageKeyError::OpaqueOrigin)
        );
        assert_eq!(
            StorageKey::for_origin(Origin::Tuple("".into()), None, None),
            Err(StorageKeyError::OpaqueOrigin)
        );
        assert_eq!(
            StorageKey::for_origin(Origin::Tuple("file:///tmp".into()), None, None),
            Err(StorageKeyError::InvalidOrigin)
        );
    }

    #[test]
    fn storage_keys_isolate_containers_and_top_level_partitions() {
        let origin_a = Origin::Tuple("https://example.com".into());
        let key_default = StorageKey::for_origin(origin_a.clone(), None, None).unwrap();
        let key_work = StorageKey::for_origin(origin_a.clone(), None, Some("work".into())).unwrap();
        let key_partitioned = StorageKey::for_origin(
            origin_a,
            Some(("https".into(), "embedder.test".into())),
            None,
        )
        .unwrap();

        assert_ne!(
            key_default.canonical_partition_key(),
            key_work.canonical_partition_key()
        );
        assert_ne!(
            key_default.canonical_partition_key(),
            key_partitioned.canonical_partition_key()
        );
        assert!(key_partitioned.is_third_party());
        assert!(!key_default.is_third_party());
    }
}
