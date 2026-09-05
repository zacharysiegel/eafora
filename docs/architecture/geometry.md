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

The country remainder is shared across levels. Every level of one classification covers the same countries, so subtracting any level's footprint from the coarse source leaves the same remainder, and it is stored once and drawn at every level. The rule a client applies is therefore "features at the active level, plus the remainder", not level equality.

What this costs is duplicated coastline: a country covered at three levels stores its coast three times, since each level is a separate polygon set. Internal borders are not duplicated, only the outer ring. The lever if a measured byte count ever objects is the same topology encoding §No generalisation reaches for, which stores a shared border once.

What it requires first is that per-feature emphasis state stop living in a fixed uniform array. All levels resident at once is on the order of 2,200 features against a ceiling of 1,024, so the container has to move before anything renders; `docs/backlog.md` carries the replacement and the reason the per-region shape itself is kept.

Which levels a reader may choose comes from the manifest, being those with both geometry and values, rather than from a constant. A bundle carrying only countries and NUTS-2 offers exactly those two.

## No generalisation

Subnational geometry ships at whatever resolution its source publishes. EuroGlobalMap's NUTS regions are drawn at 1:1M against Natural Earth's 1:50M for countries, and mixing the two is not a defect: finer detail below a pixel is invisible rather than wrong, it pays off when zoomed in, and a finer coastline has nothing to disagree with because the sea is background.

Simplifying is what would create defects. Douglas-Peucker applied to each polygon independently pulls shared borders apart, because a border simplified twice in two contexts no longer agrees with itself, and the result is slivers and gaps along every internal boundary. Doing it safely means simplifying the boundary network rather than the polygons: extract each shared border once, simplify it once, rebuild polygons from the shared arcs, which is what TopoJSON encodes. That is the tool to reach for if a measured byte count ever demands generalisation. Until a number demands it, geometry is emitted as published.

## Reconciling two sources into one layer

Two boundary sources drawn at different scales disagree about where a shared border runs. Where the fine source's coverage ends, its border and the coarse source's border do not coincide, so drawing both leaves overlapping or bare slivers, and the pass has no depth test to arbitrate per pixel.

Matching the two datasets vertex by vertex is the expensive answer and fails silently: deciding which runs of vertices describe the same border needs a tolerance, and independently generalised renderings of a river border differ by kilometres. Subtraction is exact instead, and runs in the producer:

1. Union every fine polygon into one shape, the footprint of everything the fine source covers.
2. Emit every fine polygon unchanged, each keyed to its own region code.
3. Subtract the footprint from each coarse country polygon and emit whatever remains, keyed to that country's region code. A country wholly inside the footprint leaves nothing and disappears; one partly overlapped keeps the part outside; one the footprint never touches is unchanged.

Step 3 runs once for the whole classification rather than once per level, because every level covers the same countries and so shares one footprint. Step 1 can be taken at the finest level and reused: a coarser level is a union of finer polygons, so its footprint is identical by construction.

No overlap can survive, because every emitted coarse piece has had the fine area cut out of it, and no hole can open, because the only area removed is area a fine polygon now fills. Both hold from the operation rather than from a tolerance, and the covered countries need no separate exclusion list — subtraction removes them.

The two disagreement cases resolve on their own. Where the fine regions cross a coarse border, that strip leaves the coarse neighbour and draws as the fine region. Where they stop short of it, the strip survives the subtraction and draws as the country it belonged to.

`BooleanOps::difference` in geo carries this, alongside the `union` the writer already uses to fold territories into their sovereign. Three concerns attach to it, none algorithmic:

- Cost. Cutting every country against a footprint of over a thousand polygons is wasteful. Prefilter by bounding box so only countries touching the footprint are cut, and union the fine polygons per country first so each subtraction works against a small operand. The producer runs weekly, so its slowness costs nothing a reader sees.
- Robustness. Floating-point boolean operations leave hairline slivers along coincident edges and misbehave on degenerate rings, which real boundary data contains. Remainder rings below an area threshold are discarded, and the threshold is a named constant with its reason.
- Verification. The property wanted is that every point in the reconciled area falls inside exactly one feature, which a grid scan asserts directly — exactly one, so overlaps fail the test as well as holes. Conserved total area is the second assertion. This is the test that settles whether the library behaved, and it is worth more than reading its source.

## National outlines

A feature's outline is its polygon's edges: the renderer strokes the same rings it fills. So once a country's polygon has been subtracted away, nothing draws that country's border, and the national border appears only as the outer edge of its subnational regions, stroked at the same weight as the regional borders inside it. The country reads as a uniform mesh rather than as a country subdivided.

If national borders should read heavier than regional ones, the national outline has to be its own feature: union the subnational polygons per country and stroke that union more heavily. Same source, so the two agree vertex for vertex.

## Boundary sources

- **Countries: Natural Earth 1:50m.** Public domain, no obligations. Unchanged.
- **NUTS regions: EuroGlobalMap, via Open Maps for Europe.** Its `BND` coverage carries a `NUTS_3` feature class with `NUTS_CODE` (five characters, "as defined and published by Eurostat") and `NUTS_LABEL`. NUTS-2 and NUTS-1 are derived by unioning NUTS-3 on the code's four- and three-character prefixes, since NUTS codes nest by prefix.
- **Türkiye: Natural Earth admin-1.** Public domain, like the countries layer. Its 81 Turkish provinces match Eurostat's 81 Turkish NUTS-3 regions one to one with nothing left over on either side, which follows from Turkish NUTS-3 being defined on the provinces themselves; grouping them on the four-character prefix yields exactly the 26 NUTS-2 regions Eurostat publishes. The two sources name a province with unrelated identifiers, ISO 3166-2 and a NUTS code, and `subdivision` holds the pair so the join is seed data rather than a mapping the pipeline consults.
- **Montenegro: nothing needed.** Eurostat publishes it as a single region at every level, so the country outline already is its NUTS geometry.
- **Not GISCO.** Eurostat's own NUTS boundaries are the better fit — pre-generalised at five scales, per level, a few megabytes — but they are EuroGeographics material carved out of Eurostat's reuse policy, and their terms require that "the data will not be used for commercial purposes". EuroBoundaryMap licensing starts at €6,600 for European coverage at the smallest user band.

The EuroGeographics Open Data Licence that governs EuroGlobalMap defines use as "any act for any legal purpose, including commercial exploitation", conditional on an attribution statement. Its short form is "© EuroGeographics 2026." and it must appear "within every use of the Dataset", so the map legend carries it. Attribution is per source, so the geometry artifact carries it as data rather than the client carrying a constant.

Both questions the specification leaves open were settled from the data. `NUTS_3` is partial, as its optional status allows: it carries 1,437 distinct codes across 35 countries, Montenegro and Türkiye absent, which is what sends those two to the sources above. Its codes follow the current classification rather than a revision it names, measured as 1,420 of the 1,437 being present in the NUTS 2021 seed; the seeded codes it lacks are dominated by Türkiye's 81 and Montenegro's one. Rolling its codes up by prefix gives 113 NUTS-1 and 311 NUTS-2 groups against the 125 and 340 Eurostat publishes, the shortfall again being those two countries.

Acquisition is this source's one real obstacle, and it is not technical. The distribution is a 507 MB shapefile whose download link is minted server-side and sent by email after a registration form, and the site's client exposes no download route at all. So the file arrives by hand and is kept outside the repository, which departs from the pinned-release fetch every other source uses and means a rebuild depends on a local copy rather than on the network. The WFS the coverage probe went through is not an alternative: it serves `NUTS_3` alone, in GeoJSON alone of the formats it advertises, and requires a credential the publisher does not offer as a public API.

## Order of work

Subnational values do not depend on subnational geometry, so the geometry work is last rather than first:

1. Eurostat country-level values, and the scalar statistics no source we hold offers.
2. The subnational region model and its values, at every NUTS level.
3. This document's geometry: the diverged bundle variants, the subtraction, the level a feature carries and the control that selects it, and the legend attribution.

Steps 1 and 2 have landed. Step 3's prerequisite is the emphasis-state container, since a layer holding every level exceeds what the current uniform array can index.
