\restrict dbmate

-- Dumped from database version 18.4 (Homebrew)
-- Dumped by pg_dump version 18.4 (Homebrew)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: artifact_version; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.artifact_version (
    id uuid DEFAULT uuidv7() NOT NULL,
    version_label text NOT NULL,
    artifact_created timestamp with time zone DEFAULT now() NOT NULL,
    manifest_sha256 text NOT NULL,
    manifest_url text NOT NULL,
    data_source_revisions_jsonb jsonb NOT NULL,
    notes text
);


--
-- Name: COLUMN artifact_version.version_label; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.artifact_version.version_label IS 'ISO date of the scheduled build (e.g. ''2026-05-18''); disambiguating suffix added if two builds land the same day';


--
-- Name: COLUMN artifact_version.manifest_sha256; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.artifact_version.manifest_sha256 IS 'content hash of manifest.json';


--
-- Name: COLUMN artifact_version.manifest_url; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.artifact_version.manifest_url IS 'CDN URL of manifest.json';


--
-- Name: COLUMN artifact_version.data_source_revisions_jsonb; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.artifact_version.data_source_revisions_jsonb IS 'snapshot of every data_source''s latest publication at build time, keyed by data_source.code: {"wb_wdi": {"revision": "2024-Q4", "fetched": "2026-05-26T03:00:00Z"}, "hfd": {"revision": "2025-12", "fetched": "2026-05-26T03:00:00Z"}}; revision is the source''s own label, fetched is when ingestion captured the publication; used to attribute artifact contents to upstream snapshots and to let clients detect when re-fetching is worthwhile';


--
-- Name: country; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.country (
    region_id uuid NOT NULL,
    iso3 text NOT NULL,
    iso2 text NOT NULL,
    created timestamp with time zone DEFAULT now() NOT NULL,
    modified timestamp with time zone DEFAULT now() NOT NULL,
    deleted timestamp with time zone
);


--
-- Name: COLUMN country.region_id; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.country.region_id IS 'both PK and FK to region.id; enforces the strict 1:1 extension shape (every country row corresponds to exactly one region row at level=''country'', and vice versa)';


--
-- Name: COLUMN country.iso3; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.country.iso3 IS 'ISO 3166-1 alpha-3 (''USA'', ''DEU'', ''JPN'')';


--
-- Name: COLUMN country.iso2; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.country.iso2 IS 'ISO 3166-1 alpha-2 (''US'', ''DE'', ''JP'')';


--
-- Name: data_source; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.data_source (
    id uuid DEFAULT uuidv7() NOT NULL,
    code text NOT NULL,
    name_en text NOT NULL,
    homepage_url text NOT NULL,
    license_class text NOT NULL,
    license_name text NOT NULL,
    license_url text NOT NULL,
    attribution_text text NOT NULL,
    preference_rank integer NOT NULL,
    created timestamp with time zone DEFAULT now() NOT NULL,
    modified timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: COLUMN data_source.code; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.data_source.code IS 'short identifier naming the publisher rather than one of its datasets (''wb_wdi'', ''hfd'', ''eurostat''), since preference_rank judges the publisher';


--
-- Name: COLUMN data_source.license_class; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.data_source.license_class IS 'one of: public_domain | attribution | attribution_share_alike | noncommercial';


--
-- Name: COLUMN data_source.license_name; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.data_source.license_name IS 'e.g. ''CC BY 4.0'', ''Open Government Licence v3.0''';


--
-- Name: COLUMN data_source.attribution_text; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.data_source.attribution_text IS 'exact display string for UI citations';


--
-- Name: COLUMN data_source.preference_rank; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.data_source.preference_rank IS 'orders sources when more than one supplies a cell; lower wins, ties broken deterministically by data_source.code';


--
-- Name: data_source_publication; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.data_source_publication (
    id uuid DEFAULT uuidv7() NOT NULL,
    data_source_id uuid NOT NULL,
    revision_label text NOT NULL,
    published timestamp with time zone,
    fetched timestamp with time zone NOT NULL,
    created timestamp with time zone DEFAULT now() NOT NULL,
    modified timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: COLUMN data_source_publication.revision_label; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.data_source_publication.revision_label IS 'the source''s own revision label for this publication event (WB WDI ''2024-Q4'', HFD ''2025-12'', Eurostat the response''s `updated` timestamp); sources without native versioning get a synthesized label (response payload hash or fetch date); read before a fetch so an unchanged revision skips the write; aggregated per-source into the manifest at artifact-build time';


--
-- Name: COLUMN data_source_publication.published; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.data_source_publication.published IS 'source''s own publication timestamp where derivable (often only a year or version label, hence nullable)';


--
-- Name: COLUMN data_source_publication.fetched; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.data_source_publication.fetched IS 'wall-clock instant our adapter captured this publication';


--
-- Name: region; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.region (
    id uuid DEFAULT uuidv7() NOT NULL,
    code text NOT NULL,
    name_en text NOT NULL,
    level text NOT NULL,
    parent_region_id uuid,
    m49_code text,
    created timestamp with time zone DEFAULT now() NOT NULL,
    modified timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: COLUMN region.code; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.region.code IS 'human-readable slug (''americas'', ''south_america'', ''sub_saharan_africa'', ''usa'', ''germany'')';


--
-- Name: COLUMN region.level; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.region.level IS '''region'' | ''subregion'' | ''intermediate_region'' | ''country'' | (future subnational levels: ''subnational_1'', ''subnational_2'', ...)';


--
-- Name: COLUMN region.parent_region_id; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.region.parent_region_id IS 'null only for top-level region nodes (Africa, Americas, Asia, Europe, Oceania); every other row including countries has a parent';


--
-- Name: COLUMN region.m49_code; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.region.m49_code IS 'UN M49 numeric code as text (preserves leading zeros like ''021''); also populated for country-level rows (USA=''840'', DEU=''276''); nullable for future non-M49 levels (subnational) that have no M49 equivalent';


--
-- Name: schema_migrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.schema_migrations (
    version character varying NOT NULL
);


--
-- Name: statistic; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.statistic (
    id uuid DEFAULT uuidv7() NOT NULL,
    code text NOT NULL,
    name_en text NOT NULL,
    units text NOT NULL,
    created timestamp with time zone DEFAULT now() NOT NULL,
    modified timestamp with time zone DEFAULT now() NOT NULL,
    name_abbreviated_en text NOT NULL
);


--
-- Name: COLUMN statistic.code; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.statistic.code IS 'short identifier used downstream (''tfr'', ''cbr'', ''asfr_15_19''); stable across versions, renaming is a migration event';


--
-- Name: COLUMN statistic.name_abbreviated_en; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.statistic.name_abbreviated_en IS 'Short English label (often an acronym) for space-constrained UI like breadcrumbs; name_en remains the long form.';


--
-- Name: statistic_value; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.statistic_value (
    id uuid DEFAULT uuidv7() NOT NULL,
    region_id uuid NOT NULL,
    statistic_id uuid NOT NULL,
    period_start date NOT NULL,
    period_end date NOT NULL,
    value double precision NOT NULL,
    data_source_id uuid NOT NULL,
    data_source_publication_id uuid NOT NULL,
    data_status text NOT NULL,
    superseded timestamp with time zone,
    created timestamp with time zone DEFAULT now() NOT NULL,
    modified timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: COLUMN statistic_value.region_id; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.statistic_value.region_id IS 'points at any level — country (common in v1), subnational (v2+ when subnational data lands), or supranational grouping (for stored aggregates)';


--
-- Name: COLUMN statistic_value.period_start; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.statistic_value.period_start IS 'inclusive lower bound: calendar year 2024 → ''2024-01-01''; Q1 2024 → ''2024-01-01''; 2020-2025 cohort → ''2020-01-01''';


--
-- Name: COLUMN statistic_value.period_end; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.statistic_value.period_end IS 'exclusive upper bound: calendar year 2024 → ''2025-01-01''; Q1 2024 → ''2024-04-01''; 2020-2025 cohort → ''2025-01-01''';


--
-- Name: COLUMN statistic_value.data_source_id; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.statistic_value.data_source_id IS 'denormalized from data_source_publication.data_source_id; needed for the partial unique index that enforces ''at most one current row per cell per source''; the upsert path keeps the two in sync';


--
-- Name: COLUMN statistic_value.data_source_publication_id; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.statistic_value.data_source_publication_id IS 'points at the publication event this row''s value was captured from; the row is never updated to point elsewhere — when the source revises, a NEW row is inserted with the new publication, and this row''s superseded timestamp is set';


--
-- Name: COLUMN statistic_value.superseded; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.statistic_value.superseded IS 'wall-clock instant when this row stopped being the current view of its (region, statistic, period, data_source_id) cell — i.e., when a newer publication for the same source produced a different value, this row got marked as historical. NULL means current (the row reflects the latest publication''s view of the cell)';


--
-- Name: subdivision; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.subdivision (
    region_id uuid NOT NULL,
    nuts_code text,
    iso_3166_2 text,
    created timestamp with time zone DEFAULT now() NOT NULL,
    modified timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: TABLE subdivision; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.subdivision IS 'expected for every region below country level; nothing enforces it';


--
-- Name: COLUMN subdivision.nuts_code; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.subdivision.nuts_code IS 'identifies a territory only within one revision: NUTS is re-legislated periodically and codes are reused across revisions';


--
-- Name: artifact_version artifact_version_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.artifact_version
    ADD CONSTRAINT artifact_version_pkey PRIMARY KEY (id);


--
-- Name: artifact_version artifact_version_version_label_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.artifact_version
    ADD CONSTRAINT artifact_version_version_label_key UNIQUE (version_label);


--
-- Name: country country_iso2_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.country
    ADD CONSTRAINT country_iso2_key UNIQUE (iso2);


--
-- Name: country country_iso3_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.country
    ADD CONSTRAINT country_iso3_key UNIQUE (iso3);


--
-- Name: country country_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.country
    ADD CONSTRAINT country_pkey PRIMARY KEY (region_id);


--
-- Name: data_source data_source_code_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_source
    ADD CONSTRAINT data_source_code_key UNIQUE (code);


--
-- Name: data_source data_source_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_source
    ADD CONSTRAINT data_source_pkey PRIMARY KEY (id);


--
-- Name: data_source_publication data_source_publication_data_source_id_revision_label_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_source_publication
    ADD CONSTRAINT data_source_publication_data_source_id_revision_label_key UNIQUE (data_source_id, revision_label);


--
-- Name: data_source_publication data_source_publication_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_source_publication
    ADD CONSTRAINT data_source_publication_pkey PRIMARY KEY (id);


--
-- Name: region region_code_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.region
    ADD CONSTRAINT region_code_key UNIQUE (code);


--
-- Name: region region_m49_code_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.region
    ADD CONSTRAINT region_m49_code_key UNIQUE (m49_code);


--
-- Name: region region_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.region
    ADD CONSTRAINT region_pkey PRIMARY KEY (id);


--
-- Name: schema_migrations schema_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schema_migrations
    ADD CONSTRAINT schema_migrations_pkey PRIMARY KEY (version);


--
-- Name: statistic statistic_code_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.statistic
    ADD CONSTRAINT statistic_code_key UNIQUE (code);


--
-- Name: statistic statistic_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.statistic
    ADD CONSTRAINT statistic_pkey PRIMARY KEY (id);


--
-- Name: statistic_value statistic_value_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.statistic_value
    ADD CONSTRAINT statistic_value_pkey PRIMARY KEY (id);


--
-- Name: statistic_value statistic_value_region_id_statistic_id_period_start_period__key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.statistic_value
    ADD CONSTRAINT statistic_value_region_id_statistic_id_period_start_period__key UNIQUE (region_id, statistic_id, period_start, period_end, data_source_publication_id);


--
-- Name: subdivision subdivision_iso_3166_2_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.subdivision
    ADD CONSTRAINT subdivision_iso_3166_2_key UNIQUE (iso_3166_2);


--
-- Name: subdivision subdivision_nuts_code_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.subdivision
    ADD CONSTRAINT subdivision_nuts_code_key UNIQUE (nuts_code);


--
-- Name: subdivision subdivision_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.subdivision
    ADD CONSTRAINT subdivision_pkey PRIMARY KEY (region_id);


--
-- Name: statistic_value_current_per_source; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX statistic_value_current_per_source ON public.statistic_value USING btree (region_id, statistic_id, period_start, period_end, data_source_id) WHERE (superseded IS NULL);


--
-- Name: country country_region_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.country
    ADD CONSTRAINT country_region_id_fkey FOREIGN KEY (region_id) REFERENCES public.region(id);


--
-- Name: data_source_publication data_source_publication_data_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_source_publication
    ADD CONSTRAINT data_source_publication_data_source_id_fkey FOREIGN KEY (data_source_id) REFERENCES public.data_source(id);


--
-- Name: region region_parent_region_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.region
    ADD CONSTRAINT region_parent_region_id_fkey FOREIGN KEY (parent_region_id) REFERENCES public.region(id);


--
-- Name: statistic_value statistic_value_data_source_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.statistic_value
    ADD CONSTRAINT statistic_value_data_source_id_fkey FOREIGN KEY (data_source_id) REFERENCES public.data_source(id);


--
-- Name: statistic_value statistic_value_data_source_publication_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.statistic_value
    ADD CONSTRAINT statistic_value_data_source_publication_id_fkey FOREIGN KEY (data_source_publication_id) REFERENCES public.data_source_publication(id);


--
-- Name: statistic_value statistic_value_region_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.statistic_value
    ADD CONSTRAINT statistic_value_region_id_fkey FOREIGN KEY (region_id) REFERENCES public.region(id);


--
-- Name: statistic_value statistic_value_statistic_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.statistic_value
    ADD CONSTRAINT statistic_value_statistic_id_fkey FOREIGN KEY (statistic_id) REFERENCES public.statistic(id);


--
-- Name: subdivision subdivision_region_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.subdivision
    ADD CONSTRAINT subdivision_region_id_fkey FOREIGN KEY (region_id) REFERENCES public.region(id);


--
-- PostgreSQL database dump complete
--

\unrestrict dbmate


--
-- Dbmate schema migrations
--

INSERT INTO public.schema_migrations (version) VALUES
    ('20260525184135'),
    ('20260525184136'),
    ('20260603030136'),
    ('20260621120000'),
    ('20260812120000'),
    ('20260814120000'),
    ('20260819140100'),
    ('20260822120000'),
    ('20260822140000'),
    ('20260825120000'),
    ('20260901120000'),
    ('20260901180000'),
    ('20260902120000');
