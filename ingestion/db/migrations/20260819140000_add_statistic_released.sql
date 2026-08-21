-- migrate:up

alter table statistic
    add column if not exists released timestamp with time zone;

comment on column statistic.released is 'When the statistic began being offered to clients; null means it is ingested but not yet offered.';

update statistic
    set released = now()
    where code = 'tfr';

-- migrate:down

alter table statistic
    drop column if exists released;
