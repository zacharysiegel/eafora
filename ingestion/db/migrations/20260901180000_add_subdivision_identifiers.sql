-- migrate:up

-- The two schemes name the same territory with no computable relation between them: ISO numbers a country's
-- subdivisions alphabetically, NUTS by statistical grouping.
create table if not exists subdivision (
    region_id  uuid                     not null primary key references region (id),
    nuts_code  text                              unique,
    iso_3166_2 text                              unique,
    created    timestamp with time zone not null default now(),
    modified   timestamp with time zone not null default now()
);

comment on table  subdivision           is 'expected for every region below country level; nothing enforces it';
comment on column subdivision.nuts_code is 'identifies a territory only within one revision: NUTS is re-legislated periodically and codes are reused across revisions';

-- migrate:down

drop table if exists subdivision;
