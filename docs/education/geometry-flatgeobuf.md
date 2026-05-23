# FlatGeobuf

FlatGeobuf (`.fgb`) is a **binary, indexed, streaming-friendly geospatial vector format** built on top of Google's FlatBuffers serialization library. One file holds a feature collection (geometries + attributes) plus a packed spatial index, all laid out so a client can fetch only the bytes it needs over HTTP range requests. It was designed by Björn Harrtell as a more practical alternative to GeoJSON and Shapefile for serving static geospatial data from object storage.

This guide covers: what problem it solves → on-disk layout → the spatial index → how range-request reads work → comparison with the formats it competes with → when it's the right choice → Eafora-specific notes.

---

## 1. The problem it exists to solve

For static, server-hosted geospatial data, the older options each have a real flaw:

- **GeoJSON**: text. Parse cost scales with file size. No index — to find features in a bbox, you parse the whole thing. A planet-scale country dataset is 100 MB+ of JSON the client has to download and parse before it can render anything.
- **Shapefile**: 1990s-era binary, but multi-file (`.shp` + `.shx` + `.dbf` + `.prj` + ...), no streaming, attribute-name length limits, no UTF-8 by default, no proper spatial index in the standard. It's "supported everywhere" and "good in practice nowhere."
- **GeoPackage**: a SQLite database with geospatial tables. Powerful (full SQL queries, spatial index via R*-tree), but you typically download and open the whole file. Range-querying a GeoPackage over HTTP works in theory (SQLite-over-HTTP exists) but isn't the format's design center.
- **GeoJSON Feature Service / WFS**: requires a live server. Defeats CDN caching.
- **Vector tiles (Mapbox MVT / PMTiles)**: pre-tiled at fixed zoom levels. Great for slippy maps. Bad if you want **the actual geometry** — tiles are simplified per zoom, clipped to tile boundaries, and you can't reconstruct the unclipped feature without stitching.

FlatGeobuf is the answer for: "I want the actual full-resolution features, indexed, addressable by bbox, served from a CDN, no live database server."

---

## 2. On-disk layout

A `.fgb` file is laid out in three sections, in order:

```
┌───────────────────┐
│  Magic bytes      │  8 bytes: "fgb" + version
├───────────────────┤
│  Header           │  FlatBuffer-encoded: feature count, geometry type,
│                   │    CRS (typically EPSG code), columns schema,
│                   │    feature properties schema, index node size,
│                   │    flag for whether the index is present.
├───────────────────┤
│  Index            │  Packed Hilbert R-tree (optional but normal).
│                   │  Fixed node size (default 16). Each leaf entry:
│                   │    [bbox: 4×f64] + [feature offset: u64] + [feature size: u64]
├───────────────────┤
│  Features         │  Each feature is a FlatBuffer:
│                   │    geometry (point / line / polygon / multi*)
│                   │    + per-feature property values matching the schema.
│                   │  Features are stored in Hilbert order so spatially
│                   │    adjacent features are adjacent on disk.
└───────────────────┘
```

Two design choices matter:

**(a) FlatBuffers, not Protobuf.** FlatBuffers is a "zero-copy" serialization format: the on-disk bytes are the in-memory representation. You don't parse a feature; you cast a byte slice to a typed accessor and read fields directly. This is why decoding is fast — for a polygon with 50,000 vertices, the vertex array is just a `&[f64]` view over the file's bytes.

**(b) Packed Hilbert R-tree, not the SQLite-style R*-tree.** A packed tree is built once at write time (no runtime inserts/deletes), so it's denser and smaller than a dynamic R-tree. The Hilbert curve ordering means siblings in the tree tend to be spatially near each other, which keeps tree-walking range queries efficient.

The header is small (typically a few hundred bytes to a few KB depending on schema). The index for a million features at node size 16 is on the order of a few MB. The bulk is the features themselves.

---

## 3. The spatial index in detail

The index is a **packed R-tree**. Concretely:

1. **Sort features by Hilbert curve value of their bbox center.** This puts spatially nearby features in adjacent positions.
2. **Group every N features into a leaf node** (N = node size, default 16). Each leaf node stores the union bbox of its 16 features plus an offset table.
3. **Group every N leaf nodes into a parent node.** Same pattern, recursively, up to the root.
4. **Write the tree top-down**: root node, then its N children, then their children, etc. The leaf level comes last.

To find features in a bbox `Q`:

1. Read the root node (one range request, a few hundred bytes).
2. For each child whose bbox intersects `Q`, recurse into that child node (another range request).
3. At the leaf level, you get a list of `(feature_offset, feature_size)` pairs.
4. Issue range requests for each candidate feature's bytes. Decode and re-test against `Q` exactly (R-tree gives candidates; the bbox filter is approximate at any tree level above the leaves).

The clever bit: each tree level is contiguous on disk. So a real client doesn't issue one HTTP request per node — it batches by reading a **whole tree level** in one range, then walks it in memory.

The R-tree node size is tunable. Small N (2–8) means more levels, smaller individual nodes, more selective queries; large N (32+) means fewer levels, larger individual reads, fewer round trips. Default 16 is a reasonable balance.

---

## 4. Range-request reads — the cloud-optimized story

This is the design center. Hosted on a static CDN (Cloudflare R2, S3, etc.) that supports HTTP range requests:

1. Client issues `GET file.fgb` with `Range: bytes=0-8191` (read first ~8 KB).
2. Parses the magic + header. Now knows: total feature count, geometry type, schema, and the byte offset where the index starts.
3. Issues another range request for the first level of the index (or, often, for the whole index — a few MB is fine to fetch upfront).
4. Walks the index entirely in memory to find candidate features overlapping the query bbox.
5. Issues batched range requests for the candidate features' byte ranges.

For a typical query (bbox around one country in a planet-scale dataset), this means:
- Header: ~1 KB
- Index walk: ~100 KB to 5 MB (depending on dataset size)
- Features: only the ones you actually need, often <1 MB

Versus downloading a 200 MB GeoJSON. Two orders of magnitude less bandwidth, no parse cost on the unused features.

The format spec doesn't mandate that the client uses range requests — you can also stream the file from front to back (also a designed-for use case), since features are stored in Hilbert order and you can early-exit once you've walked past the bbox you care about.

---

## 5. Format quirks worth knowing

- **Single geometry type per file.** You declare "this file contains polygons" (or points or lines or one of the multi-variants) in the header. Mixing types means multiple files. Eafora's case (countries = polygons) is fine; if you wanted polygons + their centroids in one file, you'd need two files or use the `Unknown` geometry type which loses some validation.
- **Properties schema is fixed in the header.** Every feature has the same property columns. No per-feature schema variation. Saves space and decode time.
- **CRS is single-valued per file.** Almost always WGS84 / EPSG:4326 in practice. If you want a projected CRS, that's a separate file.
- **No edits.** Append-only would technically work but isn't a supported flow. The format is "build once, serve many."
- **No tiling.** This is the deliberate trade vs. PMTiles. A single `.fgb` file = whole dataset. You serve it from CDN. Range requests give you tile-like locality without the format being tile-based.
- **Tooling**: `ogr2ogr` (GDAL ≥ 3.1) reads and writes it directly. The `flatgeobuf` Rust, JS, Python, and C++ libraries all exist; the Rust one is reasonable. QGIS supports it as a native format.
- **Compression**: not built in. You can serve `.fgb` over HTTP with `Content-Encoding: gzip` or `br`, but range-request semantics with content-encoded responses are spotty (some CDNs serve identity-encoded bytes for ranges, some don't). Most pipelines leave the file uncompressed and rely on the format's already-tight FlatBuffer encoding.

---

## 6. Side-by-side with the other options

| Format         | Indexed?              | Range-request friendly? | Multi-file? | Tile-based? | Full geometry preserved? | Streaming? | When to use                                              |
| -------------- | --------------------- | ----------------------- | ----------- | ----------- | ------------------------ | ---------- | -------------------------------------------------------- |
| GeoJSON        | no                    | no                      | no          | no          | yes                      | no         | small dataset, debug, interop                            |
| Shapefile      | optional .qix         | no                      | yes (5+)    | no          | yes                      | no         | legacy interop                                           |
| GeoPackage     | yes (R*-tree, SQLite) | possible (sqlite-http)  | no          | no          | yes                      | partial    | desktop GIS workflows, complex queries                   |
| PMTiles        | yes (tile addressing) | yes (designed for it)   | no          | yes         | no (clipped/simplified)  | yes        | slippy-map base layer, web mapping                       |
| Mapbox MVT     | yes (per tile)        | depends on host         | yes (many)  | yes         | no                       | per tile   | tiled vector layer behind a tile server                  |
| **FlatGeobuf** | yes (packed R-tree)   | yes (designed for it)   | no          | no          | yes                      | yes        | full-fidelity features served from a static CDN          |

The cleanest mental rule: **PMTiles is for tiled rendering, FlatGeobuf is for tiled access to whole features**. They overlap in distribution model (single file, range-request, CDN-hosted) but answer different questions about geometry fidelity.

---

## 7. When it's the right choice

FlatGeobuf is a good fit when **all** of these are true:

- Your data is mostly static (write once, serve many).
- You want full-resolution geometry on the client (not pre-tiled, not pre-simplified).
- Clients query by bbox (or you want to read the whole dataset top-to-bottom and stop early).
- You can host a single binary blob on object storage / CDN.
- You don't need server-side query capabilities (joins, complex filters).

It's the wrong choice when:

- You need a slippy-map base map at varying zoom levels — use PMTiles.
- You need server-side geospatial joins or non-bbox queries — use a real spatial DB.
- Your dataset changes often — the rebuild cost adds up.
- Your features are tiny (millions of points with minimal geometry) — protobuf-based formats can pack tighter, and the per-feature FlatBuffer overhead becomes proportionally larger.

---

## 8. How it fits Eafora

Eafora's geometry artifact case:

- **Country polygons (Natural Earth Admin 0)**: ~250 features, vertex counts vary wildly (Russia's polygon is enormous, Vatican's is trivial). Total ~10–20 MB at full resolution. Static — Natural Earth ships annual releases. Queries are typically "give me everything visible in the current map viewport." This is the textbook FlatGeobuf case.

- **Subnational polygons (Admin 1, eventually Admin 2)**: tens of thousands of features. Total possibly 100+ MB at full resolution. Same query pattern. Range-request access pays off here especially — most viewports only need a few hundred features at a time.

- **Coastlines, lakes, rivers**: same story. One `.fgb` per layer.

The architecture overview's note about "ship full polygons, swap PMTiles → FlatGeobuf" is reasoning from exactly the trade above: at planet scale Eafora doesn't need tile-pyramid simplification because vector polygon counts are modest, and we *do* want full-fidelity geometry on the client so smoothed hover scaling and high-zoom rendering look right. PMTiles' clipped tile geometry would have made hover hit-testing on the unclipped country shape awkward.

The artifact-build pipeline becomes: ingestion job pulls Natural Earth → cleans/normalizes in PostgreSQL/PostGIS → exports per-layer `.fgb` files via `ogr2ogr` (or the Rust `flatgeobuf` crate) → uploads to Cloudflare R2 with content-hashed filenames → publishes a manifest mapping layer name → URL → hash. Clients fetch the manifest, then the relevant `.fgb`, then issue range requests via the `flatgeobuf` Rust crate (compiled into the WASM core, used via UniFFI on iOS/Android).

The Rust crate's API is roughly:

```rust
let reader = flatgeobuf::HttpFgbReader::open("https://cdn.eafora.org/geom/countries.fgb").await?;
let bbox: [f64; 4] = [west, south, east, north];
let features = reader.select_bbox(&bbox).await?;
while let Some(feature) = features.next().await? {
    let geometry = feature.geometry()?;
    let iso3: &str = feature.property("iso3")?;
    // ...
}
```

That's the full read pattern. The crate handles the range-request orchestration, parses the index, decodes the FlatBuffers as you iterate. No server-side code path needed at runtime — Cloudflare R2 serves the bytes.

---

## 9. What to read next

- The format spec lives at https://github.com/flatgeobuf/flatgeobuf — the README has a clear ASCII diagram of the layout, and the schema files (`feature.fbs`, `header.fbs`) are short enough to read in full to understand exactly what's in a feature.
- Björn Harrtell's blog posts on the design tradeoffs (search "FlatGeobuf cloud native") explain why the choices were made — particularly the Hilbert ordering and why he didn't go with a more traditional R*-tree.
- For the hands-on exercise: take a Natural Earth countries shapefile, run `ogr2ogr -f FlatGeobuf out.fgb in.shp`, then open the file in QGIS to confirm. Then write a tiny Rust program with the `flatgeobuf` crate that reads features in a small bbox and prints their ISO codes — that's enough to feel the API and confirm the range-request behavior on a CDN-hosted file.
