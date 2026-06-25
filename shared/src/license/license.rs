use crate::canonical::canonical_model::LicenseShardClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistributionContext {
    FirstParty,
    Embedded,
}

impl DistributionContext {
    pub fn authorized_classes(self) -> &'static [LicenseShardClass] {
        match self {
            DistributionContext::FirstParty => &[
                LicenseShardClass::Base,
                LicenseShardClass::NonCommercial,
                LicenseShardClass::ShareAlike,
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
            &[LicenseShardClass::Base, LicenseShardClass::NonCommercial, LicenseShardClass::ShareAlike],
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
