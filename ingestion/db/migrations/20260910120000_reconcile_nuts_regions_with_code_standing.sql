-- migrate:up

-- Eurostat keeps disseminating a code after superseding it, and gives the replacement the retired code's name,
-- so the seed took NL31 Utrecht and NL35 Utrecht for two regions. The codelist's IS_STANDARD_CODE annotation
-- is what separates them, and this brings the store to what that annotation says: the superseded codes go, the
-- codes Eurostat marks only because the United Kingdom left the classification stay, and the seven Northern
-- Irish districts and two Dorset ones the vintage filter dropped are added.
-- A revision marker appears on a label only once that revision has retired the code, so an unmarked code
-- belongs to the cut in force rather than to the newest revision any label names.

update subdivision
set nuts_revision = 2024
where nuts_code not like 'UK%'
;

delete from statistic_value
where region_id in (select id from region where code in (
    'deg04', 'deg0b', 'deg0f', 'deg0h', 'deg0i', 'deg0n', 'deg0p', 'fi193',
    'fi194', 'fi195', 'fi197', 'fi1c3', 'fi1c4', 'fi1d1', 'fi1d2', 'fi1d3',
    'lv003', 'lv006', 'lv007', 'lv008', 'nl111', 'nl113', 'nl124', 'nl125',
    'nl31', 'nl310', 'nl324', 'nl329', 'nl33', 'nl332', 'nl333', 'nl337',
    'nl33a', 'nl33b', 'nl33c', 'nl412', 'nl413', 'no074', 'no082', 'no091',
    'pt16', 'pt16b', 'pt16d', 'pt16e', 'pt16f', 'pt16g', 'pt16h', 'pt16i',
    'pt16j', 'pt17', 'pt170', 'pt18', 'pt181', 'pt184', 'pt185', 'pt186',
    'pt187'
))
;

delete from subdivision
where region_id in (select id from region where code in (
    'deg04', 'deg0b', 'deg0f', 'deg0h', 'deg0i', 'deg0n', 'deg0p', 'fi193',
    'fi194', 'fi195', 'fi197', 'fi1c3', 'fi1c4', 'fi1d1', 'fi1d2', 'fi1d3',
    'lv003', 'lv006', 'lv007', 'lv008', 'nl111', 'nl113', 'nl124', 'nl125',
    'nl31', 'nl310', 'nl324', 'nl329', 'nl33', 'nl332', 'nl333', 'nl337',
    'nl33a', 'nl33b', 'nl33c', 'nl412', 'nl413', 'no074', 'no082', 'no091',
    'pt16', 'pt16b', 'pt16d', 'pt16e', 'pt16f', 'pt16g', 'pt16h', 'pt16i',
    'pt16j', 'pt17', 'pt170', 'pt18', 'pt181', 'pt184', 'pt185', 'pt186',
    'pt187'
))
;

delete from region
where code in (
    'deg04', 'deg0b', 'deg0f', 'deg0h', 'deg0i', 'deg0n', 'deg0p', 'fi193',
    'fi194', 'fi195', 'fi197', 'fi1c3', 'fi1c4', 'fi1d1', 'fi1d2', 'fi1d3',
    'lv003', 'lv006', 'lv007', 'lv008', 'nl111', 'nl113', 'nl124', 'nl125',
    'nl31', 'nl310', 'nl324', 'nl329', 'nl33', 'nl332', 'nl333', 'nl337',
    'nl33a', 'nl33b', 'nl33c', 'nl412', 'nl413', 'no074', 'no082', 'no091',
    'pt16', 'pt16b', 'pt16d', 'pt16e', 'pt16f', 'pt16g', 'pt16h', 'pt16i',
    'pt16j', 'pt17', 'pt170', 'pt18', 'pt181', 'pt184', 'pt185', 'pt186',
    'pt187'
)
;

with subnational_3_seed (code, name_en, parent_code, nuts_revision) as (
    values
        ('ukk21', 'Bournemouth and Poole', 'ukk2', 2016),
        ('ukk22', 'Dorset CC', 'ukk2', 2016),
        ('ukn10', 'Derry City and Strabane', 'ukn0', 2016),
        ('ukn11', 'Mid Ulster', 'ukn0', 2016),
        ('ukn12', 'Causeway Coast and Glens', 'ukn0', 2016),
        ('ukn13', 'Antrim and Newtownabbey', 'ukn0', 2016),
        ('ukn14', 'Lisburn and Castlereagh', 'ukn0', 2016),
        ('ukn15', 'Mid and East Antrim', 'ukn0', 2016),
        ('ukn16', 'Fermanagh and Omagh', 'ukn0', 2016)
),
subnational_3 as (
    insert into region (code, name_en, level, parent_region_id)
    select subnational_3_seed.code, subnational_3_seed.name_en, 'subnational_3', parent.id
    from subnational_3_seed
    join region as parent on parent.code = subnational_3_seed.parent_code
    returning id, code
)
insert into subdivision (region_id, nuts_code, nuts_revision)
select subnational_3.id, upper(subnational_3.code), subnational_3_seed.nuts_revision
from subnational_3
join subnational_3_seed on subnational_3_seed.code = subnational_3.code
;

-- migrate:down

-- The values that hung off the superseded regions are not restored with them; the next Eurostat run reinstates
-- them from the source.

delete from statistic_value
where region_id in (select id from region where code in (
    'ukk21', 'ukk22', 'ukn10', 'ukn11', 'ukn12', 'ukn13', 'ukn14', 'ukn15',
    'ukn16'
))
;

delete from subdivision
where region_id in (select id from region where code in (
    'ukk21', 'ukk22', 'ukn10', 'ukn11', 'ukn12', 'ukn13', 'ukn14', 'ukn15',
    'ukn16'
))
;

delete from region
where code in (
    'ukk21', 'ukk22', 'ukn10', 'ukn11', 'ukn12', 'ukn13', 'ukn14', 'ukn15',
    'ukn16'
)
;

with subnational_2_seed (code, name_en, parent_code, nuts_revision) as (
    values
        ('nl31', 'Utrecht', 'nl3', 2021),
        ('nl33', 'Zuid-Holland', 'nl3', 2021),
        ('pt16', 'Centro (PT)', 'pt1', 2021),
        ('pt17', 'Área Metropolitana de Lisboa', 'pt1', 2021),
        ('pt18', 'Alentejo', 'pt1', 2021)
),
subnational_2 as (
    insert into region (code, name_en, level, parent_region_id)
    select subnational_2_seed.code, subnational_2_seed.name_en, 'subnational_2', parent.id
    from subnational_2_seed
    join region as parent on parent.code = subnational_2_seed.parent_code
    returning id, code
)
insert into subdivision (region_id, nuts_code, nuts_revision)
select subnational_2.id, upper(subnational_2.code), subnational_2_seed.nuts_revision
from subnational_2
join subnational_2_seed on subnational_2_seed.code = subnational_2.code
;

with subnational_3_seed (code, name_en, parent_code, nuts_revision) as (
    values
        ('deg04', 'Suhl, Kreisfreie Stadt', 'deg0', 2021),
        ('deg0b', 'Schmalkalden-Meiningen', 'deg0', 2021),
        ('deg0f', 'Ilm-Kreis', 'deg0', 2021),
        ('deg0h', 'Sonneberg', 'deg0', 2021),
        ('deg0i', 'Saalfeld-Rudolstadt', 'deg0', 2021),
        ('deg0n', 'Eisenach, Kreisfreie Stadt', 'deg0', 2021),
        ('deg0p', 'Wartburgkreis', 'deg0', 2021),
        ('fi193', 'Keski-Suomi', 'fi19', 2021),
        ('fi194', 'Etelä-Pohjanmaa', 'fi19', 2021),
        ('fi195', 'Pohjanmaa', 'fi19', 2021),
        ('fi197', 'Pirkanmaa', 'fi19', 2021),
        ('fi1c3', 'Päijät-Häme', 'fi1c', 2021),
        ('fi1c4', 'Kymenlaakso', 'fi1c', 2021),
        ('fi1d1', 'Etelä-Savo', 'fi1d', 2021),
        ('fi1d2', 'Pohjois-Savo', 'fi1d', 2021),
        ('fi1d3', 'Pohjois-Karjala', 'fi1d', 2021),
        ('lv003', 'Kurzeme', 'lv00', 2021),
        ('lv006', 'Rīga', 'lv00', 2021),
        ('lv007', 'Pieriga', 'lv00', 2021),
        ('lv008', 'Vidzeme', 'lv00', 2021),
        ('nl111', 'Oost-Groningen', 'nl11', 2021),
        ('nl113', 'Overig Groningen', 'nl11', 2021),
        ('nl124', 'Noord-Friesland', 'nl12', 2021),
        ('nl125', 'Zuidwest-Friesland', 'nl12', 2021),
        ('nl310', 'Utrecht', 'nl31', 2021),
        ('nl324', 'Agglomeratie Haarlem', 'nl32', 2021),
        ('nl329', 'Groot-Amsterdam', 'nl32', 2021),
        ('nl332', 'Agglomeratie ''s-Gravenhage', 'nl33', 2021),
        ('nl333', 'Delft en Westland', 'nl33', 2021),
        ('nl337', 'Agglomeratie Leiden en Bollenstreek', 'nl33', 2021),
        ('nl33a', 'Zuidoost-Zuid-Holland', 'nl33', 2021),
        ('nl33b', 'Oost-Zuid-Holland', 'nl33', 2021),
        ('nl33c', 'Groot-Rijnmond', 'nl33', 2021),
        ('nl412', 'Midden-Noord-Brabant', 'nl41', 2021),
        ('nl413', 'Noordoost-Noord-Brabant', 'nl41', 2021),
        ('no074', 'Troms og Finnmark/Romsa ja Finnmárku/Tromssa ja Finmarkku', 'no07', 2021),
        ('no082', 'Viken', 'no08', 2021),
        ('no091', 'Vestfold og Telemark', 'no09', 2021),
        ('pt16b', 'Oeste', 'pt16', 2021),
        ('pt16d', 'Região de Aveiro', 'pt16', 2021),
        ('pt16e', 'Região de Coimbra', 'pt16', 2021),
        ('pt16f', 'Região de Leiria', 'pt16', 2021),
        ('pt16g', 'Viseu Dão Lafões', 'pt16', 2021),
        ('pt16h', 'Beira Baixa', 'pt16', 2021),
        ('pt16i', 'Médio Tejo', 'pt16', 2021),
        ('pt16j', 'Beiras e Serra da Estrela', 'pt16', 2021),
        ('pt170', 'Área Metropolitana de Lisboa', 'pt17', 2021),
        ('pt181', 'Alentejo Litoral', 'pt18', 2021),
        ('pt184', 'Baixo Alentejo', 'pt18', 2021),
        ('pt185', 'Lezíria do Tejo', 'pt18', 2021),
        ('pt186', 'Alto Alentejo', 'pt18', 2021),
        ('pt187', 'Alentejo Central', 'pt18', 2021)
),
subnational_3 as (
    insert into region (code, name_en, level, parent_region_id)
    select subnational_3_seed.code, subnational_3_seed.name_en, 'subnational_3', parent.id
    from subnational_3_seed
    join region as parent on parent.code = subnational_3_seed.parent_code
    returning id, code
)
insert into subdivision (region_id, nuts_code, nuts_revision)
select subnational_3.id, upper(subnational_3.code), subnational_3_seed.nuts_revision
from subnational_3
join subnational_3_seed on subnational_3_seed.code = subnational_3.code
;

update subdivision
set nuts_revision = 2021
where nuts_code not like 'UK%'
;
