-- migrate:up

alter table subdivision
    add column if not exists nuts_revision integer,
    add check ((nuts_code is null) = (nuts_revision is null))
;

comment on column subdivision.nuts_revision is 'the NUTS revision the code belongs to, named by year as Eurostat names it; an observation resolves on the (code, revision) pair';

-- migrate:down

alter table subdivision
    drop column if exists nuts_revision
;
