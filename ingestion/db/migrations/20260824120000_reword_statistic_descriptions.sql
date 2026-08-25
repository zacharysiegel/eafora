-- migrate:up

update statistic
set description = 'The average number of children which would be born to a woman over her lifetime if she experienced the current age-specific fertility rates throughout her reproductive years.'
where code = 'tfr'
;

update statistic
set description = 'The average number of children born to a woman of a given birth cohort by the end of her childbearing years. Unlike the total fertility rate, it counts births which actually occurred to a real cohort rather than combining one year''s age-specific rates into a hypothetical woman.'
where code = 'ccf'
;

-- migrate:down

update statistic
set description = 'Average number of children that would be born to a woman over her lifetime if she experienced the current age-specific fertility rates throughout her reproductive years.'
where code = 'tfr'
;

update statistic
set description = 'Average number of children born to a woman of a given birth cohort by the end of her childbearing years. Unlike the total fertility rate, it counts births that actually occurred to a real cohort rather than combining one year''s age-specific rates into a hypothetical woman.'
where code = 'ccf'
;
