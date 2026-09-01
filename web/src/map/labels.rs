use leptos_i18n::I18nContext;

use shared::canonical::canonical_model::TemporalBasis;
use shared::canonical::StatisticKind;

use crate::i18n::*;

pub fn statistic_label(i18n: I18nContext<Locale>, statistic: StatisticKind) -> String {
    match statistic {
        StatisticKind::Tfr => t_string!(i18n, statistic.tfr).to_string(),
        StatisticKind::Ccf => t_string!(i18n, statistic.ccf).to_string(),
        StatisticKind::MeanAgeAtChildbirth => t_string!(i18n, statistic.mean_age_at_childbirth).to_string(),
        StatisticKind::MeanAgeAtFirstBirth => t_string!(i18n, statistic.mean_age_at_first_birth).to_string(),
    }
}

/// What the scrubber's axis is measuring: a calendar year for a period measure, a birth cohort for a cohort
/// measure.
pub fn period_axis_label(i18n: I18nContext<Locale>, statistic: StatisticKind) -> String {
    match statistic.temporal_basis() {
        TemporalBasis::Period => t_string!(i18n, scrubber.label).to_string(),
        TemporalBasis::Cohort => t_string!(i18n, scrubber.cohort_label).to_string(),
    }
}

pub fn statistic_unit(i18n: I18nContext<Locale>, statistic: StatisticKind) -> String {
    match statistic {
        StatisticKind::Tfr => t_string!(i18n, statistic.tfr_unit).to_string(),
        StatisticKind::Ccf => t_string!(i18n, statistic.ccf_unit).to_string(),
        StatisticKind::MeanAgeAtChildbirth => t_string!(i18n, statistic.mean_age_at_childbirth_unit).to_string(),
        StatisticKind::MeanAgeAtFirstBirth => t_string!(i18n, statistic.mean_age_at_first_birth_unit).to_string(),
    }
}

pub fn statistic_description(i18n: I18nContext<Locale>, statistic: StatisticKind) -> String {
    match statistic {
        StatisticKind::Tfr => t_string!(i18n, statistic.tfr_description).to_string(),
        StatisticKind::Ccf => t_string!(i18n, statistic.ccf_description).to_string(),
        StatisticKind::MeanAgeAtChildbirth => t_string!(i18n, statistic.mean_age_at_childbirth_description).to_string(),
        StatisticKind::MeanAgeAtFirstBirth => t_string!(i18n, statistic.mean_age_at_first_birth_description).to_string(),
    }
}

/// The caption for the color transform's inflection on the legend (e.g. "replacement" for TFR at 2.1), or
/// `None` for a statistic with no meaningful threshold at its inflection.
pub fn reference_caption(i18n: I18nContext<Locale>, statistic: StatisticKind) -> Option<String> {
    match statistic {
        StatisticKind::Tfr | StatisticKind::Ccf => Some(t_string!(i18n, legend.replacement).to_string()),
        StatisticKind::MeanAgeAtChildbirth | StatisticKind::MeanAgeAtFirstBirth => None,
    }
}

/// Named where a statistic covers only part of the world, so selecting it does not read as a map that has
/// broken. `None` where coverage is global.
pub fn statistic_coverage(i18n: I18nContext<Locale>, statistic: StatisticKind) -> Option<String> {
    match statistic {
        StatisticKind::Tfr | StatisticKind::Ccf => None,
        StatisticKind::MeanAgeAtChildbirth | StatisticKind::MeanAgeAtFirstBirth => {
            Some(t_string!(i18n, statistic.coverage_europe).to_string())
        }
    }
}

/// How many decimal places a statistic's values are shown to. Eurostat publishes an age to one, and rendering
/// a second asserts precision the source does not carry.
pub fn statistic_decimals(statistic: StatisticKind) -> usize {
    match statistic {
        StatisticKind::Tfr | StatisticKind::Ccf => 2,
        StatisticKind::MeanAgeAtChildbirth | StatisticKind::MeanAgeAtFirstBirth => 1,
    }
}
