# Product plan

<!--
Status: draft, 2026-05-21. This document frames Eafora as a product and as a thing someone might fund. It deliberately stays high-level — most "what" and "how" questions are answered in the constitution, the data sources survey, and the architecture overview. This document answers "why," "for whom," and "how does it become sustainable."

The user's intent for this document (paraphrased from the initial brief): "essentially our business plan / product proposal — something I could use to sell the idea to someone, e.g. to get a nonprofit to pay me to build it."

Editorial note: per Constitution Principle I, the **product itself** is strictly neutral and contains no editorial copy. This document is a pitch artifact, not user-facing content; framing the work as worth doing is appropriate here. Care has been taken not to slip into advocacy language — the rationale stays "this data is hard to explore and that is a fixable problem," not "fertility is declining and that is bad."
-->

## What Eafora is

Eafora is an interactive global atlas of fertility data: total fertility rate, completed fertility rate, age-specific fertility, marriage and family formation indicators, and (over time) the broader demographic context that makes those numbers legible. It targets the web first, with native iOS and Android apps to follow.

Users land on a world map. Countries are colored by an indicator the user picks (TFR by default). Hovering a country gives a quick summary; clicking zooms in and opens a detail panel with the time series, the data status (final / preliminary / projection), and direct links to the primary source and the relevant Wikipedia article. Province- and state-level overlays appear where the source data permit them. Everything is fast, everything is sourced, nothing is editorialized.

The mission, in one sentence: **be the easiest place on the internet to look up, compare, and follow the source trail for global fertility statistics.**

## What problem this solves

Several real problems share a shape:

1. **Fertility data is fragmented.** The authoritative numbers live across UN World Population Prospects, World Bank, Eurostat, Human Fertility Database, dozens of national statistical offices, and academic survey programs. Each publishes on its own schedule, in its own format, under its own license, and in its own UI. A researcher who wants TFR for Spain since 1960 has to know which dataset to trust, where to download it, and how to reconcile it with neighbors.
2. **General-audience tools are shallow; expert tools are inaccessible.** Wikipedia has accurate but static text. World Bank's data portal has the numbers but a forbidding UI. Statista hides everything behind a paywall. Our World in Data is the closest existing match for what Eafora wants to be — and it's deliberately broader; fertility is one of hundreds of topics they cover.
3. **Provenance is rarely visible.** When a statistic shows up in a news article, the chain of custody — which publication, which revision, which retrieval date, what license — is almost never reachable in one click. For data this politically sensitive, that's a credibility gap.
4. **Sub-national, ethnic, religious, and other disaggregations are scattered across heterogeneous sources** and are absent or unreliable in many countries. Surfacing what exists, with caveats about what doesn't, is itself a service.

None of these are unsolvable in principle. They're solvable through patient aggregation and careful UX. That's the work.

## Who uses Eafora

Realistic primary audiences, ordered by how much value Eafora plausibly delivers per user:

| Audience | What they get | How often they show up |
|---|---|---|
| Demographics-curious individuals | Fast, beautiful, citable look-ups; a way to satisfy a question that came out of a news story or a conversation | Episodic; bursts during news cycles |
| Journalists | Source-linked statistics they can drop into stories with confidence; comparison views for context | Per-story basis |
| Policy researchers (early-career) | A first-pass aggregator before they go to primary sources; the bibliography Eafora builds for them | Regular; project-driven |
| University students (sociology, economics, demography, public health) | A teaching tool and a homework starting point | Heavy during semester |
| Nonprofit staff (pro-natal, family-formation, public-health) | Slides and quick-hit statistics for funder updates and outreach material | Project-driven |
| Foreign / development desks | Country comparison slices; cohort effects | Episodic |

The product is general enough that it serves all of them with the same UI; the differentiation is depth of indicators and quality of provenance, not segment-specific features.

What Eafora is **not** built for: real-time dashboard use, government statistical analysis, or academic-grade microdata exploration. Those audiences have their own established tools (Stata, R, IPUMS extracts) that Eafora can't and shouldn't compete with.

## Product scope by phase

The architecture overview spells out the technical phasing. The product narrative is parallel:

### v1 — credible MVP

A web map with TFR for ~200 countries, time series back to 1960, color-graded fills, hover and click interactions, and a country detail panel showing the time series chart plus source/Wikipedia links. Single primary source for the indicator (World Bank WDI; the licensing on World Bank is the cleanest of the major aggregators per the licensing research). Static-ish artifact regenerated periodically; no live API; no user accounts.

This is enough to share with a few dozen people, link from social media, get feedback, and build credibility. It's not enough to charge for, and it's not enough to claim "leading aggregator" — it's a demo that the architecture works.

### v1.5 — broader public soft launch

Add 3–5 more indicators (CBR, CDR, ASFR, mean age at first birth) sourced from Eurostat and HFD where they exceed World Bank's coverage. Add subnational overlays for the EU (NUTS-2/3) and the US (states). Polish the country detail panel: small charts for each indicator, family-formation indicators where available. Ship the iOS app to TestFlight; ship the Android app to internal testing.

This is the version worth posting on Hacker News, sharing with a journalist, and pitching to a funder. The map is rich enough to be genuinely useful; provenance is fully visible; subnational drill-down distinguishes Eafora from World Bank's data portal.

### v2 — production app on three platforms

iOS in the App Store; Android in the Play Store; the web app on a custom domain. Preliminary-data freshness from CDC NCHS, ONS, ABS, Eurostat flash estimates so "current year" is actually current. Cross-source merge with documented preference order; per-cell `data_status` flagging (final / provisional / projection). Country comparison view (pick two countries, see indicators side-by-side). Population pyramids. Mean ages, parity distributions where available.

This is the version that earns the "leading aggregator" claim — in scope, in freshness, and in provenance integrity. It's also the version where monetization becomes plausible.

### v3+ — depth and contributions

Disaggregations: TFR by ethnicity (US, UK, Australia, Canada — the limited set where official statistics permit), by educational attainment (DHS where applicable). User-submitted corrections with moderation. Semantic search over the dataset. A live API for third parties to embed. A formal mobile-app v2 release with offline-first caching and push notifications when "your tracked country" gets a data update.

v3 is also where the project plausibly outgrows a solo nights-and-weekends shape and either dies, finds funding, or finds a partner.

## How Eafora differentiates

Honest competitive landscape:

| Alternative | What they do well | Where Eafora wins |
|---|---|---|
| **Our World in Data** | Broad topic coverage, beautiful charts, clean licensing, world-class explainer writing | Eafora is *only* fertility / family / demography — depth, not breadth. Eafora has interactive country drill-down, subnational overlays, and provenance per cell rather than per chart. OWID's strength is editorial; Eafora's neutrality is a feature for some audiences who don't want the editorial layer |
| **World Bank Data portal** | Authoritative for the indicators they cover; clean API | Eafora's UI is built for browsing, not querying; Eafora merges WB with sources WB doesn't have (HFD, national stat offices); Eafora's sub-national support |
| **UN WPP browser** | Authoritative for global projections | Same UX argument; Eafora layers in non-WPP sources |
| **Wikipedia** | Source-linked text; widely trusted | Wikipedia is text-first; Eafora is data-first with the same source-linking principle |
| **Statista, Macrotrends** | Polished UI; aggressive SEO | Both gate data behind paywalls or interstitials. Eafora is free and source-transparent |
| **Pew Research** | High-quality survey data on family attitudes | Pew is survey-and-essay shaped; Eafora doesn't compete, complements |

The defensible position is: **the easiest place to look up a fertility number, compare neighbors, drill into a country, and trace the citation, without paying or wading through a research portal.** Nothing in that sentence is technically novel — but no one currently optimizes for exactly it.

## Monetization options

The user's stated brief: "I don't expect this to be a great business or anything." That's a healthy starting position. The goal of this section is not to identify the optimal revenue model — it's to enumerate the plausible ones honestly so a funding pitch can land on whichever model the funder prefers, and so the product stays compatible with future shifts.

The constitution's Principle I (educational neutrality) and Principle VI (no live data API through v2) constrain monetization. Anything that requires Eafora to take editorial positions is out. Anything that requires per-user accounts, gating, or a live API in v1 is out. What remains is real but specific.

### A. Nonprofit grant funding

The natural first fit. Foundations active in this space — pro-natal-leaning think tanks, family-policy nonprofits, demographic-research foundations — fund work that improves public legibility of demographic trends. Eafora is a textbook "research infrastructure" project: a tool that other people can cite, embed, and build on.

| Aspect | Note |
|---|---|
| Scale potential | Small-to-medium grants ($25k – $200k typical for digital research-infrastructure projects); occasionally larger |
| Cycle | Multi-month application processes; funding flows are 6–18 months out |
| Friction | Reporting requirements; specific-deliverables framing; possible mission-creep pressure |
| Editorial fit | Excellent. Grant-funded research infrastructure is the canonical funding model for this shape of project |
| Compatibility with the constitution | Full. Funders typically don't ask for product editorial direction; the neutrality principle protects against that pressure |
| Realistic v1 path | Approach 2–3 funders with the v1.5 demo in hand. Pitch a one-year grant to fund v2 build-out (preliminary-data integration, native apps, subnational depth) |

### B. Pro-natal organization sponsorship

Adjacent to (A) but more transactional. A pro-natal nonprofit might pay Eafora to feature them prominently — logo on the landing page, a "supported by" line in the country detail panels, or a sidebar pointing users to their resources.

| Aspect | Note |
|---|---|
| Scale potential | Modest; $5k–$50k annual sponsorships are realistic for organizations with mid-five-figure budgets |
| Friction | Negotiating prominence; risk of perceived editorial capture |
| Editorial fit | **Risky.** A logo at the bottom of the page is fine. A sidebar promoting their viewpoint inside the country detail UI starts to leak into editorial territory and would conflict with Principle I |
| Compatibility with the constitution | Conditional. Sponsorship that buys placement (clearly labeled as such) is OK. Sponsorship that buys editorial influence is not |
| Realistic path | Defer to v2+. The v1.5 product needs to look like a credibly neutral source before sponsor placements appear; otherwise Eafora gets read as "the [SponsorName] map" |

### C. Pro-natal organization advertising (separate from sponsorship)

The user has explicitly raised this as an option. Different from (B) in that ads appear in clearly-marked slots, multiple advertisers compete, and there's no implication that Eafora endorses any of them. Standard model: sell ad space to pro-natal nonprofits, family-policy think tanks, fertility clinics, family-formation services.

| Aspect | Note |
|---|---|
| Scale potential | Modest at low traffic; only meaningful at v2+ scale (10k+ daily uniques) |
| Friction | Need an ad-serving infrastructure (could be as simple as static slots refreshed manually); need ad inventory (sales work) |
| Editorial fit | Good if ads are clearly separated from data UI and clearly labeled. **Bad** if ads creep into the country detail panel itself or use the indicator color scheme. The line is the same line that protects the editorial neutrality of newspapers |
| Compatibility with the constitution | Conditional. The ads themselves are sponsor copy, which Principle I forbids in the product UI. Acceptable interpretation: ads are not "product" but commercial inventory hosted alongside the product, in clearly-marked slots that are visually and structurally separate from data presentation |
| Realistic path | v2.5+. Need traffic before sales conversation makes sense |

### D. Educational institution licensing

Universities and secondary schools occasionally pay for hosted versions of demographic tools (logo on top, no ads, possibly LMS integration). Statista does well here.

| Aspect | Note |
|---|---|
| Scale potential | Plausibly significant if Eafora becomes a reference tool in undergrad sociology / economics / public-health curricula |
| Friction | Long sales cycles to procurement offices; need .edu vouching; LMS integration is a real engineering ask |
| Editorial fit | Good. Universities want neutrality |
| Compatibility | Full |
| Realistic path | v3+. The product needs to be in syllabi before institutional licensing makes sense, and getting into syllabi takes years |

### E. Premium / pro tier

Free for casual use; paid tier for power features (CSV export, comparison view, API access, embed widgets, ad-free). Spotify-shape monetization.

| Aspect | Note |
|---|---|
| Scale potential | Plausibly the largest if the product takes off; conversion rates in research-data-tools are typically 1–3% of regular users |
| Friction | Have to build account systems, billing, subscription management — all of which the v1–v2 architecture explicitly avoids per Principle VI |
| Editorial fit | Good |
| Compatibility | Requires a v3+ architectural shift to support live accounts; doable but a big lift |
| Realistic path | v3+ at earliest, and only if the free product has a meaningful audience |

### F. Donations

The Wikipedia / Our World in Data model. Banner asks during fundraising drives, plus a Patreon-style recurring-donor option.

| Aspect | Note |
|---|---|
| Scale potential | Tens to hundreds of dollars per month for a small audience; tens of thousands per year if Eafora hits OWID-scale traction (which is unlikely without a multi-year arc) |
| Friction | Low setup cost; ongoing fundraising cycles |
| Editorial fit | Excellent |
| Compatibility | Full |
| Realistic path | Available from v1.5 onward. Probably won't fund the project alone but may fund hosting + domain |

### G. Embedding fees / API for commercial users

Once a v3 live API exists, license commercial use to media organizations and dashboard tools that want to embed Eafora's data. Free for non-commercial use.

| Aspect | Note |
|---|---|
| Scale potential | Niche but per-customer revenue can be meaningful ($500–$5k/year per commercial embed) |
| Friction | Need to enforce the commercial/non-commercial distinction; need a sales-and-billing layer; need some commercial users to want to embed Eafora rather than rebuild |
| Editorial fit | Good |
| Compatibility | Requires v3+ live API. Note: respecting upstream source licenses (some of which prohibit commercial redistribution) constrains what can be in the commercial-license-able tier |
| Realistic path | v3+ if it materializes naturally; not worth designing for now |

### Recommended monetization narrative for a pitch

For a funder pitch *today*, the honest narrative is:

> Eafora is being built for free as research infrastructure. Operating costs through v2 are well under $50/month plus the Apple Developer Program fee. The work is funded by the founder's own time. We are seeking a one-year grant of $X to fund the v2 build (subnational depth, preliminary-data integration, native mobile apps), after which the product stays free and open and continues to be maintained on volunteer time. Long-term sustainability planning includes a mix of donations, sponsorship of clearly-marked slots by aligned organizations, and (eventually) a paid commercial-embedding tier — none of which would compromise the product's editorial neutrality.

This narrative:
- Doesn't oversell. The user has explicitly said this isn't expected to be a great business.
- Asks for a specific, finite amount tied to a specific deliverable.
- Names the constraints honestly (editorial neutrality limits what we can do).
- Leaves doors open without committing to any specific direction.

A different funder (a tech-skeptic family foundation, say) might prefer (B) sponsorship; a different one (a public-health funder) might prefer (D) educational licensing. The pitch can be tailored.

## Risks

A short, honest list — not exhaustive:

1. **Solo-build sustainability.** The biggest risk. The architecture optimizes for solo nights-and-weekends, but Eafora's "leading aggregator" claim only really works at v2+ scope, which is a multi-quarter build. If the project stalls between v1.5 and v2, it dies in a familiar way.
2. **Politically contested map borders.** App Store rejections, social-media-driven controversies, and even legal pressure are real outcomes for world-map apps. The constitution's "default to US-recognized lines, design data layer for swap" mitigates but doesn't eliminate; first App Store submission is the real test.
3. **Source license drift.** The data-sources licensing matrix is a snapshot. Sources can change terms (UN WPP's licensing was already ambiguous in this snapshot; HFD's dual layer requires care). Eafora's provenance integrity makes this manageable but not free.
4. **Statistical-source disagreements.** Different sources publish different numbers for the same country and year. Eafora's design surfaces both — but a journalist who copy-pastes the wrong one and gets corrected will blame the tool. Need clear UX around the merge order and the confidence each value carries.
5. **Topic sensitivity.** Fertility decline is a politically polarized topic. Eafora's neutrality is real, but it will still be misread by people who skim. Clear "we are not advocacy" framing in the about page, plus disciplined UI copy review, mitigates.
6. **Funding fit mismatch.** The most likely funder profile (pro-natal think tanks, family-policy foundations) overlaps imperfectly with the editorial-neutrality stance. A funder who wants Eafora to "make the case" will be the wrong funder; saying no to that money matters.
7. **OWID launching their own fertility-focused product.** Plausible. If they did, Eafora would have to lean harder into depth (subnational, preliminary data, family formation indicators) where OWID's broader scope works against them.
8. **The owner's day job at Apple constraining iOS distribution.** Apple's external-technology policy may or may not apply; the architecture overview flags this as something the owner must verify before submitting to the App Store.

## What a funding ask looks like

This is a sketch — the real ask gets refined when there's a real conversation with a real funder.

### One-year grant for v2 build

**Total ask**: $40k–$80k (range reflects whether the founder is buying back day-job hours or working on top of them; the higher number assumes a sabbatical equivalent for ~6 months)

**Deliverables**:
- All v1.5 work landed publicly: 5+ indicators, EU/US subnational overlays, time-series charts in country detail panel, native iOS app on TestFlight
- v2 work: preliminary-data integration (CDC NCHS, ONS, Eurostat flash); native iOS app in App Store and Android in Play Store; comparison view; population pyramids
- A public, citable, neutral aggregator with all primary anglosphere + EU sources integrated and freshness within 6 months of source publication
- All source code public under a license to be determined (probably permissive open source by v2 launch — the constitution's "license revisit" follow-up TODO)

**Reporting**: quarterly progress updates with public-facing changelog; final write-up with usage statistics, source coverage matrix, and qualitative outcomes

**Stewardship after the grant**: the architecture is built for $5–25/month operating cost; the founder commits to maintaining the deployed version for at least two years post-grant on volunteer time, with the data pipeline running on a scheduled basis

### Smaller wedge: pure infrastructure / hosting fund

**Ask**: $5k for two years of hosting, domain, Apple/Google developer accounts, and any paid data-source access fees

**Deliverable**: keeping the lights on at v1.5 quality. Useful as a starter conversation with a smaller funder

## Open questions before pitching

The user should decide these before walking into a real funder conversation:

1. **What's the founder's hour rate / opportunity cost?** This anchors the grant ask. Apple compensation is the reference; a "buy back X hours of my time" framing requires being able to name a number.
2. **Open-source timing.** The constitution flags license-revisit as a TODO before public release. A funder will almost always want public source eventually; pre-committing to open-source-by-v2 may be a positive signal in pitch conversations.
3. **Specific funder shortlist.** The plan above doesn't name funders — that's research that needs doing for any real pitch. Pro-natal foundations, family-policy think tanks, demography-research funders, and "civic data" infrastructure funders are the four buckets to research.
4. **Apple-employee external-work clearance.** This needs to be sorted before any external pitching. A funder will be uncomfortable funding a project the founder may not be able to ship.
5. **The team-of-one question.** Pitch documents typically introduce "the team." Saying "the team is one person who is also a full-time engineer at Apple" is fine; saying "the team is two people and we plan to bring in a designer" requires bringing in a designer first.
6. **Branding investment.** Eafora as a name is locked. Logo, color system, type, and a one-page "about" deserve work before the first funder conversation. Defer to a designer with a small specific scope, or DIY with a tasteful default.
7. **Press strategy for v1.5 launch.** Hacker News, a few research-Twitter accounts, demographic-Substack writers, and one journalist reach are the leverage points. Plan it.

## Follow-up work

Subsequent branches that depend on or extend this document:

- `docs-product-funder-shortlist` — research a concrete list of named funders (pro-natal foundations, family-policy think tanks, "civic data" infrastructure funders) with each one's grant size, fit, and application cycle. Distinct from this strategic doc; takes real work to get right.
- `docs-product-pitch-deck` — when there's a real funder conversation pending, distill this document into a 6–10 slide deck plus a 2-page proposal. Defer until needed.
- `docs-product-launch-plan` — the v1.5 public-launch plan: who to contact, what to post where, what to have ready. Defer until v1.5 is built.
- `docs-architecture-{web,ios,android,ingestion}-client` — the per-segment architecture plans flagged in `docs/architecture/overview.md`.
- `docs-claude-md-rewrite` — fold locked decisions into `CLAUDE.md`.
