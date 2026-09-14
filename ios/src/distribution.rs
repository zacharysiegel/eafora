use shared::license::DistributionContext;

/// The boundary's own spelling of [`DistributionContext`], so the FFI does not depend on UniFFI's
/// hidden remote-type macro. The match below fails to compile if a variant is ever added.
#[derive(uniffi::Enum)]
pub enum FfiDistributionContext {
    FirstParty,
    ThirdParty,
}

impl From<FfiDistributionContext> for DistributionContext {
    fn from(context: FfiDistributionContext) -> DistributionContext {
        match context {
            FfiDistributionContext::FirstParty => DistributionContext::FirstParty,
            FfiDistributionContext::ThirdParty => DistributionContext::ThirdParty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_maps_to_its_shared_counterpart() {
        assert_eq!(
            DistributionContext::from(FfiDistributionContext::FirstParty),
            DistributionContext::FirstParty,
        );
        assert_eq!(
            DistributionContext::from(FfiDistributionContext::ThirdParty),
            DistributionContext::ThirdParty,
        );
    }
}
