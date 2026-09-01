# Geometry layers

How boundary geometry beyond country outlines reaches a client. Country geometry itself is covered by `ingestion.md` §Geometry ingestion; this document covers what happens once a second boundary source arrives, which Eurostat's NUTS regions are the first case of and US states and Canadian provinces the next.

## Two bundles, one swap

The producer already emits two bundle variants per build, `complete` for the CDN and `downsampled` for the embedded first-paint tree. Today both carry the same geometry file. They diverge instead:

- `downsampled` carries countries only, at Natural Earth's 1:50m, as the first-paint starter. It stays small because the embedded bundle answers to a 2 MB cap.
- `complete` carries the whole layer: subnational regions where a source supplies them, countries elsewhere, all reconciled into one file.

A client renders the embedded starter, then upgrades to the complete layer when the live bundle arrives — the path every visitor already takes once per load. `Renderer::refresh_country_geometry` rebuilds the vertex buffers when the incoming bundle names a geometry file it was not built from, so the upgrade reaches the map's shapes and not only its colours.

Whole-layer replacement rather than splicing a second layer into the loaded one. The country layer it replaces is approx. 500 KB compressed, discarded once per published version rather than once per visit, since artifacts are content-addressed and the cache keys on the hash. The consequence to watch is that the complete layer grows monotonically as sources are added, and every reader downloads all of it regardless of where they look. Second paint has an 8 MB target against 1.19 MB used as of the compression work, so the headroom is wide; when it is not, the escape hatch is per-layer artifacts fetched on a zoom trigger, which is a change to what the manifest lists and to how the renderer keys its buffers, not to anything below them.

## No generalisation

Subnational geometry ships at whatever resolution its source publishes. EuroGlobalMap's NUTS regions are drawn at 1:1M against Natural Earth's 1:50M for countries, and mixing the two is not a defect: finer detail below a pixel is invisible rather than wrong, it pays off when zoomed in, and a finer coastline has nothing to disagree with because the sea is background.

Simplifying is what would create defects. Douglas-Peucker applied to each polygon independently pulls shared borders apart, because a border simplified twice in two contexts no longer agrees with itself, and the result is slivers and gaps along every internal boundary. Doing it safely means simplifying the boundary network rather than the polygons: extract each shared border once, simplify it once, rebuild polygons from the shared arcs, which is what TopoJSON encodes. That is the tool to reach for if a measured byte count ever demands generalisation. Until a number demands it, geometry is emitted as published.

## Reconciling two sources into one layer

Two boundary sources drawn at different scales disagree about where a shared border runs. Where the fine source's coverage ends, its border and the coarse source's border do not coincide, so drawing both leaves overlapping or bare slivers, and the pass has no depth test to arbitrate per pixel.

Matching the two datasets vertex by vertex is the expensive answer and fails silently: deciding which runs of vertices describe the same border needs a tolerance, and independently generalised renderings of a river border differ by kilometres. Subtraction is exact instead, and runs in the producer:

1. Union every fine polygon into one shape, the footprint of everything the fine source covers.
2. Emit every fine polygon unchanged, each keyed to its own region code.
3. Subtract the footprint from each coarse country polygon and emit whatever remains, keyed to that country's region code. A country wholly inside the footprint leaves nothing and disappears; one partly overlapped keeps the part outside; one the footprint never touches is unchanged.

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
- **Not GISCO.** Eurostat's own NUTS boundaries are the better fit — pre-generalised at five scales, per level, a few megabytes — but they are EuroGeographics material carved out of Eurostat's reuse policy, and their terms require that "the data will not be used for commercial purposes". EuroBoundaryMap licensing starts at €6,600 for European coverage at the smallest user band.

The EuroGeographics Open Data Licence that governs EuroGlobalMap defines use as "any act for any legal purpose, including commercial exploitation", conditional on an attribution statement. Its short form is "© EuroGeographics 2026." and it must appear "within every use of the Dataset", so the map legend carries it. Attribution is per source, so the geometry artifact carries it as data rather than the client carrying a constant.

Open at the time of writing, and answerable only from the data rather than the specification: `NUTS_3` is optional per country in EuroGlobalMap, so coverage may be partial; and which NUTS revision its codes follow is not stated, which matters because codes are reused across revisions for different territory.

## Order of work

Subnational values do not depend on subnational geometry, so the geometry work is last rather than first:

1. Eurostat country-level values, and the scalar statistics no source we hold offers.
2. The subnational region model and NUTS-2 values.
3. This document's geometry: the diverged bundle variants, the subtraction, and the legend attribution.
