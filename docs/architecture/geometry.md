# Geometry layers

How boundary geometry beyond country outlines reaches a client. Country geometry itself is covered by `ingestion.md` §Geometry ingestion; this document covers what happens once a second boundary source arrives, which Eurostat's NUTS regions are the first case of and US states and Canadian provinces the next.

## Two bundles, one swap

The producer already emits two bundle variants per build, `complete` for the CDN and `downsampled` for the embedded first-paint tree. Today both carry the same geometry file. They diverge instead:

- `downsampled` carries countries only, at Natural Earth's 1:50m, as the first-paint starter. It stays small because the embedded bundle answers to a 2 MB cap.
- `complete` carries the whole layer: every subnational level a source supplies, countries elsewhere, all reconciled into one file.

A client renders the embedded starter, then upgrades to the complete layer when the live bundle arrives — the path every visitor already takes once per load. `Renderer::refresh_country_geometry` rebuilds the vertex buffers when the incoming bundle names a geometry file it was not built from, so the upgrade reaches the map's shapes and not only its colours.

Whole-layer replacement rather than splicing a second layer into the loaded one. The country layer it replaces is approx. 500 KB compressed, discarded once per published version rather than once per visit, since artifacts are content-addressed and the cache keys on the hash. The consequence to watch is that the complete layer grows monotonically as sources are added, and every reader downloads all of it regardless of where they look. Second paint has an 8 MB target against 1.19 MB used as of the compression work, so the headroom is wide; when it is not, the escape hatch is per-layer artifacts fetched on demand, which is a change to what the manifest lists and to how the renderer keys its buffers, not to anything below them.

## Levels in one layer

Every level ships in the same file rather than one file per level, and each feature carries the level it represents. A reader switching granularity then costs nothing but a redraw: the vertices are already resident, so there is no fetch and no buffer rebuild at the moment of the switch. The alternative pays a network round trip on an interaction a reader will repeat.

Nesting is what makes the level property load-bearing. A point in Germany falls inside a NUTS-1, a NUTS-2 and a NUTS-3 polygon, so both the draw path and the hit test select on level first: the hit test filters to the active level before testing containment, and the draw path scopes to that level's spans the way emphasis draws are already scoped to a feature's range. Without the filter the first polygon the spatial index happens to return would win.

The country remainder is shared across levels. Every level of one classification covers the same countries, so subtracting any level's footprint from the coarse source leaves the same remainder, and it is stored once and drawn at every level. The rule a client applies is therefore "features at the active level, plus the remainder", not level equality, with §When a level has nothing to show adding the one case where a covered country is drawn whole instead of subdivided.

What this costs is duplicated coastline: a country covered at three levels stores its coast three times, since each level is a separate polygon set. Internal borders are not duplicated, only the outer ring. The lever if a measured byte count ever objects is the same topology encoding §No generalisation reaches for, which stores a shared border once.

What it requires first is that per-feature emphasis state stop living in a fixed uniform array. All levels resident at once is on the order of 2,200 features against a ceiling of 1,024, so the container has to move before anything renders; `docs/backlog.md` carries the replacement and the reason the per-region shape itself is kept.

Which levels a reader may choose comes from the manifest, being those with both geometry and values, rather than from a constant. A bundle carrying only countries and NUTS-2 offers exactly those two.

## When a level has nothing to show

A country whose regions all lack a value at the active period draws whole, as a country, rather than as its subdivisions in uniform no-data grey. The United Kingdom is the case that forces it: its regional series ends around 2018, so at 2024 every one of its 170 districts is empty, and drawing them subdivided says "we have 170 things to tell you about Britain" while telling the reader nothing. Drawn whole it carries the country-level value a source does hold.

The fallback is per country and per period, and it triggers only when the whole set is empty. Partial gaps stay partial: three empty districts out of Germany's 400 is a fact about the data and is drawn as such.

What this costs is the coarse polygon the subtraction was throwing away. §Reconciling cut a fully covered country's outline off precisely because its regions covered that ground, so nothing remained to fall back to. The layer therefore keeps both shapes for a covered country, and the rule picks one per period.

Two things follow that the rule above cannot express on its own, and both are met by one feature property: each feature names its parent's region code, empty for a country.

The first is telling a fallback shape from a remainder. Both are country-level features, so drawing every country-level feature at a subnational level would draw a covered country whole *and* subdivided. A country-level feature is a fallback shape exactly when some feature at the active level names it as parent, which the property answers directly.

The second is evaluating the fallback at all. Asking whether every region in a country is empty means grouping the active level by country, and a client holds neither the region tree nor anything to derive it from. Truncating a region code would work for NUTS and fail for every scheme that does not nest by prefix, ISO 3166-2 among them, so the producer states the parentage rather than leaving it to be guessed. It also happens to be what the deferred item in `docs/backlog.md` needs to follow parentage across a level switch.

It also decides where a covered country's whole shape comes from. Because the set of countries drawn subdivided now changes with the scrubber, the border between a subdivided country and a whole one moves per period, and a subtraction computed once against a coarse source cannot follow it. So a covered country's whole shape is the union of its own subnational polygons: same source, same scale, agreeing with its neighbours' regions vertex for vertex, so no sliver can open along that border whichever way the rule falls. The coarse source is left to cover only the countries that have no subnational polygons at all.

## No generalisation

Subnational geometry ships at whatever resolution its source publishes. EuroGlobalMap's NUTS regions are drawn at 1:1M against Natural Earth's 1:50M for countries, and mixing the two is not a defect: finer detail below a pixel is invisible rather than wrong, it pays off when zoomed in, and a finer coastline has nothing to disagree with because the sea is background.

Simplifying is what would create defects. Douglas-Peucker applied to each polygon independently pulls shared borders apart, because a border simplified twice in two contexts no longer agrees with itself, and the result is slivers and gaps along every internal boundary. Doing it safely means simplifying the boundary network rather than the polygons: extract each shared border once, simplify it once, rebuild polygons from the shared arcs, which is what TopoJSON encodes. That is the tool to reach for if a measured byte count ever demands generalisation. Until a number demands it, geometry is emitted as published.

## Reconciling two sources into one layer

Two boundary sources drawn at different scales disagree about where a shared border runs. Where the fine source's coverage ends, its border and the coarse source's border do not coincide, so drawing both leaves overlapping or bare slivers, and the pass has no depth test to arbitrate per pixel.

Matching the two datasets vertex by vertex is the expensive answer and fails silently: deciding which runs of vertices describe the same border needs a tolerance, and independently generalised renderings of a river border differ by kilometres. Subtraction is exact instead, and runs in the producer:

1. Union each covered country's fine polygons into that country's whole shape, and emit it as that country's country-level feature. This is the shape §When a level has nothing to show falls back to, and it comes from the fine source so it agrees with its neighbours' regions exactly.
2. Emit every fine polygon unchanged, each keyed to its own region code.
3. Union every fine polygon into one footprint, subtract it from each coarse country polygon, and emit whatever remains. A country wholly inside the footprint leaves nothing and disappears, its whole shape having come from step 1 instead; one partly overlapped keeps the part outside; one the footprint never touches is unchanged.

Step 3 runs once for the whole classification rather than once per level, because every level covers the same countries and so shares one footprint. The footprint can be taken at the finest level and reused: a coarser level is a union of finer polygons, so its footprint is identical by construction, and step 1's per-country unions are the same operation grouped by country rather than globally.

No overlap can survive, because every emitted coarse piece has had the fine area cut out of it, and no hole can open, because the only area removed is area a fine polygon now fills. Both hold from the operation rather than from a tolerance, and the covered countries need no separate exclusion list — subtraction removes them from the coarse source.

The two disagreement cases resolve on their own. Where the fine regions cross a coarse border, that strip leaves the coarse neighbour and draws as the fine region. Where they stop short of it, the strip survives the subtraction and draws as the country it belonged to.

`BooleanOps::difference` in geo carries this, alongside the `union` the writer already uses to fold territories into their sovereign. Three concerns attach to it, none algorithmic:

- Cost. Cutting every country against a footprint of over a thousand polygons is wasteful. Prefilter by bounding box so only countries touching the footprint are cut, and reuse step 1's per-country unions as the operands so each subtraction works against a small shape. The producer runs weekly, so its slowness costs nothing a reader sees.
- Robustness. Floating-point boolean operations leave hairline slivers along coincident edges and misbehave on degenerate rings, which real boundary data contains. Remainder rings below an area threshold are discarded, and the threshold is a named constant carrying both its reason and the licence clause it answers to, since discarding area is the one step that could make the data say something untrue.
- Verification. The property wanted is that every point in the reconciled area falls inside exactly one feature at a given level, which a grid scan asserts directly — exactly one, so overlaps fail the test as well as holes. Conserved total area is the second assertion. This is the test that settles whether the library behaved, and it is worth more than reading its source.

## National outlines

A feature's outline is its polygon's edges: the renderer strokes the same rings it fills. So a subdivided country has no border of its own being drawn; the national border appears only as the outer edge of its subnational regions, stroked at the same weight as the regional borders inside it, and the country reads as a uniform mesh rather than as a country subdivided.

Making national borders read heavier needs no new geometry. §Reconciling already emits each covered country's whole shape, unioned from its own regions, because §When a level has nothing to show falls back to it; stroking that same feature more heavily at a subnational level is a render decision rather than a producer one. Same source as the regions, so the two agree vertex for vertex.

## Boundary sources

- **Countries: Natural Earth 1:50m.** Public domain, no obligations. Unchanged.
- **NUTS regions: EuroGlobalMap, via Open Maps for Europe.** Its `BND` coverage carries a `NUTS_3` feature class with `NUTS_CODE` (five characters, "as defined and published by Eurostat") and `NUTS_LABEL`. NUTS-2 and NUTS-1 are derived by unioning NUTS-3 on the code's four- and three-character prefixes, since NUTS codes nest by prefix.
- **Türkiye: Natural Earth admin-1.** Public domain, like the countries layer. Its 81 Turkish provinces match Eurostat's 81 Turkish NUTS-3 regions one to one with nothing left over on either side, which follows from Turkish NUTS-3 being defined on the provinces themselves; grouping them on the four-character prefix yields exactly the 26 NUTS-2 regions Eurostat publishes. The two sources name a province with unrelated identifiers, ISO 3166-2 and a NUTS code, and `subdivision` holds the pair so the join is seed data rather than a mapping the pipeline consults.
- **Montenegro: nothing needed.** Eurostat publishes it as a single region at every level, so the country outline already is its NUTS geometry.
- **Not GISCO.** Eurostat's own NUTS boundaries are the better fit — pre-generalised at five scales, per level, a few megabytes — but they are EuroGeographics material carved out of Eurostat's reuse policy, and their terms require that "the data will not be used for commercial purposes". EuroBoundaryMap licensing starts at €6,600 for European coverage at the smallest user band.

Both questions the specification leaves open were settled from the distribution itself rather than from the WFS the coverage probe used, whose counts it does not agree with. `NUTS_3` is partial, as its optional status allows: 16,899 polygon parts carrying 1,411 distinct codes across 34 countries, with Montenegro, Türkiye and Serbia absent. 1,399 of the 1,558 seeded NUTS-3 codes have a polygon; it holds 12 the seed lacks, among them Svalbard, Jan Mayen and UK codes postdating Eurostat's retired UK series.

It follows no single revision, and asking which one it follows is the wrong question. Its `beginLifespanVersion` spans 2018 to 2024, because the layer is assembled from national contributions made at different times. So a code is reconciled against Eurostat's own `IS_STANDARD_CODE` annotation, which says whether Eurostat still disseminates it, rather than against a revision label. That annotation is also what showed the seed to be carrying superseded codes of its own, since a code can be labelled with the current revision and be obsolete within it.

Acquisition is this source's one real obstacle, and it is not technical. The distribution is a 507 MB shapefile whose download link is minted server-side and sent by email after a registration form, and the site's client exposes no download route at all. So the file arrives by hand and is kept outside the repository, which departs from the pinned-release fetch every other source uses and means a rebuild depends on a local copy rather than on the network. The WFS the coverage probe went through is not an alternative: it serves `NUTS_3` alone, in GeoJSON alone of the formats it advertises, and requires a credential the publisher does not offer as a public API.

## Licence obligations

EuroGlobalMap is the only boundary source carrying obligations; Natural Earth is public domain. The licence grants reproduction, communication to the public, adaptation, distribution, extraction and reutilisation, and states commercial exploitation among the purposes it permits, so every step here is a granted right: taking `NUTS_3` alone is extraction, deriving the coarser levels is adaptation, and serving the file to a browser is reutilisation. Nothing requires a derivative to be marked as modified and nothing imposes share-alike.

What is owed is the licence's full attribution statement, not the familiar symbol-and-year:

> This dataset includes Intellectual Property from European National Mapping and Cadastral Authorities and is licensed on behalf of these by EuroGeographics. Original dataset is available for free at https://www.mapsforeurope.org. Terms of the licence available at https://www.mapsforeurope.org/licence All attribution statements can be found here.

with "here" linking the licensor's attributions list. The shorter `© EuroGeographics <year>.` is a fallback the licence conditions on the full statement being unusable, which a map legend and a detail panel are not.

That resolves the year, by removing it: the full statement carries none. The year exists only in the fallback, where the licence states no rule for what it refers to, and the licensor's own page fills it with whatever year the reader loads the page in, which is why its archived copy reads 2024 where the live page reads 2026. A year in our attribution string would be an artifact of the day someone read that page.

**Great Britain brings a second rightsholder.** Its polygons are in the layer: EuroGlobalMap lacks 138 of the 1,558 seeded NUTS-3 codes while the United Kingdom alone accounts for 170, so it cannot be among the absent. The licensor's attributions list gives Great Britain an Ordnance Survey line, Crown copyright and database right under the Open Government Licence v3, with its own year placeholder that is not the EuroGeographics one. Publishing the layer without that credit omits a third party's licence rather than wording one badly.

The statement belongs with the data, not only in the interface. The obligation reads "within every use of the Dataset", and both the rendered map and the published geometry file are uses; a file addressable by its own URL reaches readers who never load the client, and handing it to them is the sub-licence the passthrough condition governs. So a plain-text attribution and licence file sits beside the geometry in the published tree, and the map renders the same statement. Attribution held only in the manifest travels with a bundle, not with a file fetched alone.

Further conditions, none of which the pipeline currently answers to:

- The acknowledgement requirement passes to recipients, and no additional or different conditions may be imposed on the licensed part. That constrains whatever terms the artifact tree states over itself.
- The data must not be used in a way suggesting official status or the licensor's endorsement. The lineage documentation makes the same point about the boundaries themselves being cartographic descriptions rather than endorsements, which a choropleth over a disputed boundary should surface rather than imply away.
- The name is not licensed as a trademark. Attribution use only.
- Breach terminates the licence automatically, without notice, which makes a defect in any of the above a licence problem rather than a presentation one.
- The data must not be altered so that what it contains becomes erroneous or misleading. §Reconciling discards remainder rings below an area threshold, so that threshold is the concrete act answering to this clause and its reasoning is recorded with it rather than assumed.

Citing the licence needs care, because the live text is client-rendered by a single-page application and carries no version marker. EuroGeographics' own published deliverable reproduces it as an annex and is the fixed, hashable copy worth citing, with the retrieval date recorded; the EU data portal identifies the edition's licence as the 2024 version, so versions do exist even though the page shows none. Two of the distribution's four documents must not be committed: the user guide forbids reproduction without written permission, and the data specification is marked as restricted to the association's members despite being served publicly. Cite their URLs and quote operative clauses only.

Two questions stay unanswered from public material: whether the collective credit discharges the individual national authorities or whether the pointer to their list must be reproduced, and whether the notice may live on a linked page rather than beside the file. The shape above discharges both conservatively without waiting on an answer, and the licensor's contact form settles them if a definitive one is wanted.

## Order of work

Subnational values do not depend on subnational geometry, so the geometry work is last rather than first:

1. Eurostat country-level values, and the scalar statistics no source we hold offers.
2. The subnational region model and its values, at every NUTS level.
3. This document's geometry: the diverged bundle variants, the subtraction, the level a feature carries and the control that selects it, and the legend attribution.

Steps 1 and 2 have landed. Step 3's prerequisite is the emphasis-state container, since a layer holding every level exceeds what the current uniform array can index.
