use serde::{Deserialize, Serialize};

use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StatisticShardKey {
    pub statistic_kind: StatisticKind,
    pub license_shard_class: LicenseShardClass,
}
