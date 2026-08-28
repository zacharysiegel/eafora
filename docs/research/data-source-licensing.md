# Data source licensing for Eafora

Research date: 2026-05-21. All findings below are from live primary-source
fetches performed at that date. Where a primary source could not be retrieved
(403, 404, JS-only, or auth wall), this is called out explicitly rather than
guessed at.

This document evaluates licensing for fertility/demographic data sources Eafora
is likely to ingest, and maps them against plausible monetization models. The
goal is to identify which sources are safe under any future monetization choice
versus which ones constrain that choice.

A short legal disclaimer up front: this is an engineering/operations review
based on public terms pages. It is not legal advice. Before shipping a
commercial product against any of these feeds, the relevant terms-of-use page
should be re-fetched and reviewed by counsel.

## Part 1: World Bank WDI

### License

The World Development Indicators (WDI) dataset is released under
**Creative Commons Attribution 4.0 International (CC BY 4.0)**, confirmed via
two primary sources:

- The World Bank Data Catalog license overview page
  (`https://datacatalog.worldbank.org/public-licenses`) states:
  > "CC-BY 4.0, with the additional terms below, is the default license for
  > all Datasets produced by the World Bank itself."
- The WDI dataset entry in the Data Catalog
  (`https://datacatalog.worldbank.org/search/dataset/0037712/World-Development-Indicators`)
  says explicitly:
  > "This dataset is licensed under Creative Commons Attribution 4.0."

CC BY 4.0 itself permits commercial use, modification, redistribution, and
mixing with material under other licenses, subject to attribution.

### Required attribution

The World Bank dataset terms
(`https://www.worldbank.org/en/about/legal/terms-of-use-for-datasets`) require
attribution in the form:

> "The World Bank: Dataset name: Data source (if known)."

Sub-licensees must either include the same attribution or reference the World
Bank terms by URL.

### No-endorsement clause (additional term beyond CC BY)

> "You may not publicly represent or imply that The World Bank is participating
> in, or has sponsored, approved or endorsed the manner or purpose of your use."

### Trademark restrictions (additional term beyond CC BY)

Use of the World Bank's name, trademarks, official emblems, or logos requires
prior written consent. This is independent of the data license — Eafora must
not use the World Bank logo or imply official affiliation regardless of how the
underlying data is used.

### Third-party data carve-out

> "Some datasets and indicators are provided by third parties, and may not be
> redistributed or reused without [the original data provider's consent]."

Per-indicator metadata in the WDI catalog must therefore be checked. Most
core WDI fertility indicators (e.g. SP.DYN.TFRT.IN — Fertility rate, total) are
World-Bank-produced compilations and fall under the default CC BY 4.0, but the
per-indicator metadata is the load-bearing source.

### API vs bulk download

Per the developer overview
(`https://datahelpdesk.worldbank.org/knowledgebase/articles/889386`):

> "The use of World Bank datasets listed in the Data Catalog is governed by a
> specific [Terms of Use for World Bank Data]. The use of the APIs is governed
> by the [Terms and Conditions]."

Both routes ultimately reach the same dataset license (CC BY 4.0 for WDI). The
API is governed by an additional Terms-and-Conditions document covering
acceptable use of the service itself; this is distinct from the data license
but does not change the redistribution rights for data retrieved through the
API.

### Verdict

WDI is the cleanest non-trivial data source available: CC BY 4.0, commercial
use permitted, modification permitted, redistribution permitted, attribution
boilerplate is short, and the only meaningful constraints (no-endorsement, no
trademark use) are easy to comply with.

## Part 2: Source-by-source audit

### UN World Population Prospects (UN DESA Population Division)

**Status: ambiguous; verify before shipping.**

I could not locate an explicit license declaration on `population.un.org/wpp/`
(several documented terms-of-use URLs returned 404, and the Population
Division does not appear to publish a CC mark on the WPP download pages
themselves).

Two adjacent UN sources give conflicting signals:

- **UNdata** (`https://data.un.org/Host.aspx?Content=UNdataUse`) states that
  data "may be copied freely, duplicated and further distributed" provided
  UNdata is cited. UNdata republishes WPP data among other UN datasets, which
  suggests broad reuse is the intent.
- **The general UN terms of use** (`https://www.un.org/en/about-us/terms-of-use`)
  grant permission "for the User's personal, non-commercial use, without any
  right to resell or redistribute them." This is a much more restrictive
  default and would block Eafora's use case.
- **The general UN copyright page** (`https://www.un.org/en/about-us/copyright`)
  reserves all rights and points back to the terms of use.

The honest answer is that the WPP-specific license is ambiguous on the
public web. In practice many third parties (including Our World in Data)
redistribute WPP data without controversy, but Eafora should not rely on that
as a license. Before ingesting WPP data into a product with any commercial
dimension, contact `population@un.org` for written confirmation of reuse
terms, or rely on a downstream republisher that has its own clear license
(e.g. World Bank or OWID) for the same indicators.

### Eurostat

**Status: CC BY 4.0 for most data; partial restrictions on third-country trade
data.**

Per Eurostat's copyright/reuse notice
(`https://ec.europa.eu/eurostat/web/main/help/copyright-notice`):

> "The copyright for the editorial content of this website, which is owned by
> the EU, is licensed under the Creative Commons Attribution 4.0 International
> licence."

> "Reuse of statistical data, metadata, publications, and other dissemination
> tools published on this website for commercial or non-commercial purposes is
> authorised provided the source is acknowledged."

Exceptions: third-party copyrighted material, logos, trademarks, and certain
trade datasets sourced from non-EU/EFTA countries have additional
restrictions. Fertility data is Eurostat-produced and falls under the default
CC BY 4.0.

### OECD

**Status: CC BY 4.0 since July 2024.**

OECD transitioned to an open-access model in July 2024 and now uses CC BY 4.0
on data and publications. The OECD-hosted terms pages
(`oecd.org/en/about/terms-and-conditions.html` and
`oecd.org/en/about/policies/oecd-licensing.html`) returned 403 to my fetcher,
so I could not quote the policy verbatim, but the transition is well-attested
in secondary sources. Re-verify the exact wording on the live OECD terms page
before shipping; if for any reason the dataset Eafora ingests is older
than July 2024 and was originally posted under a different OECD license, the
older terms may still apply to that historical snapshot.

### US Census Bureau

**Status: public domain (US federal government work) within the United States;
attribution requested but not legally required for public-use data.**

Works of US federal government employees in the course of their official
duties are not subject to copyright protection in the US (17 USC § 105).
The Census Bureau's citation page
(`https://www.census.gov/about/policies/citation.html`) confirms attribution
is a request, not a copyright restriction, for public-use data products.

International caveat: per `usa.gov/government-copyright`, "U.S. copyright laws
may not protect U.S. government works outside the country," and the US
government may assert copyright outside the US. In practice this rarely
matters for public statistics, but Eafora's apps will be distributed
internationally, so attribution should be applied as a matter of good
practice.

Restricted-use Census microdata is a separate track with its own DUA and is
out of scope for Eafora.

### CDC / NCHS

**Status: most public-use data is in the US public domain; specific data
products may require a Data Use Agreement.**

I was unable to fetch the NCHS data-use-agreement and access-restrictions
pages directly (404 on every URL variation I tried — CDC has reorganized
their site recently). The CDC's general agency-materials page
(`https://www.cdc.gov/other/agencymaterials.html`) confirms most CDC content
is in the public domain, but adds these expectations even for public-domain
material:

1. Attribute to CDC/ATSDR/HHS.
2. Include a disclaimer that use does not imply CDC endorsement.
3. Do not change substantive content.
4. Where applicable, link back to the original on cdc.gov.

For NCHS public-use vital statistics files (the source for US fertility data),
the historical pattern is that the data file itself is public domain but
Restricted-Use files require a formal NCHS Data Use Agreement administered
through the Research Data Center. Eafora should be using public-use files
only; restricted files are not appropriate for an aggregator product anyway.

Re-verify the current NCHS public-use data agreement before shipping.

### Human Fertility Database (humanfertility.org)

**Status: HFD-produced estimates are CC BY 4.0; underlying input data from
national statistical offices is NOT.**

The HFD User Agreement explicitly distinguishes two layers:

1. **HFD original estimates** — released under CC BY 4.0. Commercial use
   permitted with attribution.
2. **Input data from national statistical offices** — "should not be used
   for commercial gain or re-published in any form without the explicit
   permission of the data owners."

The HFD also "discourages" redistribution of copies, preferring users to be
referred back to humanfertility.org. That is a request, not a CC-level
restriction, but it sets the cultural expectation.

**Operational implication for Eafora:** if Eafora restricts itself to the
HFD's own derived indicators (period TFR, completed cohort fertility, parity
distributions, etc. that HFD computes), the data is usable commercially with
attribution. If Eafora ingests the raw national input tables that HFD also
hosts, those carry the original NSO terms and likely block commercial use
without per-country negotiation. The safe move is: ingest only HFD's
*output* indicators, never their input layer.

### Human Mortality Database (mortality.org)

**Status: same dual-layer structure as HFD.** HMD outputs (life tables, death
rates) under CC BY 4.0; input data from NSOs not for commercial use without
explicit permission. Less directly relevant to Eafora since the focus is
fertility, but if Eafora wants completed-cohort calculations or
mortality-adjusted fertility metrics, the same operational rule applies:
ingest only the HMD-produced outputs.

### Our World in Data

**Status: OWID's own work is CC BY; republished third-party data inherits the
original license.**

Per OWID's FAQ:

> "Most of the data on Our World in Data comes from third-party providers
> (such as the WHO, UN, and World Bank) and is subject to the license terms of
> those providers."

> "[Charts and articles created by OWID are released under] Creative Commons
> BY license."

OWID is therefore not a license-laundering shortcut. If Eafora pulls the
"Total Fertility Rate" series from OWID, that series is upstream UN WPP
data and remains governed by UN WPP terms. OWID is useful as a *reference
implementation* (their own processing scripts, indicator definitions,
country-code harmonization) but is not a substitute for going to the
original source for license clearance.

Where a series is labeled "Official data collated by Our World in Data" or
"with major processing by Our World in Data," OWID's CC BY does apply to
their derivative work — but the upstream license still applies to the
underlying data.

### Gapminder

**Status: CC BY 4.0; commercial use explicitly permitted.**

Gapminder's free-material terms permit inclusion "in commercial products or
services that you charge for." Required attribution mentions "free,"
links back to Gapminder, and includes the CC-BY designation. Trademark "GAPMINDER"
cannot appear in the product/service name.

Important note: Gapminder is itself an aggregator. Their fertility series are
mostly built on UN WPP plus historical demographic reconstruction. The same
upstream-license caveat as OWID applies — Gapminder's CC BY covers their
processing/curation work; the underlying UN data is subject to UN terms.

### IPUMS

**Status: registration required; per-collection terms; mostly redistribution-
prohibited.**

- IPUMS USA, CPS, NHGIS: no explicit commercial prohibition, but
  redistribution is generally prohibited without permission.
- IPUMS International, PMA, GeoMarker: explicit prohibition on commercial
  use.
- All collections: registration and per-user agreement required, valid one
  year.

**Operational implication for Eafora:** IPUMS is fundamentally incompatible
with an aggregator product that redistributes data. Eafora cannot ingest
IPUMS at all and republish it through its own API/app; only individual
researchers can use IPUMS, not products. Use IPUMS at most for one-off
internal analysis (e.g. validating an indicator), not as a runtime data
source.

### National statistics offices (general pattern)

Highly variable. Several common license families:

- **UK ONS** — Open Government Licence v3.0 (effectively CC BY-equivalent;
  commercial use permitted with attribution).
- **Germany Destatis** — Datenlizenz Deutschland 2.0 (DL-DE BY 2.0); commercial
  use permitted with attribution. Caveat: some datasets are DL-DE Zero
  (public-domain-equivalent), some carry stricter terms.
- **France INSEE** — Licence Ouverte / Open Licence 2.0 (Etalab); commercial
  use permitted with attribution. Effectively CC BY-equivalent.
- **Statistics Canada** — Statistics Canada Open Licence; commercial use
  permitted with attribution. Effectively CC BY-equivalent.
- **Many other NSOs** — bespoke terms; some are essentially CC BY, some
  prohibit redistribution, some require registration. Per-country verification
  is required.

The safe pattern for Eafora is to source NSO-origin data through an
intermediary that has already done license clearance (World Bank, Eurostat,
OECD) rather than ingesting NSO sites directly. Direct NSO ingestion is a
country-by-country legal review.

## Monetization-model matrix

A note on terminology: "commercial use" in CC BY 4.0 is defined broadly. The
license text (Section 2(a)(1)) grants the right to use the work "for
commercial and non-commercial purposes alike." Specifically:

- Ad-supported free apps: yes, this is commercial use, and yes, CC BY permits
  it.
- Sponsorship logos / "supported by X": yes, commercial use; permitted under
  CC BY.
- Freemium / paid tiers: permitted under CC BY.
- Selling the data itself or selling API access: permitted under CC BY (with
  attribution; the license cannot be made more restrictive than the upstream).

A NonCommercial (CC BY-NC) license would block all five of those models. None
of the sources surveyed above are CC BY-NC, but several have non-CC custom
terms (UN general, IPUMS) that effectively act like NC.

Legend: ✅ permitted, ⚠️ permitted with caveat, ❌ not permitted under that
source's terms.

| Source                      | Nonprofit/educational | Grant-funded free | Sponsorship | Display ads | Freemium / paid tier | Selling data / API |
| --------------------------- | --------------------- | ----------------- | ----------- | ----------- | -------------------- | ------------------ |
| World Bank WDI              | ✅                     | ✅                 | ✅           | ✅           | ✅                    | ✅[a]               |
| UN World Population Prospects | ⚠️[b]                | ⚠️[b]             | ⚠️[b]       | ⚠️[b]       | ⚠️[b]                | ⚠️[b]              |
| Eurostat                    | ✅                     | ✅                 | ✅           | ✅           | ✅                    | ⚠️[c]              |
| OECD                        | ✅                     | ✅                 | ✅           | ✅           | ✅                    | ✅[a]               |
| US Census Bureau            | ✅                     | ✅                 | ✅           | ✅           | ✅                    | ✅[d]               |
| CDC / NCHS public-use       | ✅                     | ✅                 | ✅           | ✅           | ✅                    | ✅[d]               |
| Human Fertility Database    | ✅[e]                  | ✅[e]              | ✅[e]        | ✅[e]        | ✅[e]                 | ✅[e]               |
| Human Mortality Database    | ✅[e]                  | ✅[e]              | ✅[e]        | ✅[e]        | ✅[e]                 | ✅[e]               |
| Our World in Data           | →[f]                  | →[f]              | →[f]        | →[f]        | →[f]                 | →[f]               |
| Gapminder                   | ✅                     | ✅                 | ✅           | ✅           | ✅                    | ✅[a]               |
| IPUMS International / PMA   | ⚠️[g]                 | ⚠️[g]             | ❌           | ❌           | ❌                    | ❌                  |
| IPUMS USA / CPS / NHGIS     | ⚠️[g]                 | ⚠️[g]             | ⚠️[g]       | ⚠️[g]       | ⚠️[g]                | ❌[g]               |
| ONS (UK), Destatis, INSEE, StatCan | ✅              | ✅                 | ✅           | ✅           | ✅                    | ✅[a]               |
| Other NSOs                  | ⚠️[h]                 | ⚠️[h]             | ⚠️[h]       | ⚠️[h]       | ⚠️[h]                | ⚠️[h]              |

Footnotes:

- **[a]** Permitted under CC BY 4.0 / equivalent open licence. Eafora's
  downstream license cannot be more restrictive than CC BY for the underlying
  data — i.e. Eafora can charge for access, but cannot prevent customers from
  re-extracting and redistributing the upstream data under CC BY. The
  proprietary "all rights reserved" framing must be applied only to Eafora's
  *own* added value (UI, code, novel processing, presentation), not to the
  upstream data itself.
- **[b]** UN WPP licensing is ambiguous on the public web (see source-by-source
  notes). Treat as blocked pending written confirmation from
  population@un.org or until an authoritative CC mark is found on the
  download pages. In the interim, source the same indicators via World Bank
  WDI (which republishes UN data under World Bank's CC BY 4.0).
- **[c]** Eurostat blocks commercial reuse of certain non-EU/EFTA trade
  datasets. Fertility data is unaffected, but if Eafora ever expands into
  trade/economic indicators, re-check.
- **[d]** US-domestic public domain. Outside the US, copyright status is
  weaker but in practice treated as freely usable. Apply attribution and
  no-endorsement disclaimer regardless.
- **[e]** Only HFD/HMD's own *derived* outputs. Their NSO *input* data layer
  carries original NSO terms and likely blocks commercial use without
  per-country permission. Ingest only the HFD/HMD output indicators.
- **[f]** OWID's own work is CC BY (commercial OK), but OWID is mostly an
  aggregator of upstream sources. Each indicator inherits its upstream
  license — Eafora must check the upstream, not OWID.
- **[g]** IPUMS prohibits redistribution generally. Even where commercial use
  is not explicitly forbidden, Eafora's product model (republishing data
  through its own API/UI) is incompatible with IPUMS terms. Use IPUMS only
  for one-off internal validation, never as a runtime feed.
- **[h]** NSO-by-NSO determination required. Default to "blocked" until the
  specific NSO's licence is reviewed.

## Recommended ingestion roadmap

### Tier 1 — safe under any monetization model

These can be ingested today without constraining future business model
choices. All are CC BY 4.0 or equivalent open licences. Attribution and
no-endorsement boilerplate is the only obligation.

- **World Bank WDI** — start here. Has wide country coverage, includes UN-
  sourced fertility indicators republished under World Bank's clear CC BY,
  and the API is well-documented.
- **OECD data** — for OECD member countries, often higher quality and more
  recent than WDI.
- **Eurostat** — for EU/EFTA fertility data; CC BY 4.0 with explicit
  authorization for commercial reuse.
- **Gapminder** — useful for historical reconstruction of fertility back into
  the 19th century where modern sources don't reach.
- **US Census Bureau and CDC NCHS public-use** — for US-specific indicators.
- **Major NSOs with open licences** — UK ONS, Destatis, INSEE, Statistics
  Canada — when more granular national data than WDI/Eurostat is needed.

### Tier 2 — usable but constrained

- **HFD / HMD outputs (not inputs)** — high-quality cohort fertility
  estimates that nothing in Tier 1 matches. Ingest only the HFD-derived
  output indicators, never the underlying NSO input tables. Attribution
  follows HFD's citation format.
- **Our World in Data** — not ingested. Each indicator inherits its upstream
  license, so a series taken from OWID is governed by whoever produced it;
  `docs/architecture/ingestion.md` §Our World in Data records the decision.

### Tier 3 — verify before shipping

- **UN World Population Prospects** — the canonical fertility forecasting
  series, but its public license posture is genuinely unclear. Either
  (a) source the same indicators via World Bank WDI, which has clear CC BY,
  or (b) get written confirmation from UN DESA Population Division before
  ingesting directly. Until clarified, treat direct WPP ingestion as blocked
  for any product with a commercial dimension.
- **Other NSOs without a documented open licence** — country-by-country
  review required.

### Tier 4 — incompatible with Eafora's product model

- **IPUMS** (any collection). Per-user registration model and redistribution
  prohibition mean Eafora cannot use IPUMS as a runtime feed. Acceptable
  only for one-off internal analysis by individual researchers.
- **HFD / HMD raw input layer.** Carries original NSO terms.

### Operational guardrails to establish from day one

1. **Track per-indicator provenance and licence in the data model**, not just
   per-source. WDI republishes UN WPP; HFD republishes NSO inputs; OWID
   republishes everything. License questions are per-indicator, not
   per-source.

2. **Render the attribution string and license name on every visualization
   and every API response that ships upstream-licensed data.** CC BY 4.0
   compliance is mostly about visible attribution; making this automatic
   eliminates a class of compliance bugs.

3. **Render a no-endorsement disclaimer in the app's About / Legal page**
   covering at least World Bank, OECD, Eurostat, US federal agencies, and
   the UN. The wording is similar across all of them.

4. **Do not use upstream logos or trademarks in Eafora's UI** unless an
   explicit trademark licence has been obtained. The "no endorsement" clauses
   in World Bank, OECD, and CDC all imply this.

5. **Eafora's proprietary "all rights reserved" license applies only to
   Eafora's own added value** — the application code, UI design, novel
   processing pipelines, and any genuinely new analyses Eafora produces. It
   cannot apply to the upstream CC BY data flowing through. This is
   compatible with both CC BY (the upstream license) and Eafora's commercial
   intent, but the marketing copy and Terms of Service need to reflect this
   distinction or Eafora will be misrepresenting what it owns.

6. **Re-verify all licenses immediately before any monetization launch.**
   Terms pages change. The OECD's CC BY 4.0 transition (July 2024) is a
   recent example of this happening in Eafora's favor; the reverse can also
   happen.
