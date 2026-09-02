-- migrate:up

-- The 1:1 extension shape country already uses, for the identifiers only a subdivision has. A NUTS code and
-- an ISO 3166-2 code name the same territory under two schemes with no computable relation between them:
-- ISO numbers a country's subdivisions alphabetically, NUTS by statistical grouping.
create table if not exists subdivision (
    region_id  uuid                     not null primary key references region (id),
    nuts_code  text                              unique,
    iso_3166_2 text                              unique,
    created    timestamp with time zone not null default now(),
    modified   timestamp with time zone not null default now()
);

comment on table  subdivision            is 'strict 1:1 extension of region rows below country level, holding the external identifier schemes only a subdivision has';
comment on column subdivision.region_id  is 'both PK and FK to region.id, enforcing the 1:1 extension shape country uses';
comment on column subdivision.nuts_code  is 'Eurostat NUTS code (''DE11'', ''TR100''), which identifies a territory only within one revision of the classification: NUTS is re-legislated periodically and a code can be reused for different territory across revisions';
comment on column subdivision.iso_3166_2 is 'ISO 3166-2 subdivision code (''TR-34''), the scheme boundary sources key subdivisions on';

-- migrate:down

drop table if exists subdivision;
