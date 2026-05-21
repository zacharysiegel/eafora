# Data sources survey

<!--
Status: draft, generated 2026-05-21 from a research-agent survey. Confidence varies by source — see "Open questions / things to verify" section near the end. Several claims (rate limits, license edge cases, exact API shapes) should be confirmed against the live source before Eafora commits to ingesting from it.
-->

## Executive summary

Eafora should prioritize integrating data from five foundational sources in v1, each addressing different aspects of the global fertility landscape and together providing comprehensive baseline coverage for the anglosphere and EU:

1. **UN World Population Prospects (WPP)** — the de facto authoritative standard for global TFR, age-structure, and demographic projections. Covers 195+ countries; updated biennially with a 1-2 year lag. Critical for establishing baseline credibility.
2. **Eurostat** — EU's primary statistical authority. Mandated open data (CC-BY). Provides granular subnational data for all EU member states plus EEA; updated quarterly to annually depending on indicator. Highest-quality data for the EU region.
3. **World Bank Open Data** — broad coverage (180+ countries), free API and bulk download, critical indicators (TFR, CBR, CDR, ASFR). License permits non-commercial reuse with attribution; check dataset-specific terms. Lag typically 2-3 years.
4. **Human Fertility Database (HFD)** — unparalleled high-quality fertility detail for 38 developed countries. Open access. Provides period and cohort TFR, age-specific fertility, mean age at birth, completed fertility by birth order. Ideal for deep historical and regional (subnational) breakdowns in Austria, Canada, Scandinavia, UK, US.
5. **Our World in Data (OWID)** — re-aggregator drawing from WPP, HFD, HMD, and others. Provides calculated indicators (effective TFR, fertility intentions) and clean, versioned downloads. Fully open-licensed (CC-BY). Useful as a secondary source validation layer and for indicators OWID calculates itself.

**Gaps and risks:**
- **License complexity**: World Bank and some national statistics offices have mixed licensing (some derivatives allowed, some prohibited). Must verify per dataset before ingesting into PostgreSQL.
- **Sub-national data sparse outside developed world**: DHS and IPUMS International are vital for developing-country subnational breakdowns, but require separate ingestion pipelines.
- **Disaggregation scarcity**: Race/ethnicity fertility data exists only for select countries (US Census, UK ONS, Australia ABS); religion-fertility intersections are severely limited (Pew Research has survey estimates only, not official stats; WRD has no fertility data). Political affiliation is essentially absent from official statistics globally.
- **Timeliness trade-off**: Official statistics lag 18-36 months; survey data (DHS, IPUMS, GGP) lag 2-5 years or more post-collection. Eafora will need to explicitly timestamp every data point and manage user expectations about recency.

Priority integration risk: **World Bank licensing**. Clarify dataset-by-dataset whether derivative aggregation (combining TFR from multiple sources into a canonical store) is permitted. Recommend contacting datahelpdesk@worldbank.org before committing to WB as v1 primary source.

## Source profiles

### 1. UN World Population Prospects (WPP)

**Name & Organization:** World Population Prospects (WPP), UN Population Division (UNFPA/DESA)

**Primary URL:** https://population.un.org/wpp/

**Coverage:**
- Countries: 195+ (all UN member states and territories)
- Indicators: TFR, CBR, CDR, NRR, ASFR (5-year age groups), population pyramids, median age, life expectancy, fertility by parity (births 1–4+), adolescent fertility
- Time range: 1950–2100 (medium-variant projections from 2024 onward)
- Sub-national: None (country-level only)

**Update Frequency:** Biennial (next: mid-2026 revision expected)

**Publication Lag:** ~18 months (2024 revision published May 2024, contains 2023 reference data where final)

**License:** Public domain. UN data is generally not restricted; check https://www.un.org/en/about-us/copyright/ for specifics. Typically free reuse with citation.

**Format:**
- Excel (.xlsx) summary tables (country profiles, key indicators)
- CSV bulk downloads (zipped; time series format)
- Web API: https://population.un.org/wpp/API/ (REST JSON endpoints by country, indicator, year)
- SDMX format also available

**Cost:** Free

**Authority/Quality:**
- Gold standard for international fertility comparisons
- Data come from national statistical offices plus UNDemographic Estimates and Projections Section (DESA) adjustments and interpolations for countries lacking vital registration
- Caveats: imputation and estimation for countries with weak vital statistics; projections use historical fertility trends, not forward-looking surveys; TFR estimates reflect national averages only (no ethnic or geographic disaggregation)

**Notes & Caveats:**
- WPP publishes both "medium variant" (most-used) and alternative scenarios (low, high variants); Eafora should default to medium but store alternatives for sensitivity analysis
- Revision cycles are biennial; intermediate-year data availability depends on whether countries updated national data; do not assume all years have been released
- Age-specific fertility rates provided only for 5-year age groups (15–19, 20–24, …, 45–49); single-year ages not available from WPP
- Revisions can be substantial year-on-year; maintain version control and publish provenance (e.g., "TFR for Country X, WPP 2024 revision, reference year 2022")

---

### 2. Eurostat

**Name & Organization:** Eurostat, European Commission Directorate-General for Statistics

**Primary URL:** https://ec.europa.eu/eurostat (data portal at https://ec.europa.eu/eurostat/databrowser-frontend/)

**Coverage:**
- Countries: 27 EU member states + 3 EEA states (Iceland, Liechtenstein, Norway) + Turkey (associated); some candidate countries
- Indicators: TFR, CBR, CDR, ASFR (single-year age groups 15–49), adolescent fertility, mean age at childbearing, marriage/divorce rates, cohabitation, live births by mother's age and parity, life expectancy, population structure
- Time range: 1960–present (annual, with quarterly provisional releases for recent data)
- Sub-national: Yes. NUTS-2 and NUTS-3 regional breakdowns (e.g., Île-de-France, Baden-Württemberg, Lazio) for TFR and CBR; coverage varies by country

**Update Frequency:** Quarterly (preliminary data), annual (final data) for most indicators. TFR final data released ~18 months after reference year.

**Publication Lag:** ~18–24 months for final annual TFR; preliminary estimates available within 6 months

**License:** CC-BY 4.0 (requires attribution; allows derivative works and commercial reuse). Source attribution: "Eurostat [online data code]. Accessed [date]."

**Format:**
- **TSV (tab-separated) bulk downloads** from Bulk Download Centre (https://ec.europa.eu/eurostat/data/database)
- **JSON API** via SDMX web service (https://ec.europa.eu/eurostat/web/json-and-unicode-web-services)
- **OData** interface (https://ec.europa.eu/eurostat/api/hub)
- Excel and CSV exports per-dataset
- SDMX-ML (statistical data exchange standard)

**Cost:** Free

**Authority/Quality:**
- Highest-quality fertility data for the EU and EEA
- Data collection mandated by EU Regulation 2015/696 (production of European statistics on live births, marriages, divorces by detailed characteristics)
- Standardized methodology across member states; comparable over time and geography
- Preliminary data (flash estimates) available within 6 months; final data audited before release

**Notes & Caveats:**
- Bulk downloads are large (~200 MB for full demographic database); filter by dataset code (e.g., `demo_aind` for age-related indicators, `demo_fer` for fertility) to reduce payload
- Regional data (NUTS-2/3) is not available for all countries and all years; check country-specific metadata
- Live births data are rich: can filter by mother's age (single year), parity, marital status, father's age; however, not all permutations are published in all years
- Eurostat flags provisional data; users must check "data status" metadata to distinguish final from provisional
- No ethnic or religious disaggregation (EU statistics prohibition on collecting race/ethnicity data in most member states)
- API rate limits not documented; for bulk ingestion, use TSV download rather than repeated API calls

---

### 3. World Bank Open Data

**Name & Organization:** World Development Indicators (WDI), The World Bank Group

**Primary URL:** https://data.worldbank.org/

**Coverage:**
- Countries: 189 (World Bank member states and territories)
- Indicators: TFR (`SP.DYN.TFRT.IN`), CBR (`SP.URB.TOTL.IN_ZS` and `SP.DYN.CBRT.IN`), CDR (`SP.DYN.CDRT.IN`), adolescent fertility (`SP.ADO.TFRT`), wanted fertility (`SP.DYN.WFRT.IN`), age-group population shares, contraceptive prevalence
- Time range: 1960–present (annual; lag varies by country and indicator)
- Sub-national: None (country-level only)

**Update Frequency:** Annual (approximately mid-year)

**Publication Lag:** 2–3 years typical (reference year 2022 data published mid-2024, for example)

**License:** Mixed. The World Bank terms (https://www.worldbank.org/en/about/legal/terms-and-conditions) state:
- Datasets in the Data Catalog have **separate, dataset-specific terms of use** incorporated by reference
- Some datasets allow commercial reuse and derivatives; others restrict commercial use
- Attribution required: "The World Bank Group authorizes the use of this material subject to the terms and conditions on its website"
- Non-commercial reuse is generally permitted; commercial aggregation requires verification per dataset

**Action Required**: Contact datahelpdesk@worldbank.org to confirm whether creating a derivative canonical database for Eafora's PostgreSQL store constitutes commercial use. Eafora's monetization model is undecided (per the initial brief: nonprofit funding, grants, sponsorships, and freemium are all in scope, but no direction is locked); some plausible models would qualify as commercial use under World Bank dataset terms, so this needs to be settled before ingesting.

**Format:**
- **REST API** (https://data.worldbank.org/developers): JSON responses; supports country, indicator, date filtering
- **Bulk CSV download** via DataBank (https://data.worldbank.org/databank/)
- Excel and CSV per-indicator exports

**Cost:** Free

**Authority/Quality:**
- Data sourced from national statistical offices, UNFPA, CDC, and others
- Comparable across countries but not as high-quality as national official sources or HFD
- Imputation and modeling used for countries with gaps
- API rate limits: not formally documented; recommend caching downloaded data rather than live API calls

**Notes & Caveats:**
- TFR indicator (`SP.DYN.TFRT.IN`) is sourced from UN WPP; do not rely on World Bank as primary source if WPP is already ingested
- Adolescent fertility and wanted fertility are sourced from DHS and national surveys; more recent data may be available directly from DHS
- Age-group population data (0–14, 15–64, 65+) are shares of total population, not absolute counts; combine with total population series for pyramid reconstruction
- No ASFR (age-specific rates) by single-year age; data are top-level indicators only
- Indicator availability varies; not all countries report all indicators in all years

---

### 4. Human Fertility Database (HFD)

**Name & Organization:** Human Fertility Database, Max Planck Institute for Demographic Research (MPIDR) and Vienna Institute of Demography (VID)

**Primary URL:** https://www.humanfertility.org/

**Coverage:**
- Countries: 37 fully processed + 4 in preliminary release (Greece, Israel, Latvia, Luxembourg)
- Fully processed: Austria, Belarus, Belgium, Bulgaria, Canada, Chile, Croatia, Czechia, Denmark, Estonia, Finland, France, Germany (East/West separate), Hungary, Iceland, Ireland, Italy, Japan, Lithuania, Netherlands, Norway, Poland, Portugal, South Korea, Russia, Slovakia, Slovenia, Spain, Sweden, Switzerland, Taiwan, Ukraine, USA, UK (England/Wales, Scotland, Northern Ireland separate)
- Indicators: TFR (period and cohort), tempo-adjusted TFR, ASFR by single-year age and birth order (1st, 2nd, 3rd+), mean age at birth, mean age at first birth, completed cohort fertility, cohort parity distribution
- Time range: ~1900–present (depth varies by country; most have data from 1950 onward; some European countries back to 1900)
- Sub-national: None in main database (some countries have state/regional variants noted, but not systematized)

**Update Frequency:** Annual (typically released mid-year, with reference year lag of 2–3 years)

**Publication Lag:** 2–3 years (e.g., 2022 data typically released mid-2025)

**License:** Open data (no formal CC designation stated, but site emphasizes "open data principles"). Data is free to download and reuse; cite source as "Human Fertility Database (HFD). Max Planck Institute for Demographic Research (MPIDR) and Vienna Institute of Demography (VID)."

**Format:**
- **Excel (.xlsx) summary tables**: Key indicators (TFR, mean age, completed fertility) by country and year in single spreadsheet
- **Zipped data files** (ASCII text, fixed-width format): Full datasets for bulk download; includes detailed documentation (Data Formats guide, Full Protocol)
- **Country-specific web pages**: Individual downloadable datasets per country

**Cost:** Free

**Authority/Quality:**
- Highest-quality fertility data available for developed countries
- Data sourced exclusively from national vital statistics (births, deaths registries); no estimates or imputations
- Standardized data checking, recoding, and validation across countries
- Widely cited in demographic research literature
- Comparable across countries and time periods due to unified methodology

**Notes & Caveats:**
- Limited to 37 developed countries (primarily OECD); no coverage of Africa, Asia (outside Japan, South Korea, Taiwan), Latin America, or Middle East
- Preliminary release data (Greece, Israel, Latvia, Luxembourg) have not been fully checked or corrected; treat as provisional
- ASFR available by age (15–49, single-year) and parity (birth order 1–4+) but not jointly (no age-parity matrix)
- Tempo-adjusted TFR calculated using Bongaarts-Feeney method; useful for cross-national comparison but requires familiarity with tempo distortion concept for user education
- Data files are detailed but require parsing; recommended to write CSV converter from HFD's fixed-width format

---

### 5. Our World in Data (OWID)

**Name & Organization:** Our World in Data (OWID), University of Oxford, Global Change Data Lab (nonprofit)

**Primary URL:** https://ourworldindata.org/

**Coverage:**
- Countries: 195+ (global)
- Indicators: TFR, ASFR, fertility rate by age group, adolescent fertility, effective TFR (Bongaarts-Feeney tempo-adjusted), desired number of children (from Pew surveys), population pyramids, life expectancy, infant mortality, births by age of mother, mean age at childbearing
- Time range: 1950–present
- Sub-national: None (country-level aggregates only)

**Update Frequency:** Continuous (source data updated as available); chart/dataset updates typically within 3–6 months of upstream source publication

**Publication Lag:** Inherits lag from upstream sources (WPP ~18 months, HFD ~24 months, DHS 3–5 years post-survey)

**License:** CC-BY 4.0 (fully open; allows commercial reuse and derivatives). Attribution: "Our World in Data, [indicator], accessed [date]."

**Format:**
- **Chart download links** (CSV, JSON, Excel) via each chart's "Download" tab
- **Chart Data API** (https://github.com/owid/owid-grapher/blob/master/docs/api.md): JSON REST endpoint `https://github.com/owid/owid-datasets/tree/master/datasets` returns dataset metadata and time-series data
- **GitHub repository** (https://github.com/owid/owid-datasets): Archived datasets; OWID notes this is no longer primary distribution method (prefers chart API)
- **OData interface**: Some datasets available via OData standard

**Cost:** Free

**Authority/Quality:**
- Curated aggregator; data inherit quality of upstream sources (WPP, HFD, HMD, DHS)
- Effective TFR and fertility-intentions indicators are OWID calculations; check methodology in chart documentation
- Visual presentation (interactive charts) is excellent for public communication but not suitable for bulk ingestion; use data exports

**Notes & Caveats:**
- OWID is a **re-aggregator**, not a primary source. For v1, ingest directly from UN WPP, Eurostat, and HFD rather than OWID to avoid a chain of dependencies
- Effective TFR calculations use Bongaarts-Feeney method applied to UN WPP data; methodology should be documented in Eafora's provenance layer
- Fertility intentions (desired children) come from Pew Research surveys, which are sparse geographically and infrequent; not suitable for comprehensive global coverage
- OWID's data update lag can vary; some indicators update within weeks of upstream publication, others remain stale for months; verify freshness before use
- Use OWID as a **validation source** (compare TFR values across sources for anomaly detection) rather than primary ingestion target

---

### 6. Demographic and Health Surveys (DHS)

**Name & Organization:** Demographic and Health Surveys (DHS), USAID-funded, ICF International (now Icahn Associates)

**Primary URL:** https://www.dhsprogram.com/

**Coverage:**
- Countries: 90+ countries (primarily Sub-Saharan Africa, South Asia, Middle East, Latin America; sparse in developed world)
- Indicators: TFR (from birth history modules), ASFR (reconstructed from births in preceding 5 years), adolescent fertility, fertility intentions (desired family size), age at first birth, age at first marriage, contraceptive prevalence, ideal number of children (by education, wealth, region)
- Time range: 1984–present (surveys conducted every 3–5 years per country)
- Sub-national: Yes. Regional and sometimes provincial/state breakdowns; includes urban/rural splits

**Update Frequency:** DHS surveys cycle every 3–5 years per country; new surveys typically released 2–3 years after fieldwork completion

**Publication Lag:** 2–5 years (data collection through final report publication)

**License:** Free. Survey microdata available for download with user registration; see https://dhsprogram.com/data/new-user-registration.cfm. License terms: data are free for non-commercial research and evaluation; commercial use requires permission. Attribution required: "Demographic and Health Surveys (DHS), [country], [year]."

**Format:**
- **DHS STATcompiler** (web interface): Tabular summaries by country, indicator, demographic breakdowns (age, education, wealth quintile, region)
- **Microdata downloads** (Stata .dta, SPSS .sav, ASCII): Requires registration and acceptance of user agreement
- **DHS API** (https://api.dhsprogram.com/rest/): JSON REST endpoint for accessing datasets, indicators, surveys by country

**Cost:** Free (microdata registration required but no fee)

**Authority/Quality:**
- High-quality household surveys with large sample sizes (typically 5,000–30,000 households per country)
- Standardized questionnaires and methodology across countries; enables comparisons
- Focuses on developing countries; limited coverage of developed world
- DHS estimates TFR from survey birth histories (retrospective recall), not vital registration; subject to recall bias and missing birth data

**Notes & Caveats:**
- DHS TFR is estimated from survey birth histories (typically births in the 5 years before survey); inherently subject to misreporting and recall error
- ASFR calculated as births per woman per year in preceding 5 years; not individual single-year age rates but 5-year period rates
- Sub-national data quality depends on sample size per region; some regions may have large confidence intervals
- DHS publishes subnational breakdowns by wealth quintile, educational attainment, and residence (urban/rural); limited by what was measured in each survey
- No ethnic or religious disaggregation in DHS standard indicators (though some countries have collected this in special modules)
- Microdata access requires registration and acceptance of user agreement (terms of service state data are confidential and not to be redistributed)

---

### 7. IPUMS (Integrated Public Use Microdata Series)

**Name & Organization:** IPUMS, University of Minnesota, Institute for Social Research and Data Innovation (ISRDI)

**Primary URL:** https://www.ipums.org/

**Coverage:**
- Collections: IPUMS USA (US Census + ACS, 1850–present), IPUMS CPS (Current Population Survey, 1962–present), IPUMS International (100+ countries, census microdata), IPUMS Global Health (DHS, MICS, PMA), IPUMS NHGIS (US historical GIS + summary tables), IPUMS IHGIS (International historical GIS)
- Indicators (fertility-related): Fertility (births ever born), children in household, age at first childbirth, marital status, household composition; varies by dataset
- Time range: 1850–present (IPUMS USA); 1900–present (IPUMS International)
- Sub-national: Yes. IPUMS links census records to geographic identifiers (county, state, district, province depending on country); enables subnational and temporal analysis

**Update Frequency:** As new census rounds are conducted and released (typically every 5–10 years per country); IPUMS adds new samples continuously

**Publication Lag:** 1–3 years after census data release (IPUMS performs cleaning, harmonization, allocation of missing data)

**License:** Data-dependent. General terms https://www.ipums.org/about/terms:
- **IPUMS USA & CPS**: Available for non-commercial research and educational use; commercial use restricted
- **IPUMS International**: Data are restricted to "research and educational purposes only." Commercial use prohibited. Genealogical research explicitly prohibited.
- **Health datasets (Global Health, NHIS, MICS)**: Restricted to "statistical reporting and analysis only"
- **Attribution required**: "IPUMS Integrated Public Use Microdata Series, [project], [dataset], [year/sample], https://doi.org/10.18128/[DOI]"

**Format:**
- **Online data extraction tool** (https://www.ipums.org/extract/): Select variables, samples, and geographies; extract generates downloadable files in Stata, SPSS, R, or fixed-ASCII format
- **Microdata files** (.dat fixed-width, .dta Stata, .sav SPSS): Downloaded directly
- **DDI (Data Documentation Initiative) metadata** (XML): Machine-readable variable and sample documentation
- **Bulk download**: Available for registered users (non-redistributable)

**Cost:** Free (registration required; no cost for download)

**Authority/Quality:**
- Integrated across time and space; standardized variable definitions enable cross-census comparisons
- Census microdata are subject to disclosure avoidance measures (PUMS swapping, noise injection in recent US Census samples); affects small-area estimates
- IPUMS performs extensive harmonization; inconsistent original variable codes are standardized
- High quality for subnational analysis (county-level in US; district-level in many countries)

**Notes & Caveats:**
- Microdata are individual records (large files); analysis requires statistical software or custom code
- IPUMS USA fertility data are limited to "children ever born" (stock measure) and age at first childbirth; does not include fertility rates by age
- IPUMS International covers 100+ countries but not all with same depth; some older censuses have limited samples (1% or 5% of population)
- Microdata are subject to confidentiality restrictions; redistribution of downloaded microdata is prohibited in most cases
- Subnational breakdowns are possible but require users to aggregate microdata themselves; no pre-tabulated subnational fertility tables in IPUMS
- For aggregate subnational TFR, use IPUMS IHGIS (historical GIS summaries) if available for the country and year

---

### 8. US Census Bureau International Database (IDB)

**Name & Organization:** US Census Bureau, International Programs Center (IPC)

**Primary URL:** https://www.census.gov/data-tools/demo/idb/

**Coverage:**
- Countries: 200+ (all countries with population >5,000)
- Indicators: Population pyramids (age-sex structure), TFR, CBR, CDR, median age, life expectancy, age dependency ratios, infant mortality, migration rates (conceptual; data vary)
- Time range: 1950–2025 (projections to 2050 available)
- Sub-national: None (country-level only)

**Update Frequency:** Annual (approximately mid-year)

**Publication Lag:** 1–2 years

**License:** Public domain (US government data). No restrictions; can be freely used and modified.

**Format:**
- **Web interface** (interactive selection tool): Query by country, age groups, year; returns tables
- **Bulk download**: CSV/Excel downloads available
- **API**: Legacy CEMD API (deprecated; check current status); newer API under development

**Cost:** Free

**Authority/Quality:**
- TFR and demographic estimates are based on UN WPP and CDC NCHS where available; IDB is not a primary source
- Population pyramids are interpolated from 5-year age groups; full single-year age detail not always available
- Useful for quick reference and cross-checking, but not a primary data source for v1 integration

**Notes & Caveats:**
- IDB is a **convenience tool** for visualizing Census Bureau's demographic database, not a primary data collection effort
- TFR data inherited from UN WPP; ingest directly from WPP instead for v1
- Data quality depends on source; developing countries' estimates may be rough approximations
- No sub-national data; no ethnic or religious disaggregation

---

### 9. CDC National Center for Health Statistics (NCHS) — Vital Statistics

**Name & Organization:** CDC National Center for Health Statistics (NCHS), Centers for Disease Control and Prevention (US)

**Primary URL:** https://www.cdc.gov/nchs/nvsr/

**Coverage:**
- Geographic scope: United States only (by state, some subnational breakdowns)
- Indicators: Births (natality data), deaths (mortality data), fertility rates, maternal age, parity, legitimacy status, live birth rates by age, place of delivery, attendant at birth
- Time range: 1900–present (annual; some subnational detail from 1950 onward)
- Sub-national: Yes. State-level breakdowns; some metropolitan area and county-level data in specialized reports

**Update Frequency:** Annual (annual Vital Statistics reports released ~10–12 months after reference year)

**Publication Lag:** 10–12 months

**License:** Public domain (US government data). Freely usable and modifiable.

**Format:**
- **Vital Statistics National Summary** (annual report): PDF with tables and text
- **CDC WONDER** (Wide-ranging ONline Data for Epidemiologic Research): Web query tool (https://wonder.cdc.gov/) for detailed natality and mortality data; can export CSV
- **FTP downloads**: CDC FTP site hosts annual natality and mortality datasets in compressed formats
- **Data.CDC.gov**: Health data catalog; links to downloadable datasets

**Cost:** Free

**Authority/Quality:**
- Based on nearly complete vital registration system (US has 100% birth registration coverage)
- Highest quality fertility data available for the United States
- Published estimates of TFR and age-specific fertility rates are reliable and widely cited

**Notes & Caveats:**
- US-only data; not relevant for global coverage but critical for US subnational detail in v1 if Eafora prioritizes US data depth
- CDC publishes both preliminary (within 6 months) and final (within 12–14 months) data; preliminary data are subject to revision
- Some data fields (e.g., father's age, parental education) are not available in all states/years
- Microdata (individual birth records) are not freely available; subnational aggregations are published instead
- For US subnational TFR, CDC WONDER or Vital Statistics of the United States (annual report) are primary sources

---

### 10. UK Office for National Statistics (ONS)

**Name & Organization:** Office for National Statistics (ONS), UK, Department for Levelling Up, Housing and Communities

**Primary URL:** https://www.ons.gov.uk/

**Coverage:**
- Geographic scope: United Kingdom (England, Wales, Scotland, Northern Ireland; England by region and local authority)
- Indicators: Live births, births by age of mother, fertility rates (TFR, ASFR by single-year age 15–49), mean age at childbearing, births outside marriage, conceptions, abortions, stillbirths, family size, marriages, divorces, civil partnerships
- Time range: 1838–present (detailed records; fertility rates from 1950 onward)
- Sub-national: Yes. Regional (9 regions in England) and local authority level breakdowns available

**Update Frequency:** Quarterly (preliminary data), annual (final data); most indicators released ~12 months after reference year

**Publication Lag:** 12 months typical for annual estimates

**License:** Open Government Licence v3.0 (OGL 3.0). Allows commercial reuse and derivatives; requires attribution: "Contains National Statistics data © Crown copyright and database rights [year]."

**Format:**
- **Statistical releases** (PDF and Excel): Annual fertility statistics reports
- **Data finder and time series explorer** (web tool): Query specific indicators; export as CSV/Excel
- **Open data downloads**: CSV/Excel files available from ONS Open Data portal
- **API**: Not formally documented; some data available via NOMIS (https://www.nomisweb.co.uk/) which indexes ONS and other UK statistics

**Cost:** Free

**Authority/Quality:**
- Sourced from UK civil registration (vital statistics) system; near-complete coverage
- Standardized methodology; comparable across decades and geographies
- Includes some demographic breakdowns (e.g., births by ethnicity of mother for England/Wales from 2003 onward, though not always released)

**Notes & Caveats:**
- Regional and local authority data are available but often with time lag (1–2 years behind England-wide estimates)
- Ethnicity data (births by mother's ethnicity) available for England/Wales but not all UK countries; not available in full time series
- ONS makes distinction between "live births" and "fertility rate" (births per 1,000 women, typically ages 15–49); both are published
- Some smaller areas have suppressed data due to disclosure avoidance; aggregation required for publication

---

### 11. Statistics Canada

**Name & Organization:** Statistics Canada, Department of Industry

**Primary URL:** https://www.statcan.gc.ca/

**Coverage:**
- Geographic scope: Canada (national and by province/territory)
- Indicators: Live births, TFR, ASFR (by single-year age and 5-year groups), births by age of mother, crude birth rate, mean age at childbearing, births outside marriage, family size, marriage rates, divorce rates
- Time range: 1921–present (detailed records; fertility rates from 1950 onward)
- Sub-national: Yes. Provincial and territorial breakdowns

**Update Frequency:** Annual (typically released 12–18 months after reference year)

**Publication Lag:** 12–18 months

**License:** Statistics Act. Data are government of Canada material available under Open Government License – Canada. Allows commercial reuse; requires attribution: "Statistics Canada, [dataset title], [date of access]."

**Format:**
- **Data Table releases** (CSV, Excel): Statistics Canada's data table system (https://www150.statcan.gc.ca/n1/en/type/data)
- **Census data**: 5-year census includes fertility questions; microdata available via IPUMS or Statistics Canada's Data Portal
- **API**: Statistics Canada Data API (https://www.statcan.gc.ca/eng/developers): JSON REST endpoint for accessing Statistics Canada tables

**Cost:** Free

**Authority/Quality:**
- Based on vital registration (births) and Census of Canada; high quality
- Standardized methodology across time and provinces
- Census data (5-year) include more detailed fertility questions (children ever born, age at first childbirth) but with 5-year lag

**Notes & Caveats:**
- Provincial fertility data are available but sometimes aggregated into larger groupings for small provinces
- Census data are collected every 5 years (most recent: 2021); years between censuses use administrative data only
- Some demographic breakdowns (e.g., by immigration status, educational attainment) are available in Census or special surveys but not in routine vital statistics

---

### 12. Australian Bureau of Statistics (ABS)

**Name & Organization:** Australian Bureau of Statistics (ABS)

**Primary URL:** https://www.abs.gov.au/

**Coverage:**
- Geographic scope: Australia (national and by state/territory)
- Indicators: Live births, TFR (1.481 in 2024 as of latest available), ASFR, births by age of mother, births by state, crude birth rate, mean age at childbearing, births outside marriage, family composition, marriage/divorce rates
- Time range: 1901–present (detailed records; fertility rates from 1950 onward)
- Sub-national: Yes. State and territory breakdowns; some data available at SA-4 (statistical area) level

**Update Frequency:** Quarterly (preliminary data), annual (final data)

**Publication Lag:** 6–12 months for final annual data

**License:** Creative Commons Attribution 4.0 International (CC-BY 4.0). Allows commercial reuse and derivatives; requires attribution: "Australian Bureau of Statistics, [title], [date], https://www.abs.gov.au."

**Format:**
- **ABS Data Releases** (PDF, Excel, CSV): Annual fertility statistics reports
- **ABS TableBuilder** (web tool): Query Census and survey data; export as Excel/CSV
- **Data API**: ABS Data API (https://api.abs.gov.au/): JSON REST endpoint for accessing ABS tables
- **Census data**: 5-year census (most recent: 2021) includes detailed fertility questions (children ever born, age at first childbirth)

**Cost:** Free

**Authority/Quality:**
- Based on vital registration and Census; high quality
- Standardized across time and geographies
- Includes ethnic/cultural diversity data in Census and some demographic breakdowns

**Notes & Caveats:**
- State-level TFR available; not always subnational below state level in routine releases
- Census data (5-year) include more detailed fertility questions; 5-year lag after Census enumeration
- ABS publishes both preliminary and final data; preliminary data can be revised

---

### 13. Stats NZ (Statistics New Zealand)

**Name & Organization:** Stats NZ – Te Whare Tatauranga Aotearoa

**Primary URL:** https://www.stats.govt.nz/

**Coverage:**
- Geographic scope: New Zealand (national; regional data available for larger regions)
- Indicators: Live births, TFR, fertility rates by age, births by age of mother, mean age at childbearing, births outside marriage, family size, marriages, divorces
- Time range: 1900s–present (detailed records; fertility rates from ~1950 onward)
- Sub-national: Limited. Regional breakdowns available but not at all subnational levels

**Update Frequency:** Annual (released ~12 months after reference year)

**Publication Lag:** 12 months typical

**License:** Creative Commons Attribution 4.0 International (CC-BY 4.0). Allows commercial reuse and derivatives; requires attribution: "Stats NZ – Te Whare Tatauranga Aotearoa, [title], [date]."

**Format:**
- **Data releases** (Excel, CSV): Annual vital statistics reports
- **Data Portal** (web tool): Query datasets; export as CSV/Excel
- **Stats NZ Data API** (https://data.stats.govt.nz/): JSON REST endpoint for accessing datasets

**Cost:** Free

**Authority/Quality:**
- Based on vital registration; high quality
- Small population (~5.3 million) means some subnational estimates have larger confidence intervals
- Standardized methodology over time

**Notes & Caveats:**
- Subnational data are limited; most publicly released data are national only
- Some demographic breakdowns (e.g., by ethnicity) are available in Census (5-year) or specific surveys but not routine vital statistics
- Regional breakdowns are typically for 12–16 broad regions, not detailed geographic units

---

### 14. Eurostat Member-State Statistical Offices (France/INSEE, Germany/DESTATIS, Italy/ISTAT, Netherlands/CBS, Sweden/SCB)

**Primary representatives:**

**INSEE (Institut National de la Statistique et des Études Économiques) — France**
- URL: https://www.insee.fr/
- Coverage: France (national and regional by département, région)
- Indicators: TFR, ASFR, births by age, marriages, divorces, family composition, life expectancy
- License: Open Government License (compatible with Eurostat; CC-BY-like)
- Format: Data files (Excel, CSV), API via SDMX (shared with Eurostat)

**DESTATIS (Statistisches Bundesamt) — Germany**
- URL: https://www.destatis.de/
- Coverage: Germany (national and by Bundesland/state)
- Indicators: TFR, ASFR, births by age, family size, marriages, divorces, life expectancy
- License: Creative Commons Attribution 4.0 (CC-BY 4.0)
- Format: Data downloads (Excel, CSV), GENESIS database (internal SDMX-like format)

**ISTAT (Istituto Nazionale di Statistica) — Italy**
- URL: https://www.istat.it/
- Coverage: Italy (national and by regione/region)
- Indicators: TFR, ASFR, births by age, family size, marriages, divorces, life expectancy
- License: Creative Commons Attribution 4.0 (CC-BY 4.0)
- Format: Data downloads (Excel, CSV), IstatData database browser

**CBS (Centraal Bureau voor de Statistiek) — Netherlands**
- URL: https://www.cbs.nl/
- Coverage: Netherlands (national and by province/municipality)
- Indicators: TFR, ASFR, births by age, family size, marriages, divorces, life expectancy
- License: Creative Commons Attribution 4.0 (CC-BY 4.0)
- Format: Data downloads (CSV, Excel), StatLine web tool, API (Dutch)

**SCB (Statistiska Centralbyrån) — Sweden**
- URL: https://www.scb.se/
- Coverage: Sweden (national and by region/county)
- Indicators: TFR, ASFR, births by age, family size, marriages, divorces, life expectancy
- License: Creative Commons Attribution 4.0 (CC-BY 4.0)
- Format: Data downloads (Excel, CSV), PxWeb API (structured data API)

**Notes & Caveats:**
- These are national statistical offices; most data are also published via Eurostat (EU-level aggregator)
- For EU-level integration, Eurostat is preferable (standardized, SDMX-accessible); national offices are useful for subnational detail
- All are free and open-licensed (CC-BY or compatible)
- Formats vary (APIs are SDMX-based but with national customizations); recommend standardizing on Eurostat's SDMX for EU data

---

### 15. UNFPA Population Data Portal

**Name & Organization:** UN Population Fund (UNFPA), Population Data Portal

**Primary URL:** https://www.unfpa.org/data (portal: https://pdp.unfpa.org/)

**Coverage:**
- Countries: 195+ (global)
- Indicators: TFR, total wanted fertility rate, adolescent birth rate, ASFR, age distribution (0–4, 5–9, 15–49, 60+, etc.), sex ratios, median age, dependency ratios, household size, marital status, children's living arrangements, elderly living arrangements, urbanization, migration
- Time range: 1990–present (varies by indicator and country)
- Sub-national: None (country-level only)

**Update Frequency:** Annual (as countries report data to UNFPA)

**Publication Lag:** 1–3 years typical

**License:** Not explicitly stated on portal homepage. Recommend contacting UNFPA directly for licensing terms. Likely to be CC-BY or public domain (UN data standard).

**Format:**
- **Portal interface** (web): Interactive country/indicator browser
- **Data downloads**: Indicator codes provided; link to PDC (PopulationDataCenter) API appears to support CSV export
- **CSV bulk export**: Available for selected indicators by country

**Cost:** Free

**Authority/Quality:**
- Data sourced from national statistical offices, DHS, MICS, and other surveys
- Comparable to World Bank WDI in terms of being a secondary aggregator
- TFR data likely inherited from UN WPP or national sources

**Notes & Caveats:**
- UNFPA is a secondary aggregator; for v1, prioritize primary sources (UN WPP, national offices)
- License terms must be clarified before use
- Data completeness and currency vary by indicator and country
- Use as supplementary source for gap-filling where other sources unavailable

---

### 16. Pew Research Center

**Name & Organization:** Pew Research Center, Religion & Public Life Project

**Primary URL:** https://www.pewresearch.org/religion/

**Coverage:**
- Topics: Religion-fertility intersections, ideal vs. actual family size by religious group, contraceptive use, reproductive attitudes
- Geographic scope: US-focused (some international surveys); limited to survey respondents
- Indicators: Desired number of children, ideal number of children, fertility rates by religious affiliation, reproductive attitudes by denomination
- Time range: 2000–present (survey data; episodic, not continuous)
- Sub-national: Survey data; can be disaggregated by US region, state

**Update Frequency:** Survey-based; new surveys conducted periodically (every 1–3 years for select topics)

**Publication Lag:** 1–2 years post-survey

**License:** Not standardized across all Pew Research. Some datasets are freely available (CC-BY or public use datasets); others require permission for reuse. Check individual survey documentation.

**Format:**
- **Report PDFs** (text and charts): Primary output
- **Datasets**: Available for download from Pew Research website (Excel, SPSS, or ASCII); requires registration for some datasets
- **Raw survey data**: Not always made public; contact Pew directly

**Cost:** Free (access to reports and some datasets; specialized datasets may have restrictions)

**Authority/Quality:**
- High-quality surveys (typically n=1,000–3,000 respondents in US; smaller for international surveys)
- Methodologically rigorous but subject to survey limitations (sampling error, non-response bias)
- Data are survey estimates, not official statistics

**Notes & Caveats:**
- Pew's fertility data are **survey-based**, not from vital registration; subject to misreporting and recall error
- Coverage is mostly US-focused; limited international fertility-religion data
- Surveys are episodic; no comprehensive global fertility-religion dataset exists in official statistics
- Useful for **qualitative insights** on fertility attitudes but not suitable for authoritative TFR by religion estimates

---

### 17. World Religion Database (WRD)

**Name & Organization:** World Religion Database (WRD), Brill, Brill Research Perspectives in Religion

**Primary URL:** https://www.worldreligiondatabase.org/

**Coverage:**
- Countries: 195+ (global)
- Indicators: Religious adherents (census estimates and projections) by major religious categories (18 religious groups + non-religious); religious diversity indices; contextual socio-economic indicators
- Time range: 1900–2050 (estimates and projections)
- Sub-national: None (country-level only)
- **Fertility by religion: NOT included**

**Update Frequency:** Updated periodically (typically every 3–5 years with new projections)

**Publication Lag:** N/A (projections not real-time)

**License:** Subscription-based (institutional or individual license). Not free. Copyright terms unclear; check Brill's terms of service.

**Format:**
- **Web portal** (login-required): Query-based access to religious adherent data
- **Data tables**: Available for download (format varies by subscription tier)
- **Excel/CSV exports**: Available for registered users

**Cost:** Paid subscription (~$500–2,000 per year for institutional access, varies)

**Authority/Quality:**
- Data sourced from censuses, national surveys, expert estimates
- WRD is a re-aggregator; not a primary data source
- **Important caveat**: WRD does NOT include fertility data. It is useful for knowing the population shares of different religious groups but does not directly provide fertility-religion intersections.

**Notes & Caveats:**
- **Not suitable for direct fertility-religion analysis** without combining with external fertility data
- Subscription model limits access; not freely available
- Projections use historical religious affiliation trends; fertility trends are not explicitly modeled
- For fertility-religion intersections, use Pew Research surveys or combine WRD population shares with fertility surveys separately

---

### 18. World Values Survey (WVS)

**Name & Organization:** World Values Survey Association (WVSA), headquartered in Vienna

**Primary URL:** https://www.worldvaluessurvey.org/

**Coverage:**
- Countries: 90+ (primarily OECD countries, some developing countries; some regions sparse)
- Indicators: Attitudes toward family, children, marriage, gender roles, life satisfaction, reproductive intentions, ideal family size
- Time range: 1981–present (7 waves; data updated every 4–5 years)
- Sub-national: Survey respondents can be disaggregated by region, education, income within countries

**Update Frequency:** New wave conducted every 4–5 years

**Publication Lag:** 2–3 years post-data collection

**License:** Data are free for academic researchers. Commercial and non-academic use may have restrictions. Check Data Archive policies: https://www.worldvaluessurvey.org/WVSContents.jsp?CMSID=Findings

**Format:**
- **SPSS, Stata, Excel files**: Downloadable from WVS Data Archive (registration required)
- **CSV exports**: Available via web portal
- **Documentation**: Codebooks and questionnaires provided

**Cost:** Free for academic researchers (registration required); commercial access may incur fees

**Authority/Quality:**
- High-quality surveys; standardized questionnaires across countries and waves
- Respondent sample sizes typically 1,000–2,000 per country
- Time-series data enable trend analysis over 40+ years

**Notes & Caveats:**
- WVS data are **survey-based attitudes** toward family size and reproductive intentions, not actual fertility rates
- Regional coverage uneven; some regions underrepresented
- Actual number of children born may differ from ideal/desired due to unmet family planning needs, economic constraints, and other factors
- **Not suitable as primary fertility data source** but useful for context on reproductive attitudes and intentions

---

### 19. European Social Survey (ESS)

**Name & Organization:** European Social Survey ERIC, coordinated from City University London

**Primary URL:** https://www.europeansocialsurvey.org/

**Coverage:**
- Countries: 30+ European countries (including EEA and some candidate countries)
- Indicators: Attitudes toward family, children, gender roles, life satisfaction, well-being, employment-family balance
- Time range: 2002–present (9 rounds; most recent: Round 9, 2018)
- Sub-national: Survey respondents can be disaggregated by region, education, income within countries

**Update Frequency:** Biennial (new round every 2 years; cycle: collection → data release typically 18–24 months later)

**Publication Lag:** 18–24 months post-data collection

**License:** Data are free under CC-BY-SA 4.0 (for most datasets) or CC-BY 4.0. Allows commercial reuse and derivatives; requires attribution. Some datasets have restricted access (must contact ESS).

**Format:**
- **SPSS, Stata, R datasets**: Downloadable from ESS Data Portal (https://ess-search.nsd.no/)
- **CSV, Excel**: Available via portal
- **Codebooks, questionnaires**: Full documentation provided

**Cost:** Free

**Authority/Quality:**
- High-quality probability surveys; standardized methodology across participating countries
- Sample sizes: 1,500–2,000 respondents per country per round
- Extensive quality control and data checking

**Notes & Caveats:**
- ESS data are **attitudes and values** toward family, not actual fertility rates
- Fertility-specific questions vary by round and country; not all rounds include detailed family questions
- Regional coverage: only European countries; no global data
- Must check individual round questionnaires to determine which rounds include specific fertility-attitude questions

---

### 20. Generations and Gender Programme (GGP)

**Name & Organization:** Generations and Gender Programme (GGP), European research infrastructure

**Primary URL:** https://www.ggp-i.org/

**Coverage:**
- Countries: 20+ (primarily Europe, plus Australia and Japan)
- Indicators: Fertility (actual and intended), partnership formation, marriage, divorce, cohabitation, intergenerational ties, aging, health, employment-family balance
- Time range: 2005–present (longitudinal survey; respondents followed over 10+ years)
- Sub-national: Regional breakdowns available for some countries

**Update Frequency:** Longitudinal survey (respondents re-interviewed every 1–2 years); new country panels added periodically

**Publication Lag:** 1–3 years post-data collection (GGP focuses on data quality over speed)

**License:** Data are available under a user agreement; terms depend on data access level (public use, restricted use). Generally free for academic researchers; commercial use may be restricted. Check GGP Data Portal.

**Format:**
- **GGP Data Portal** (https://www.ggp-i.org/data): Download SPSS, Stata, CSV files
- **Codebooks, questionnaires**: Full documentation
- **Public-use datasets**: Available with limited geographic identifiers for privacy; research-use datasets available with more geographic detail (upon request)

**Cost:** Free (registration required)

**Authority/Quality:**
- High-quality longitudinal survey; enables tracking of fertility transitions over time
- Sample sizes: 3,000–10,000 respondents per country per wave
- Standardized questionnaire across countries enables comparison

**Notes & Caveats:**
- GGP is a **longitudinal survey** focused on fertility intentions and transitions, not a source of population-level TFR estimates
- Data are based on respondent self-reports; subject to survey biases
- Intended for research on **mechanisms of family formation**, not for estimating population TFR
- Longitudinal design means data are complex (nested within respondents, multiple waves); requires statistical sophistication to analyze
- **Not suitable as primary TFR data source** but excellent for understanding fertility decision-making

---

### 21. Human Mortality Database (HMD)

**Name & Organization:** Human Mortality Database (HMD), UC Berkeley and Max Planck Institute for Demographic Research

**Primary URL:** https://mortality.org/

**Coverage:**
- Countries: 41 (primarily developed nations)
- Indicators: **Mortality only** (not fertility); period and cohort mortality rates by age/sex, life tables, survival curves, infant mortality, life expectancy
- Also available: Human Cause-of-Death Database (HCD) with cause-specific mortality
- Time range: 1900–present (depth varies; most countries 1950+)
- Sub-national: Yes. Some countries have subnational series (e.g., East/West Germany, England/Wales/Scotland separate from UK)

**Update Frequency:** Annual

**Publication Lag:** 1–2 years

**License:** Open data (see https://mortality.org/Public/About/contact.php for terms). Typically free; attribution required: "Human Mortality Database, [country], [date of access]."

**Format:**
- **Excel summary tables**: Key indicators (life expectancy, infant mortality, survival rates) by country and year
- **Zipped ASCII data files**: Full datasets for bulk download
- **Country-specific web pages**: Individual datasets per country/region

**Cost:** Free

**Authority/Quality:**
- Highest-quality mortality data for developed countries
- Data sourced from national vital statistics; standardized methodology
- Widely cited in demographic research

**Notes & Caveats:**
- **HMD provides mortality data only, not fertility data**
- The related Human Fertility Database (HFD) provides fertility; they are complementary
- HMD data are rich (single-year ages, multiple death measures), enabling detailed demographic analysis
- Limited to 41 countries; no coverage of most developing regions

---

## Indicator coverage matrix

| **Indicator** | **UN WPP** | **Eurostat** | **World Bank** | **HFD** | **Our World in Data** | **DHS** | **IPUMS** | **CDC NCHS** | **ONS** | **ABS** |
|---|---|---|---|---|---|---|---|---|---|---|
| **TFR (Total Fertility Rate)** | ✓ 1950–present, country | ✓ 1960–present, EU + region | ✓ 1960–present, country | ✓ 1900–present, 37 countries | ✓ 1950–present, country (WPP) | ✓ Survey-based, 90+ countries | ✗ (stock: children ever born) | ✓ 1900–present, US + state | ✓ 1838–present, UK + region | ✓ 1901–present, AU + state |
| **CBR (Crude Birth Rate)** | ✓ Country | ✓ EU + region | ✓ Country | ✗ | ✓ (from OWID calcs) | ✓ Derived from TFR | ✗ | ✓ US + state | ✓ UK + region | ✓ AU + state |
| **CDR (Crude Death Rate)** | ✓ Country | ✓ EU + region | ✓ Country | ✗ | ✓ | ✗ | ✗ | ✓ US (detailed) | ✓ UK | ✓ AU |
| **ASFR (Age-Specific Fertility)** | ✓ 5-year groups, country | ✓ Single-year 15–49, EU + region | ✗ | ✓ Single-year + parity, 37 countries | ✓ 5-year groups (WPP) | ✓ 5-year periods from survey | ✓ (reconstructed from births) | ✓ Single-year, US + state | ✓ Single-year, UK | ✓ Single-year, AU |
| **CFR (Completed Fertility Rate)** | ✗ | ✓ EU only (cohort data) | ✗ | ✓ Cohort parity, 37 countries | ✓ (HFD source) | ✗ | ✓ Census only | ✗ | ✗ | ✗ |
| **Tempo-adjusted TFR** | ✗ | ✗ | ✗ | ✓ Bongaarts-Feeney, 37 countries | ✓ OWID calc (Malani-Jacob) | ✗ | ✗ | ✗ | ✗ | ✗ |
| **NRR (Net Reproduction Rate)** | ✓ Country | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **Mean age at first birth** | ✗ | ✓ EU + some subnational | ✗ | ✓ HFD (mean age at birth), 37 countries | ✓ (HFD source) | ✓ DHS surveys | ✓ Census microdata | ✗ | ✓ ONS data | ✓ ABS data |
| **Age at first marriage** | ✗ | ✓ EU (some) | ✗ | ✗ | ✗ | ✓ DHS | ✓ Census microdata | ✗ | Partial | Partial |
| **Marriage rate** | ✗ | ✓ EU + region | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ US (vital statistics) | ✓ UK + region | ✓ AU + state |
| **Divorce rate** | ✗ | ✓ EU + region | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ US (vital statistics) | ✓ UK + region | ✓ AU + state |
| **Cohabitation rate** | ✗ | ✓ EU (Census/surveys) | ✗ | ✗ | ✗ | ✓ DHS (some countries) | ✗ | ✗ | ✓ (Census) | ✓ (Census) |
| **Childlessness rate** | ✗ | ✓ EU (Census) | ✗ | ✗ | ✗ | ✗ | ✓ Census microdata | ✗ | ✓ (Census) | ✓ (Census) |
| **Fertility intentions (desired children)** | ✗ | ✗ | ✗ | ✗ | ✓ Pew surveys (sparse) | ✓ DHS | ✓ Survey modules | ✗ | ✗ | ✗ |
| **Population pyramids (age × sex)** | ✓ 5-year groups, country | ✓ Single-year EU + region | ✓ Age shares (0–14, 15–64, 65+) | ✗ | ✓ (WPP data) | ✓ Survey-based (age dist.) | ✓ Census microdata | ✓ US Census + vital reg. | ✓ UK Census + estimates | ✓ AU Census + estimates |
| **Median age** | ✓ Country | ✓ EU + region | ✓ Country | ✗ | ✓ (WPP source) | ✓ Derived | ✓ Census microdata | ✓ US | ✓ UK | ✓ AU |
| **Life expectancy** | ✓ Country | ✓ EU + region | ✓ Country | ✗ | ✓ (HMD source) | ✗ | ✗ | ✓ US (CDC) | ✓ UK | ✓ AU |
| **Sex ratio at birth** | ✓ Country | ✓ EU (some) | ✗ | ✗ | ✓ (WPP source) | ✓ DHS | ✓ Census microdata | ✓ US births | ✓ UK births | ✓ AU births |
| **TFR by education** | ✗ | Partial (EU Census) | ✗ | ✗ | ✗ | ✓ DHS (primary, secondary, higher) | ✓ Census + survey microdata | ✗ | Partial (Census) | Partial (Census) |
| **TFR by race/ethnicity** | ✗ | ✗ (EU law prohibits collection in most states) | ✗ | ✗ | ✗ | ✗ | ✓ IPUMS USA (detailed microdata) | ✓ US vital statistics | ✓ ONS (mother's ethnicity, England/Wales) | ✓ ABS (detailed) |
| **TFR by religion** | ✗ | ✗ | ✗ | ✗ | ✓ Pew surveys (very sparse) | ✗ | ✗ | ✗ | ✗ | ✗ |
| **TFR by wealth quintile** | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ DHS | ✓ Survey microdata | ✗ | ✗ | ✗ |
| **Sub-national (region/state/province)** | ✗ | ✓ EU NUTS-2/3 | ✗ | ✗ | ✗ | ✓ DHS regional | ✓ Census (microdata enabling) | ✓ US state/county | ✓ UK region/LA | ✓ AU state/territory |

**Legend:** ✓ = available; ✗ = not available; partial = available for subset of time or geography; "survey-based" = sourced from surveys, not vital registration

---

## License obligations summary

| **Source** | **License** | **Attribution Required?** | **Share-Alike Clause?** | **Allow Derivatives?** | **Allow Commercial Use?** | **Redistribution Allowed?** |
|---|---|---|---|---|---|---|
| **UN WPP** | Public domain / CC0 | Yes (best practice) | No | Yes | Yes | Yes |
| **Eurostat** | CC-BY 4.0 | Yes (format: "Eurostat [dataset code]") | No | Yes | Yes | Yes |
| **World Bank WDI** | Custom / Mixed | Yes ("World Bank Group authorizes...") | No | **Verify per dataset** | **Verify per dataset** (non-commercial default) | Limited |
| **HFD** | Open data (no formal CC) | Yes ("Max Planck/VID") | No | Yes | Yes | Yes |
| **Our World in Data** | CC-BY 4.0 | Yes ("Our World in Data, [indicator]") | No | Yes | Yes | Yes |
| **DHS** | Custom | Yes ("DHS Program, [country/year]") | No | Limited (research only) | No (research/eval only) | Limited |
| **IPUMS** | Custom (by project) | Yes (DOI required) | No | **Restricted** (varies by collection) | No (research/edu only) | No (microdata not redistributable) |
| **CDC NCHS** | Public domain | Yes (best practice) | No | Yes | Yes | Yes |
| **ONS** | OGL 3.0 / CC-BY | Yes ("Crown copyright") | No | Yes | Yes | Yes |
| **Statistics Canada** | OGL-C / CC-BY | Yes ("Statistics Canada") | No | Yes | Yes | Yes |
| **ABS** | CC-BY 4.0 | Yes ("ABS, [title]") | No | Yes | Yes | Yes |
| **Stats NZ** | CC-BY 4.0 | Yes ("Stats NZ") | No | Yes | Yes | Yes |
| **INSEE** | OGL / CC-BY | Yes | No | Yes | Yes | Yes |
| **DESTATIS** | CC-BY 4.0 | Yes | No | Yes | Yes | Yes |
| **ISTAT** | CC-BY 4.0 | Yes | No | Yes | Yes | Yes |
| **CBS (Netherlands)** | CC-BY 4.0 | Yes | No | Yes | Yes | Yes |
| **SCB (Sweden)** | CC-BY 4.0 | Yes | No | Yes | Yes | Yes |
| **UNFPA PDP** | Likely CC-BY (verify) | Likely yes | No | Likely yes | Likely yes | Verify |
| **Pew Research** | Custom (datasets vary) | Yes | No | **Verify per dataset** | **Verify per dataset** | Limited |
| **WRD** | Subscription/proprietary | N/A (paid) | N/A | N/A | N/A | No |
| **World Values Survey** | CC-BY-SA / Custom | Yes | **Yes (SA clause)** | Yes | Verify | Verify |
| **ESS** | CC-BY-SA / CC-BY | Yes | Varies | Yes | Yes | With restrictions |
| **GGP** | Custom / CC-BY | Yes | Varies | Verify | Verify | Limited |
| **HMD** | Open data | Yes ("HMD, [country]") | No | Yes | Yes | Yes |

**Key column definitions:**
- **Share-Alike Clause**: If "Yes," derivative works must use same license (restrictive for proprietary aggregation)
- **Allow Derivatives**: If "No," derivative works (aggregation into new database) may be prohibited
- **Allow Commercial Use**: If "No," for-profit redistribution is prohibited
- **Redistribution Allowed**: If "No," re-sharing the data requires permission

---

## Recommended integration order

### **Phase 1 (MVP — v1.0):** Establish Global Baseline

**Priority sources:** UN WPP, Eurostat, World Bank, HFD, OWID (validation layer)

**Rationale:**
- UN WPP provides the global gold standard; 195+ countries with ~70 years of history
- Eurostat adds high-quality subnational data for 27+ EU countries; mandatory open license (CC-BY)
- World Bank adds breadth (180+ countries) and ease of API access; **must clarify licensing** before committing
- HFD enables depth for 37 developed countries (critical for anglosphere + Nordic/Alpine regions where users expect granular data)
- OWID serves as validation layer (cross-check TFR estimates) and supplements with effective-TFR calculations

**Deliverable:** PostgreSQL canonical store with:
- Country-level TFR, CBR, CDR, ASFR (5-year), population structure (age/sex), median age, life expectancy
- 1950–present for most countries (some historical variants back to 1900 for HFD countries)
- Subnational TFR for EU countries (NUTS-2/3 regions) + US (state-level CDC), UK (region), Australia (state)
- Per-cell provenance: source, retrieval date, confidence/data-quality note
- License compliance: CC-BY attribution; verify World Bank commercial-use terms with legal team

**Timeline:** 8–12 weeks to design schema, ingest, validate, and publish static PMTiles + SQLite

**Risks:**
- World Bank licensing ambiguity (must resolve before ingestion)
- Data lag (WPP/WB are 2–3 years behind current year; set user expectations)
- Subnational data quality gaps in developing regions (partial coverage at best)

---

### **Phase 2 (v1.5–v2.0):** Expand Family Formation & Disaggregations

**Priority sources:** CDC NCHS (US detail), DHS (developing countries subnational), IPUMS International (for census-based subnational in selected countries), Generations and Gender Programme (fertility transitions in Europe)

**New indicators:**
- Age at first birth, age at first marriage, childlessness rate
- Births by parity (1st, 2nd, 3rd+)
- Fertility by educational attainment (DHS, census microdata)
- Fertility intentions (desired vs. actual family size; Pew, WVS, DHS)
- Subnational TFR for selected developing countries (e.g., India via DHS, Sub-Saharan Africa via DHS)
- Marriage and divorce rates (EU, US, UK, Canada, Australia)

**Rationale:**
- Family formation indicators are user priorities (per initial brief); enable richer context on demographic transitions
- DHS provides unprecedented subnational coverage in developing world (otherwise inaccessible)
- Census microdata (IPUMS, national census records) enable education, wealth, and regional breakdowns not available in macro-statistical publications
- GGP provides longitudinal perspective on how fertility decisions form (adds research depth)

**Deliverable:** Extended schema with family-formation indicators; subnational TFR for 30–50 countries across Africa, Asia, Latin America. Also: preliminary-data freshness for the EU + Anglosphere via national-stat-office quarterly/monthly tracks (see "Preliminary vs final vital statistics" section below for the integration list and schema implications).

**Timeline:** 12–16 weeks (parallel ingestion of DHS API, IPUMS extraction tool, census releases)

**Risks:**
- DHS license restricts redistribution of microdata (must store aggregated statistics only, not individual records)
- IPUMS terms prohibit commercial redistribution; must ingest and aggregate separately, store only aggregate counts
- Data quality highly variable by region and survey round; confidence intervals / data quality flags essential
- GGP is longitudinal microdata; requires sophisticated analysis; recommend aggregating only summary statistics, not publishing raw microdata

---

### **Phase 3 (v2.5–v3.0):** Race/Ethnicity, Religion, Sub-national Depth

**Priority sources:** IPUMS USA (US racial demographics), UK ONS (ethnic breakdowns), CDC NCHS (US detailed natality), Pew Research Center (religion-fertility), Australian ABS (ethnic breakdowns), census offices of Canada, NZ

**New indicators:**
- TFR by race/ethnicity (US, UK, Australia, Canada)
- TFR by religion (Pew Research survey estimates; WRD for religious population shares; combined analysis)
- TFR by political affiliation (survey data only; World Values Survey, GSS, Eurobarometer attitudes toward family; note: no official statistics exist)
- Sub-national TFR at county/district level (US, UK, Australia, Canada)
- Fertility-related attitudes by education, income, ideology (WVS, ESS, Eurobarometer)

**Rationale:**
- These disaggregations are limited but user-valuable; enable myth-dispelling on (e.g.) fertility by education, ethnic/religious patterns
- Racial/ethnic data exist for Anglo-American countries; limited elsewhere due to legal/ethical restrictions
- Religion-fertility intersection is sparse (Pew surveys only) but newsworthy; position Eafora as leading source for this limited data
- Subnational depth increases map interactivity and local relevance

**Deliverable:** Disaggregated data layer with caveats on coverage limits and data scarcity; interactive map enables drill-down to ethnicity, education, religion where available

**Timeline:** 16–20 weeks (source-by-source licensing negotiation, microdata harmonization, survey aggregation)

**Risks:**
- Ethical/political sensitivity around race/ethnicity data (must provide education on why gaps exist, e.g., EU data-collection prohibitions)
- Religion-fertility data are survey estimates, not official statistics; large confidence intervals; caveats essential
- Political affiliation data do not exist in official statistics; WVS and similar provide attitudes only, not realized fertility-by-party
- Subnational sourcing is geographically fragmented (no unified API); requires custom per-country scraping/ETL

---

## Preliminary vs final vital statistics (v2 concern)

The MVP can ship using only "final" data from stable aggregator sources (UN WPP, Eurostat final, HFD, World Bank). Lag of 18–36 months from reference year to publication is structural and acceptable for v1.

**Beyond v1, every credible aggregator has to confront a parallel publication track that the survey above only mentions in passing**: most national statistics offices publish *preliminary* (a.k.a. *provisional*) vital statistics on a much faster cadence than their final annual releases. To compete with academic and journalistic uses of fertility data, Eafora needs to surface preliminary data — visibly flagged as such — by v2.

### How preliminary data is published (the pattern)

The flow is the same everywhere, with national variations in cadence and naming:

```
Hospital / birthing center
        │   (files birth certificate)
        ▼
State / regional vital records office
        │   (aggregates, runs first-pass quality checks)
        ▼
National statistical office
        │
        ├── Preliminary / provisional track  (fast)
        │     • Released quarterly or after ~3–6 months
        │     • Late-filed certificates, demographic re-coding, and imputation
        │       for missing fields are not yet finalized
        │     • Subject to revision in subsequent releases
        │
        └── Final track  (slow)
              • Released annually, ~12–24 months after reference year
              • Audited, fully coded, late-arrival certificates incorporated
```

Both tracks come from the same underlying vital registration system; the difference is the amount of QA, imputation, and late-filing wait time the data has been through. Preliminary numbers can revise meaningfully when finals land — usually small, occasionally not.

### Country specifics

**United States**
- Preliminary: CDC NCHS *Vital Statistics Rapid Release* (VSRR), quarterly, ~3–6 month lag. Includes births by state, age of mother, race/ethnicity, total counts, TFR.
- Final: CDC NCHS *National Vital Statistics Reports* (NVSR), "Births: Final Data for <year>", annual, ~12–14 month lag. Adds long-tail fields (mother's education, prenatal-care timing, payer source, attendant at birth).
- Both are public-domain, free, downloadable from `data.cdc.gov` and CDC WONDER.

| Country | Preliminary track | Cadence / lag | Final track |
|---|---|---|---|
| United States | CDC NCHS VSRR (Vital Statistics Rapid Release) | Quarterly, ~3–6 mo | NVSR "Births: Final Data" annual, ~12–14 mo |
| United Kingdom | ONS "Births in England and Wales (provisional)" | Quarterly | ONS "Births in England and Wales" annual, ~12 mo |
| Australia | ABS "Births, Australia (provisional)" | Quarterly | Annual final, ~12 mo |
| Canada | StatCan provisional vital statistics | Quarterly | Annual final, ~12–18 mo |
| EU (aggregate) | Eurostat "Population on 1 January (flash)" + provisional fertility | Annual ~6 mo lag | Annual final, ~18–24 mo |
| France | INSEE "Bilan démographique" (provisional) | Annual, ~3 mo | Annual final, ~18 mo |
| Germany | DESTATIS "Lebendgeborene (vorläufig)" | Quarterly | Annual final, ~12–18 mo |
| Netherlands | CBS provisional monthly counts | Monthly | Annual final, ~12 mo |
| Sweden | SCB monthly preliminary counts | Monthly | Annual final, ~12 mo |
| New Zealand | Stats NZ "Births and deaths (provisional)" | Quarterly | Annual final, ~12 mo |

The pattern is most useful for the EU + Anglosphere (Eafora's stated initial scope). Outside those regions, vital-registration coverage is the binding constraint, and preliminary tracks are rarely meaningful.

### What this means for Eafora's schema and ingestion (v2 work)

1. **Schema must carry a `data_status` field per cell**, not just a freshness timestamp. Plausible enum values: `final`, `provisional`, `preliminary`, `flash_estimate`, `projection`, `imputed`, `interpolated`. The user-facing display can color-code, footnote, or filter by status; the ingestion merge layer can prefer `final` over `provisional` when both exist for the same `(country, year, indicator)` tuple. Constitution Principle II (source provenance) implies we already need this fidelity — preliminary tracks just make it operationally important.
2. **Ingestion runs on different cadences per source.** v1 can be a manual annual run pulling from UN WPP, Eurostat final, HFD. v2 needs at least: a quarterly pull for VSRR, ONS provisional, ABS provisional, StatCan provisional; a monthly pull for CBS and SCB. The ingestion layer should track per-source cadence as configuration, not bake it into code.
3. **The merge layer needs revision tracking.** Preliminary numbers revise when finals land. Eafora should retain old values with their original status for audit/reproducibility, not silently overwrite. This is a v2 schema decision that will be expensive to retrofit in v3.
4. **Per-source ingestion adapters cost more than aggregator adapters.** Each national statistical office has its own portal, format, and authentication model. The v2 budget for adding preliminary-data freshness for the EU + Anglosphere is plausibly several months of part-time work, with most of the cost in writing 6–10 separate scrapers/parsers, not in building one beautiful generic one.

### Sources to add for v2 freshness

In rough priority order (highest user impact first):
1. **CDC NCHS VSRR** — US is high-traffic and the freshest cadence available; quarterly VSRR pulls are cheap (CSV download).
2. **ONS provisional births (England and Wales)** — second-largest English-speaking audience.
3. **Eurostat flash estimates** — single source feeding the entire EU; biggest leverage per integration.
4. **DESTATIS, INSEE, ISTAT, CBS, SCB** — picked up via Eurostat already; direct national portals add slightly faster cadence and finer subnational detail at the cost of one adapter per office.
5. **ABS, StatCan, Stats NZ** — smaller audiences, but completes the Anglosphere story.

This list deliberately omits developing-region preliminary tracks; coverage and quality there don't justify the work in v2.

---

## Open questions / things to verify

1. **World Bank WDI License — Commercial Use Clarification**
   - Does creating a derivative PostgreSQL store with aggregated WB data for a product whose monetization model is not yet decided (and which could plausibly involve commercial use) constitute "commercial use" under WB terms?
   - Action: Contact datahelpdesk@worldbank.org before Phase 1 ingestion; get written confirmation

2. **UN WPP Revision Cycle & Data Lag**
   - WPP revisions are biennial (next: mid-2026); how should Eafora version and republish data with each new WPP revision?
   - Should Eafora archive prior WPP versions in the data store for historical comparison?
   - Action: Design versioning schema for source data; publish data lineage in docs

3. **DHS Microdata Redistribution**
   - DHS terms restrict redistribution of individual-level microdata. Can Eafora publish pre-aggregated subnational TFR statistics (i.e., counts of births / women per region)?
   - Action: Contact DHS program directly (https://www.dhsprogram.com/About/Contact-Us.cfm) for written guidance

4. **IPUMS Commercial Use**
   - IPUMS terms explicitly restrict commercial use of US Census microdata. If Eafora aggregates IPUMS data (e.g., TFR by state/race) and publishes results, is that permissible?
   - Action: Consult IPUMS Terms of Use § 4 again; may need to contact ISRDI directly for clarification

5. **Eurostat Bulk Download Rate Limits**
   - Eurostat's SDMX API and TSV bulk downloads lack documented rate limits. What is the ingestion frequency Eafora can sustain without triggering IP blocking or service degradation?
   - Action: Start with monthly bulk downloads; contact Eurostat support (estat@ec.europa.eu) if throttling occurs

6. **HFD Data Update Lag for Preliminary-Release Countries**
   - Four HFD countries (Greece, Israel, Latvia, Luxembourg) have incomplete processing. What is the timeline for completion?
   - Should Eafora ingest preliminary data with explicit "preliminary" flags, or wait for final release?
   - Action: Contact HFD (humanfertility.org) for guidance

7. **Pew Research Religion-Fertility Data Availability**
   - Pew publishes religion-fertility trends but not as downloadable structured datasets. Can data be extracted from report tables for integration?
   - License terms for derived datasets unclear; must confirm whether republishing aggregated Pew data is permissible
   - Action: Contact Pew Research Center for data-access and licensing inquiry

8. **Sub-national Data Coverage by Country**
   - Which countries publish subnational (region/province/state) TFR data officially? Eurostat and HFD cover EU well; what about Canada, Australia, NZ, US, Chile, South Korea?
   - Action: Audit national statistical offices and census documents; build coverage matrix by country and administrative level

9. **Tempo-Adjusted TFR Methodology**
   - OWID publishes tempo-adjusted TFR (Malani-Jacob method) but MPIDR/HFD also calculate Bongaarts-Feeney tempo-adjusted rates. Which is preferable for Eafora's user base?
   - Should Eafora publish both and explain differences, or pick one?
   - Action: Coordinate with demographic consultants (if any); document both methods in Eafora's technical guide

10. **DHS vs. National Vital Statistics Reconciliation**
    - In countries where both DHS survey data and national vital registration TFR exist, they often diverge. How should Eafora handle discrepancies?
    - Action: Design conflict-resolution rules in data ingest pipeline; flag discrepancies in provenance layer for user review

---

## Sources considered and rejected

1. **Statista** — Premium paid database ($500–2,000/year subscription). Aggregates public data (UN, WB, national sources). **Rejected**: Not a primary source; reduntant with free public sources. Cost prohibitive.

2. **CIA World Factbook** — Provides country-level demographic snapshots (population, TFR, median age, etc.). **Rejected**: Data are sourced from UN WPP and other public sources; not primary. Factbook is static, not suitable for ETL pipeline. No bulk download API.

3. **UN DESA Demographic Estimates Section reports** — Annual publications on demographic trends. **Rejected**: Content is narrative / summary statistics; underlying data better accessed via UN WPP directly. Reports not structured for automated ingestion.

4. **National Statistical Offices (full audit of all countries)** — Each country has its own stat office (e.g., FSO in Switzerland, CSO in Ireland, INEGI in Mexico). **Rejected as exhaustive sources for v1–v3**: Too many (195+) with heterogeneous APIs, languages, formats, and licensing. Better to source regional aggregators (Eurostat, OECD, World Bank) where available; add country-specific sources only for priority countries (US, UK, Canada, Australia, select EU countries). Individual national offices should be added in v3 as subnational depth expands.

5. **European Values Survey** — Cross-national survey on values, attitudes, including family. **Rejected for fertility data**: Limited to attitudes toward family; does not measure actual TFR or fertility behavior. Useful for Phase 3 (attitudes layer) but not core fertility metrics.

6. **MICS (Multiple Indicator Cluster Surveys, UNICEF)** — Similar to DHS; covers 50+ countries, similar to DHS in scope and lag. **Rejected as primary v1 source**: DHS is more mature and widely used; MICS can be added in Phase 2 for geographic gap-filling if needed.

7. **International Labour Organization (ILO) — Labour Force Surveys** — Collect data on employment, education, family formation (e.g., women in labor force by number of children). **Rejected**: Indirect fertility proxy (women's employment by number of children) not true fertility rate. Tangential to mission.

8. **IHME (Institute for Health Metrics and Evaluation) Global Burden of Disease (GBD)** — Estimates fertility as input to population projections. **Rejected**: GBD is a modeling framework, not a primary data source. Underlying data sourced from UN WPP and others. Not suitable for direct ingestion.

9. **Google Trends** — Query volume for fertility-related searches. **Rejected**: Not a data source; a proxy for interest/awareness. Not relevant to Eafora's mission.

10. **Facebook / Meta Fertility Attitudes Survey** — Proprietary survey data on reproductive intentions. **Rejected**: Proprietary; no public API; licensing unclear. Not accessible for public product integration.

---

## Summary of key findings

**Immediate actions (before Phase 1):**
1. Clarify World Bank WDI licensing for commercial derivative use
2. Audit licensing implications under each plausible monetization model (nonprofit, grant-funded, freemium, sponsorship, advertising) and document the constraints each model imposes on which sources Eafora can ingest and redistribute.
3. Prototype PostgreSQL schema with provenance (per-cell source attribution, retrieval timestamp, version/revision tracking)

**Phase 1 achievable with:**
- UN WPP (global TFR, CBR, CDR, ASFR, population structure) — 195 countries, 1950–present
- Eurostat (EU + EEA subnational TFR, family indicators) — 30 countries, 1960–present, NUTS-2/3 regional
- World Bank WDI (secondary global baseline for cross-check) — 189 countries, broad indicators
- HFD (high-quality developed-country detail, 37 countries) — 1900–present, single-year ASFR, parity, tempo-adjusted TFR
- CDC NCHS (US detail) — state/county, single-year ASFR, births by mother characteristics

**Major gaps in v1–v3:**
- Sub-national data in most of developing world (DHS partial; depends on survey design)
- Race/ethnicity disaggregation outside Anglo-American countries (EU law restrictions, other countries don't collect)
- Religion-fertility intersections (Pew estimates only; sparse)
- Political affiliation (no official statistics exist; attitudes only via surveys)
- Real-time data (all sources lag 18–36 months; users must manage expectations)
- Effective TFR detail (limited to HFD, OWID calculations; not available for many countries)

**License complexity:**
- Eurostat, HFD, OWID, national offices are fully open (CC-BY or public domain) — safest for v1
- World Bank mixed (verify dataset-by-dataset)
- DHS, IPUMS restrictive (research/non-commercial use; microdata not redistributable)
- Pew, WVS, ESS, GGP: check per-dataset terms; varying restrictions

Before Phase 1 ingestion, the licensing findings above should be reviewed against the chosen monetization model and confirmed in writing with the source where the terms are ambiguous. This is solo work today, so "review with legal counsel" is something to schedule if and when funding or commercial monetization becomes a real path; until then, conservative interpretation of each license is the safer default.
