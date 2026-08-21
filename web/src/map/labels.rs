use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::StatisticKind;

use crate::i18n::*;

pub fn statistic_label(i18n: I18nContext<Locale>, statistic: StatisticKind) -> AnyView {
    match statistic {
        StatisticKind::Tfr => t!(i18n, statistic.tfr).into_any(),
        StatisticKind::Ccf => t!(i18n, statistic.ccf).into_any(),
        // test-only variant; never active in production, so this arm only satisfies match exhaustiveness
        StatisticKind::TestAlpha => statistic.code().into_any(),
    }
}

pub fn statistic_unit(i18n: I18nContext<Locale>, statistic: StatisticKind) -> AnyView {
    match statistic {
        StatisticKind::Tfr => t!(i18n, statistic.tfr_unit).into_any(),
        StatisticKind::Ccf => t!(i18n, statistic.ccf_unit).into_any(),
        StatisticKind::TestAlpha => ().into_any(),
    }
}

/// The caption for the color transform's inflection on the legend (e.g. "replacement" for TFR at 2.1), or
/// `None` for a statistic with no meaningful threshold at its inflection.
pub fn reference_caption(i18n: I18nContext<Locale>, statistic: StatisticKind) -> Option<AnyView> {
    match statistic {
        StatisticKind::Tfr | StatisticKind::Ccf => Some(t!(i18n, legend.replacement).into_any()),
        StatisticKind::TestAlpha => None,
    }
}
