-- migrate:up

insert into data_source (code, name_en, homepage_url, license_class, license_name, license_url, attribution_text, preference_rank) values
    ('hfd', 'Human Fertility Database', 'https://www.humanfertility.org/', 'attribution', 'CC BY 4.0', 'https://creativecommons.org/licenses/by/4.0/', 'Human Fertility Database. Max Planck Institute for Demographic Research (Germany) and Vienna Institute of Demography (Austria). Available at www.humanfertility.org (CC BY 4.0)', 50);

-- Left unreleased: the client cannot yet draw a cohort as a range, and a released statistic must have a StatisticKind variant.
insert into statistic (code, name_en, name_abbreviated_en, description, units) values
    ('ccf', 'Completed cohort fertility', 'CCF', 'Average number of children born to a woman of a given birth cohort by the end of her childbearing years. Unlike the total fertility rate, it counts births that actually occurred to a real cohort rather than combining one year''s age-specific rates into a hypothetical woman.', 'children per woman');

-- migrate:down

-- statistic_value references both statistic(id) and data_source(id), and data_source_publication
-- references data_source(id), none with ON DELETE CASCADE. Phase A accumulates values before the
-- statistic is released, so dependents are the normal state here rather than an edge case. Keyed on each
-- parent separately because a later source may supply ccf, and hfd may supply another statistic.
delete from statistic_value where statistic_id in (select id from statistic where code = 'ccf');
delete from statistic_value where data_source_id in (select id from data_source where code = 'hfd');
delete from source_choice   where statistic_id in (select id from statistic where code = 'ccf');
delete from source_choice   where data_source_id in (select id from data_source where code = 'hfd');
delete from data_source_publication where data_source_id in (select id from data_source where code = 'hfd');
delete from statistic   where code = 'ccf';
delete from data_source where code = 'hfd';
