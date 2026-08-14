-- migrate:up

-- The World aggregate (UN M49 code 001) is a supranational region with no country extension and no
-- geometry. It is standalone (parent_region_id null), not wired as the parent of the five M49 top-level
-- regions. World Bank WDI publishes its per-year figure under countryiso3code 'WLD'; the WDI adapter
-- resolves that code to this region so the value lands as an ordinary statistic_value.
insert into region (code, name_en, level, parent_region_id, m49_code) values
    ('world', 'World', 'world', null, '001');

-- migrate:down

-- statistic_value and source_choice reference region(id) without ON DELETE CASCADE, and the world
-- region accumulates World Bank values, so clear dependents before removing the region.
delete from statistic_value where region_id in (select id from region where code = 'world');
delete from source_choice   where region_id in (select id from region where code = 'world');
delete from region  where code = 'world';
