-- migrate:up

create table if not exists source_choice (
    id                  uuid                     not null default uuidv7() primary key,
    region_id           uuid                     references region (id),
    statistic_id        uuid                     not null references statistic (id),
    license_shard_class text                     not null,
    data_source_id      uuid                     not null references data_source (id),
    created             timestamp with time zone not null default now(),
    modified            timestamp with time zone not null default now()
);

-- one global default per (statistic, license_shard_class); null region_id = global
create unique index if not exists source_choice_global_uq
    on source_choice (statistic_id, license_shard_class)
    where region_id is null
;

-- at most one per-region override per (region, statistic, license_shard_class)
create unique index if not exists source_choice_override_uq
    on source_choice (region_id, statistic_id, license_shard_class)
    where region_id is not null
;

comment on column source_choice.region_id           is 'null = global default for this (statistic, license_shard_class); non-null = per-region override';
comment on column source_choice.license_shard_class is 'destination shard the chosen data_source contributes to; one of: base | share_alike | noncommercial';

insert into source_choice (statistic_id, license_shard_class, data_source_id)
select statistic.id, 'base', data_source.id
from statistic, data_source
where statistic.code = 'tfr'
  and data_source.code = 'wb_wdi'
;

-- migrate:down

drop table if exists source_choice;
