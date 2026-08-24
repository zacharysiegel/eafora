use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::canonical_model::TemporalBasis;
use shared::canonical::StatisticKind;

use crate::i18n::*;

pub fn statistic_label(i18n: I18nContext<Locale>, statistic: StatisticKind) -> AnyView {
    match statistic {
        StatisticKind::Tfr => t!(i18n, statistic.tfr).into_any(),
        StatisticKind::Ccf => t!(i18n, statistic.ccf).into_any(),
    }
}

/// What the scrubber's axis is measuring: a calendar year for a period measure, a birth cohort for a cohort
/// measure.
pub fn period_axis_label(i18n: I18nContext<Locale>, statistic: StatisticKind) -> AnyView {
    match statistic.temporal_basis() {
        TemporalBasis::Period => t!(i18n, scrubber.label).into_any(),
        TemporalBasis::Cohort => t!(i18n, scrubber.cohort_label).into_any(),
    }
}

/// The same distinction as [`period_axis_label`], for an attribute that takes text rather than a view.
pub fn period_axis_label_text(i18n: I18nContext<Locale>, statistic: StatisticKind) -> String {
    match statistic.temporal_basis() {
        TemporalBasis::Period => t_string!(i18n, scrubber.label).to_string(),
        TemporalBasis::Cohort => t_string!(i18n, scrubber.cohort_label).to_string(),
    }
}

pub fn statistic_unit(i18n: I18nContext<Locale>, statistic: StatisticKind) -> AnyView {
    match statistic {
        StatisticKind::Tfr => t!(i18n, statistic.tfr_unit).into_any(),
        StatisticKind::Ccf => t!(i18n, statistic.ccf_unit).into_any(),
    }
}

/// The caption for the color transform's inflection on the legend (e.g. "replacement" for TFR at 2.1), or
/// `None` for a statistic with no meaningful threshold at its inflection.
pub fn reference_caption(i18n: I18nContext<Locale>, statistic: StatisticKind) -> Option<AnyView> {
    match statistic {
        StatisticKind::Tfr | StatisticKind::Ccf => Some(t!(i18n, legend.replacement).into_any()),
    }
}

/// An SVG text node takes a string rather than a view.
pub fn reference_caption_string(i18n: I18nContext<Locale>, statistic: StatisticKind) -> Option<String> {
    match statistic {
        StatisticKind::Tfr | StatisticKind::Ccf => Some(t_string!(i18n, legend.replacement).to_string()),
    }
}
