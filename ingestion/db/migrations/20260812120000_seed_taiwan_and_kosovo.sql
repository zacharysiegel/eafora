-- migrate:up

-- Taiwan and Kosovo are absent from the UN M49 / ISO 3166 CSV the initial seed is generated from: M49
-- folds Taiwan into China and leaves its region fields blank (so the seed generator skips it), and it
-- assigns Kosovo no code at all. Both are added here as country-level regions under their geographic
-- parent, with no m49_code. Kosovo keys on the World Bank's user-assigned 'XKX' so its indicator data
-- joins; Taiwan has no World Bank data.

insert into region (code, name_en, level, parent_region_id, m49_code) values
    ('twn', 'Taiwan', 'country', (select id from region where code = 'eastern_asia'),    null),
    ('xkx', 'Kosovo', 'country', (select id from region where code = 'southern_europe'), null);

insert into country (region_id, iso3, iso2) values
    ((select id from region where code = 'twn'), 'TWN', 'TW'),
    ((select id from region where code = 'xkx'), 'XKX', 'XK');

-- migrate:down

delete from country where iso3 in ('TWN', 'XKX');
delete from region  where code in ('twn', 'xkx');
