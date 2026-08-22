-- migrate:up

insert into source_choice (statistic_id, license_shard_class, data_source_id)
select statistic.id, 'base', data_source.id
from statistic, data_source
where statistic.code = 'ccf'
  and data_source.code = 'hfd'
;

-- migrate:down

delete from source_choice
where statistic_id in (select id from statistic where code = 'ccf')
  and data_source_id in (select id from data_source where code = 'hfd')
;
