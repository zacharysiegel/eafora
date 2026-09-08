-- migrate:up

-- An aggregated source names more than one rightsholder: EuroGlobalMap is assembled from national mapping
-- authorities and licensed collectively by EuroGeographics, while its British part carries a separate
-- Ordnance Survey credit under a different licence, so licence name and URL repeat per rightsholder.
create table if not exists data_source_attribution (
    id               uuid                     not null default uuidv7() primary key,
    data_source_id   uuid                     not null references data_source (id),
    position         integer                  not null, -- render order; the collective credit precedes a per-territory one
    attribution_text text                     not null,
    license_name     text                     not null,
    license_url      text                     not null,
    homepage_url     text                     not null,
    created          timestamp with time zone not null default now(),
    modified         timestamp with time zone not null default now(),
    unique (data_source_id, position)
);

comment on table  data_source_attribution                  is 'a source has at least one; nothing enforces it, and the artifact build fails on a source with none';
comment on column data_source_attribution.attribution_text is 'the exact string a consumer must display, rendered verbatim because the licence asks for that wording';

insert into data_source_attribution (data_source_id, position, attribution_text, license_name, license_url, homepage_url)
select data_source.id, 0, data_source.attribution_text, data_source.license_name, data_source.license_url, data_source.homepage_url
from data_source
;

alter table data_source
    drop column if exists attribution_text,
    drop column if exists license_name,
    drop column if exists license_url,
    drop column if exists homepage_url
;

-- migrate:down

alter table data_source
    add column if not exists attribution_text text not null default '',
    add column if not exists license_name     text not null default '',
    add column if not exists license_url      text not null default '',
    add column if not exists homepage_url     text not null default ''
;

update data_source
set attribution_text = data_source_attribution.attribution_text,
    license_name     = data_source_attribution.license_name,
    license_url      = data_source_attribution.license_url,
    homepage_url     = data_source_attribution.homepage_url
from data_source_attribution
where data_source_attribution.data_source_id = data_source.id
  and data_source_attribution.position = 0
;

drop table if exists data_source_attribution;
