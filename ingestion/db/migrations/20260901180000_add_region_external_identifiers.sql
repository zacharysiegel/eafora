-- migrate:up

alter table region
    add column if not exists nuts_code text unique,
    add column if not exists iso_3166_2 text unique;

comment on column region.nuts_code  is 'Eurostat NUTS code (''DE11'', ''TR100''), which identifies a region only within one revision of the classification: NUTS is re-legislated every few years and a code can be reused for different territory across revisions, so a code is meaningful only alongside the vintage it was seeded from';
comment on column region.iso_3166_2 is 'ISO 3166-2 subdivision code (''TR-34''); the scheme boundary geometry sources key subnational units on, and unrelated to nuts_code, which numbers by statistical grouping rather than alphabetically';

-- migrate:down

alter table region drop column if exists nuts_code;
alter table region drop column if exists iso_3166_2;
