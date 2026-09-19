use shared::license::DistributionContext;

/// The one place the distribution context is decided. It must not be inferred from which bundle was
/// loaded: the embedded bundle and the live bundle are the same distribution.
pub fn resolve_context() -> DistributionContext {
    DistributionContext::FirstParty
}
