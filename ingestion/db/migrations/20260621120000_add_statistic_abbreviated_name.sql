-- migrate:up

alter table statistic
    add column if not exists name_abbreviated_en text;

update statistic
    set name_en = 'Total fertility rate',
        name_abbreviated_en = 'TFR'
    where code = 'tfr';

alter table statistic
    alter column name_abbreviated_en set not null;

comment on column statistic.name_abbreviated_en is 'Short English label (often an acronym) for space-constrained UI like breadcrumbs; name_en remains the long form.';

-- migrate:down

alter table statistic
    drop column if exists name_abbreviated_en;

update statistic
    set name_en = 'Total Fertility Rate'
    where code = 'tfr';
