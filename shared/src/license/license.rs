use crate::canonical::canonical_model::LicenseShardClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistributionContext {
    FirstParty,
    Embedded,
}

impl DistributionContext {
    /// Order is significant: shard selection takes the first authorized class that has a shard for
    /// the requested statistic, so this is a precedence list, not just a membership set. Reordering
    /// changes which shard renders when a statistic ships under more than one authorized class.
    pub fn authorized_classes(self) -> &'static [LicenseShardClass] {
        match self {
            DistributionContext::FirstParty => &[
                LicenseShardClass::Base,
                LicenseShardClass::ShareAlike,
                LicenseShardClass::NonCommercial,
            ],
            DistributionContext::Embedded => &[
                LicenseShardClass::Base,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_context_first_party_authorizes_all_classes() {
        assert_eq!(
            DistributionContext::FirstParty.authorized_classes(),
            &[LicenseShardClass::Base, LicenseShardClass::ShareAlike, LicenseShardClass::NonCommercial],
        );
    }

    #[test]
    fn distribution_context_embedded_authorizes_base_only() {
        assert_eq!(
            DistributionContext::Embedded.authorized_classes(),
            &[LicenseShardClass::Base],
        );
    }
}
