use serde::{Deserialize, Serialize};

use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};

/// Content-Type the producer sets when uploading each artifact-bundle file kind; the CDN edge,
/// the browser HTTP cache, and Accept-header negotiation depend on them.
pub const CONTENT_TYPE_MANIFEST: &str = "application/json";
pub const CONTENT_TYPE_FLATGEOBUF: &str = "application/octet-stream";
pub const CONTENT_TYPE_SQLITE: &str = "application/vnd.sqlite3";

/// Cache-Control the producer sets per file kind. Manifest is short-cached so re-platforms
/// propagate within minutes.
pub const CACHE_CONTROL_MANIFEST: &str = "public, max-age=300";
/// Immutable: shard filenames are content-addressed, so a shard's bytes never change.
pub const CACHE_CONTROL_SHARD: &str = "public, max-age=31536000, immutable";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StatisticShardKey {
    pub statistic_kind: StatisticKind,
    pub license_shard_class: LicenseShardClass,
}
