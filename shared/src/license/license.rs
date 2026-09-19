use crate::canonical::canonical_model::LicenseShardClass;

/// Where an Eafora client is being served from, which decides how much of the licensed data it may show.
/// This is a property of the deployment, not of which artifact bundle happens to be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistributionContext {
    /// Eafora serving its own site.
    FirstParty,
    /// Eafora running inside another party's site, where share-alike and non-commercial terms forbid
    /// redistributing the data.
    ThirdParty,
}

impl DistributionContext {
    /// Order is significant; the slice is a precedence list.
    pub fn authorized_classes(self) -> &'static [LicenseShardClass] {
        match self {
            DistributionContext::FirstParty => &[
                LicenseShardClass::Base,
                LicenseShardClass::ShareAlike,
                LicenseShardClass::NonCommercial,
            ],
            DistributionContext::ThirdParty => &[
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
    fn distribution_context_third_party_authorizes_base_only() {
        assert_eq!(
            DistributionContext::ThirdParty.authorized_classes(),
            &[LicenseShardClass::Base],
        );
    }
}
