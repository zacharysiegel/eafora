-- migrate:up

-- Single migration for the initial canonical-store schema covering all seven
-- tables: region, country, statistic, data_source, data_source_publication,
-- statistic_value, artifact_version. See docs/architecture/ingestion.md for
-- the full design rationale; this file is the executable form of that design.
--
-- Postgres 18+ required (uuidv7() is the default for every primary key).

create table if not exists region (
    id               uuid                     not null default uuidv7() primary key,
    code             text                     not null unique,
    name_en          text                     not null,
    level            text                     not null,
    parent_region_id uuid                              references region (id),
    m49_code         text                              unique, -- text vs int leaves room for a non-M49 taxonomy if §Boundary recognition's alt-taxonomy clause is exercised; nullable to accommodate future subnational levels that have no M49 equivalent
    created          timestamp with time zone not null default now(),
    modified         timestamp with time zone not null default now()
);

comment on column region.code             is 'human-readable slug (''americas'', ''south_america'', ''sub_saharan_africa'', ''usa'', ''germany'')';
comment on column region.level            is '''region'' | ''subregion'' | ''intermediate_region'' | ''country'' | (future subnational levels: ''subnational_1'', ''subnational_2'', ...)';
comment on column region.parent_region_id is 'null only for top-level region nodes (Africa, Americas, Asia, Europe, Oceania); every other row including countries has a parent';
comment on column region.m49_code         is 'UN M49 numeric code as text (preserves leading zeros like ''021''); also populated for country-level rows (USA=''840'', DEU=''276''); nullable for future non-M49 levels (subnational) that have no M49 equivalent';

create table if not exists country (
    region_id uuid                     not null primary key references region (id),
    iso3      text                     not null unique,
    iso2      text                     not null unique,
    created   timestamp with time zone not null default now(),
    modified  timestamp with time zone not null default now(),
    deleted   timestamp with time zone
);

comment on column country.region_id is 'both PK and FK to region.id; enforces the strict 1:1 extension shape (every country row corresponds to exactly one region row at level=''country'', and vice versa)';
comment on column country.iso3      is 'ISO 3166-1 alpha-3 (''USA'', ''DEU'', ''JPN'')';
comment on column country.iso2      is 'ISO 3166-1 alpha-2 (''US'', ''DE'', ''JP'')';

create table if not exists statistic (
    id          uuid                     not null default uuidv7() primary key,
    code        text                     not null unique,
    name_en     text                     not null,
    description text                     not null,
    units       text                     not null,
    created     timestamp with time zone not null default now(),
    modified    timestamp with time zone not null default now()
);

comment on column statistic.code is 'short identifier used downstream (''tfr'', ''cbr'', ''asfr_15_19''); stable across versions, renaming is a migration event';

create table if not exists data_source (
    id               uuid                     not null default uuidv7() primary key,
    code             text                     not null unique,
    name_en          text                     not null,
    homepage_url     text                     not null,
    license_class    text                     not null,
    license_name     text                     not null,
    license_url      text                     not null,
    attribution_text text                     not null,
    preference_rank  int                      not null,
    created          timestamp with time zone not null default now(),
    modified         timestamp with time zone not null default now()
);

comment on column data_source.code             is 'short identifier (''wb_wdi'', ''eurostat_demo_fer'', ''hfd'')';
comment on column data_source.license_class    is 'one of: public_domain | attribution | attribution_share_alike | noncommercial';
comment on column data_source.license_name     is 'e.g. ''CC BY 4.0'', ''Open Government Licence v3.0''';
comment on column data_source.attribution_text is 'exact display string for UI citations';
comment on column data_source.preference_rank  is 'drives data-source-preference merge; lower wins; ties broken deterministically by data_source.id';

create table if not exists data_source_publication (
    id             uuid                     not null default uuidv7() primary key,
    data_source_id uuid                     not null references data_source (id),
    revision_label text                     not null,
    published      timestamp with time zone,
    fetched        timestamp with time zone not null,
    created        timestamp with time zone not null default now(),
    modified       timestamp with time zone not null default now(),
    unique (data_source_id, revision_label)
);

comment on column data_source_publication.revision_label is 'the source''s own revision label for this publication event (WB WDI ''2024-Q4'', Eurostat ''2026-w20'', HFD ''2025-12'', WPP ''WPP-2024-rev1''); sources without native versioning get a synthesized label (response payload hash or fetch date); read by the adapter''s read_latest_publication_revision step for incremental fetches; aggregated per-source into the manifest''s data_source_versions_jsonb at artifact-build time';
comment on column data_source_publication.published      is 'source''s own publication timestamp where derivable (often only a year or version label, hence nullable)';
comment on column data_source_publication.fetched        is 'wall-clock instant our adapter captured this publication';

create table if not exists statistic_value (
    id                         uuid                     not null default uuidv7() primary key,
    region_id                  uuid                     not null references region (id),
    statistic_id               uuid                     not null references statistic (id),
    period_start               date                     not null,
    period_end                 date                     not null,
    value                      double precision         not null,
    data_source_id             uuid                     not null references data_source (id),
    data_source_publication_id uuid                     not null references data_source_publication (id),
    data_status                text                     not null,
    superseded                 timestamp with time zone,
    created                    timestamp with time zone not null default now(),
    modified                   timestamp with time zone not null default now(),
    unique (region_id, statistic_id, period_start, period_end, data_source_publication_id)
);

create unique index if not exists statistic_value_current_per_source
    on statistic_value (region_id, statistic_id, period_start, period_end, data_source_id)
    where superseded is null
;

comment on column statistic_value.region_id                  is 'points at any level — country (common in v1), subnational (v2+ when subnational data lands), or supranational grouping (for stored aggregates)';
comment on column statistic_value.period_start               is 'inclusive lower bound: calendar year 2024 → ''2024-01-01''; Q1 2024 → ''2024-01-01''; 2020-2025 cohort → ''2020-01-01''';
comment on column statistic_value.period_end                 is 'exclusive upper bound: calendar year 2024 → ''2025-01-01''; Q1 2024 → ''2024-04-01''; 2020-2025 cohort → ''2025-01-01''';
comment on column statistic_value.data_source_id             is 'denormalized from data_source_publication.data_source_id; needed for the partial unique index that enforces ''at most one current row per cell per source''; the upsert path keeps the two in sync';
comment on column statistic_value.data_source_publication_id is 'points at the publication event this row''s value was captured from; the row is never updated to point elsewhere — when the source revises, a NEW row is inserted with the new publication, and this row''s superseded timestamp is set';
comment on column statistic_value.data_status                is 'one of: final | provisional | preliminary | projection | imputed | interpolated';
comment on column statistic_value.superseded                 is 'wall-clock instant when this row stopped being the current view of its (region, statistic, period, data_source_id) cell — i.e., when a newer publication for the same source produced a different value, this row got marked as historical. NULL means current (the row reflects the latest publication''s view of the cell)';

create table if not exists artifact_version (
    id                         uuid                     not null default uuidv7() primary key,
    version_label              text                     not null unique,
    artifact_created           timestamp with time zone not null default now(),
    manifest_sha256            text                     not null,
    manifest_url               text                     not null,
    data_source_versions_jsonb jsonb                    not null,
    notes                      text
);

comment on column artifact_version.version_label              is 'ISO date of the scheduled build (e.g. ''2026-05-18''); disambiguating suffix added if two builds land the same day';
comment on column artifact_version.manifest_sha256            is 'content hash of manifest.json';
comment on column artifact_version.manifest_url               is 'CDN URL of manifest.json';
comment on column artifact_version.data_source_versions_jsonb is 'snapshot of every data_source''s data_source_revision at build time: {"wb_wdi": "2024-Q4", "hfd": "2025-12"}; used to attribute artifact contents to upstream snapshots and to let clients detect when re-fetching is worthwhile';

-- migrate:down

drop table if exists artifact_version;
drop table if exists statistic_value;
drop table if exists data_source_publication;
drop table if exists data_source;
drop table if exists statistic;
drop table if exists country;
drop table if exists region;
