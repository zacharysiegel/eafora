-- migrate:up

-- Rank 40 places Eurostat above HFD (50) and World Bank WDI (100), so it wins every cell it supplies.
-- Its data carries the Commission's reuse authorisation, which is not the licence covering the boundary
-- geometry Eurostat's GISCO service distributes; that one is restricted to non-commercial use.
insert into data_source (code, name_en, homepage_url, license_class, license_name, license_url, attribution_text, preference_rank) values
    ('eurostat', 'Eurostat', 'https://ec.europa.eu/eurostat', 'attribution', 'Commission Decision 2011/833/EU', 'https://ec.europa.eu/eurostat/web/main/help/copyright-notice', 'Eurostat (© European Union); reuse authorised under Commission Decision 2011/833/EU', 40);

-- Both are published to one decimal place and cover the EU, EFTA and candidate countries only.
insert into statistic (code, name_en, name_abbreviated_en, units) values
    ('mean_age_at_childbirth', 'Mean age of women at childbirth', 'MAC', 'years'),
    ('mean_age_at_first_birth', 'Mean age of women at first birth', 'MAFB', 'years');

-- The set of statuses lives in the DataStatus enum; enumerating it here would be a second copy that rots.
comment on column statistic_value.data_status is null;

-- Eurostat publishes no revision label; its JSON-stat responses carry an `updated` timestamp, which is what
-- the adapter records. The previous text named a week-numbered form Eurostat does not publish.
comment on column data_source_publication.revision_label is 'the source''s own revision label for this publication event (WB WDI ''2024-Q4'', HFD ''2025-12'', Eurostat the response''s `updated` timestamp); sources without native versioning get a synthesized label (response payload hash or fetch date); read before a fetch so an unchanged revision skips the write; aggregated per-source into the manifest at artifact-build time';

comment on column data_source.code is 'short identifier naming the publisher rather than one of its datasets (''wb_wdi'', ''hfd'', ''eurostat''), since preference_rank judges the publisher';

-- migrate:down

-- statistic_value references both statistic(id) and data_source(id), and data_source_publication
-- references data_source(id), none with ON DELETE CASCADE. Keyed on each parent separately because a later
-- source may supply either statistic, and Eurostat supplies tfr as well as the two seeded here.
delete from statistic_value where statistic_id in (select id from statistic where code in ('mean_age_at_childbirth', 'mean_age_at_first_birth'));
delete from statistic_value where data_source_id in (select id from data_source where code = 'eurostat');
delete from data_source_publication where data_source_id in (select id from data_source where code = 'eurostat');
delete from statistic where code in ('mean_age_at_childbirth', 'mean_age_at_first_birth');
delete from data_source where code = 'eurostat';

comment on column statistic_value.data_status is 'one of: final | provisional | preliminary | projection | imputed | interpolated';
comment on column data_source_publication.revision_label is 'the source''s own revision label for this publication event (WB WDI ''2024-Q4'', Eurostat ''2026-w20'', HFD ''2025-12'', WPP ''WPP-2024-rev1''); sources without native versioning get a synthesized label (response payload hash or fetch date); read by the adapter''s read_latest_publication_revision step for incremental fetches; aggregated per-source into the manifest''s data_source_revisions_jsonb at artifact-build time';
comment on column data_source.code is 'short identifier (''wb_wdi'', ''eurostat_demo_fer'', ''hfd'')';
