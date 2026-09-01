use shared::canonical::canonical_model::SourceRevision;

use crate::adapter::AdapterOptions;

/// Whether a run can stop before normalizing: the source republishes the same revision until it revises, and
/// every adapter learns the revision only after fetching, so the request is unavoidable and the write is not.
pub fn should_skip_run(
    last_seen: &Option<SourceRevision>,
    revision_label: &str,
    options: AdapterOptions,
) -> bool {
    if options.force_full_refetch {
        return false;
    }

    match last_seen {
        Some(last_seen) => last_seen.revision == revision_label,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_revision(revision: &str) -> SourceRevision {
        SourceRevision {
            revision: revision.to_string(),
            published: None,
            fetched: "2026-07-02T00:00:00Z".parse().unwrap(),
        }
    }

    fn options(force_full_refetch: bool) -> AdapterOptions {
        AdapterOptions { force_full_refetch }
    }

    #[test]
    fn should_skip_run_runs_on_a_first_run() {
        assert!(!should_skip_run(&None, "2026-07-02", options(false)));
    }

    #[test]
    fn should_skip_run_skips_an_unchanged_revision() {
        let last_seen: Option<SourceRevision> = Some(source_revision("2026-07-02"));

        assert!(should_skip_run(&last_seen, "2026-07-02", options(false)));
    }

    #[test]
    fn should_skip_run_runs_a_changed_revision() {
        let last_seen: Option<SourceRevision> = Some(source_revision("2026-07-02"));

        assert!(!should_skip_run(&last_seen, "2026-12-01", options(false)));
    }

    #[test]
    fn should_skip_run_honours_the_force_override_for_an_unchanged_revision() {
        let last_seen: Option<SourceRevision> = Some(source_revision("2026-07-02"));

        assert!(!should_skip_run(&last_seen, "2026-07-02", options(true)));
    }
}
