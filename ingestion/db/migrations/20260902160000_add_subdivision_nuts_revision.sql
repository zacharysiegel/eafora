-- migrate:up

alter table subdivision
    add column if not exists nuts_revision integer
;

comment on column subdivision.nuts_revision is 'the revision of the NUTS classification the code in nuts_code belongs to, named by year as Eurostat names it (2016, 2021); a code names territory only within its revision, and a later revision may reassign it, so an observation resolves on the pair rather than the code';

update subdivision
set nuts_revision = 2021
where nuts_code is not null
;

alter table subdivision
    add check ((nuts_code is null) = (nuts_revision is null))
;

-- migrate:down

alter table subdivision
    drop column if exists nuts_revision
;
