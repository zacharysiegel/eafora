use shared::license::DistributionContext;

/// The context every bundle in this session is opened under. Detecting a third-party host is not wired up
/// yet, so this always answers first party; when it is, this is the one place that decides. It must not be
/// inferred from which bundle was loaded: the onboard bundle and the live bundle are the same deployment.
pub fn resolve_distribution_context() -> DistributionContext {
    DistributionContext::FirstParty
}
