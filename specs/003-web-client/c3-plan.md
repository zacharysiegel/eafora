# Phase C3 — map camera, interaction, and the selection/hover render pass

- Feature: `003-web-client`, Phase C3 (follows C2, which closed with C2.7 + C2.3).
- Affected crates: `shared` (map: projection, viewport, renderer, pipelines, WGSL), `web` (driver, canvas).

## Scope and decomposition

C3 completes the map's interaction model. It is four sub-slices, built in order, each its own PR (and, for the render pass and camera work, its own detailed design section added here as it is picked up):

- **C3.1 — viewport aspect correction.** Fit the world to the canvas without stretching; home view centered on Washington DC. Detailed below.
- **C3.2 — selection/hover render pass.** On-map feedback: a 2px black outline on the selected country and a slight scale-up on both hovered and selected countries; hit-testing stays on the unscaled polygon.
- **C3.3 — manual pan/zoom.** Wheel-zoom + drag-pan the camera, in projected space, clamped to world bounds. Event-driven, so it rides the existing on-demand redraw (no animation loop).
- **C3.4 — animated zoom-to-country.** A `Camera` state machine (cubic easing, centroid target, bounding-box + margin scale) advanced by a self-scheduling `requestAnimationFrame` loop; click-to-zoom, coexisting with the C3.2 selection.

### Scope decisions (owner)

- **Manual and animated pan/zoom are in v1 / C3.** This overrides `specs/006-core-renderer/spec.md` §Out-of-scope, which deferred the animated zoom-to-country `Camera` to v1.5+, and relaxes `docs/design/README.md` §Animation ("no animations in v1") for the zoom-to-country transition specifically. Those two docs are updated in the C3.3 / C3.4 work.
- **Inertial (momentum) pan is deferred** to the backlog — it needs a time-driven decay loop, while direct pan/zoom does not.
- **Selection renders as a 2px black outline, not the 1px red** of FR-017 and `docs/design/README.md` §Map — a red outline is invisible on the red choropleth fills. FR-017 and the design README are updated in C3.2.

## C3.1 — viewport aspect correction

### Problem

The renderer maps the viewport's projected x-span to the full surface width and its y-span to the full surface height, independently per axis (`map.wgsl` `project_to_clip`). Two consequences:

1. **The map stretches.** `world_viewport()` fixes the viewport to the whole world regardless of the canvas shape, so the world is squashed to whatever aspect the surface has. The map is undistorted only when the viewport's projected aspect equals the surface's `W/H`, which nothing enforces.
2. **The projected plane is anisotropic.** `project` converts latitude to radians for `y` but passes longitude through in degrees for `x` (`projection.rs`), so "the map's aspect" is not `x-span / y-span` (mixed units), and FR-010's rationale — a constant projected increment moves the view a constant screen distance — does not hold. That uniformity is exactly what C3.3 pan/zoom needs.

### Design

**Isotropic projected space.** Reconcile units at the projection boundary so the projected plane is a proper isotropic space; geographic (data) coordinates stay in degrees.

- `projection.rs`: `project` returns `x = lon.to_radians()`; `unproject` returns `lon = x.to_degrees()`. Latitude already does this for `y`; longitude joins it. Update the two projection tests that assert `x == lon` in degrees.
- `map.wgsl`: the wraparound constants become radians — `±180.0 → ±π`, `360.0 → 2π` (the seam test and the whole-turn shift).
- Everything downstream inherits radian projected coordinates because it flows through `project` / `unproject`: the mesh vertices in `country_mesh`, the world viewport in the driver, and hit-test's `unproject`. `hit_test::wrap_longitude` stays in degrees — it wraps the geographic longitude *after* `unproject`, in the degree domain.

Result: geographic domain (geometry, `GeoPoint`, `WORLD_BOUNDS`, hit-test's point-in-polygon and `wrap_longitude`) is degrees; projected domain (`project` output, mesh, `Viewport`, shader, and later pan/zoom) is isotropic radians; one conversion at the boundary.

**Vertical-fill viewport.** A pure `shared::map` function, `Viewport::fill_height`, produces a `Viewport` whose aspect equals the surface aspect and whose vertical extent is exactly the requested half-height, filling the surface top to bottom; the horizontal extent is that half-height times the surface aspect (isotropic, never stretched). On a surface narrower than the content, the horizontal sides fall outside the viewport and are reached by panning (C3.3); it never leaves paper margins on the filled (vertical) axis.

**Home view.** The home viewport fills the surface vertically with a **fixed latitude band** (`HOME_VIEW_MIN_LAT`..`HOME_VIEW_MAX_LAT`, −56°..84°, chosen to enclose the drawn continents from Tierra del Fuego to northern Greenland), **horizontally centered on Washington DC's longitude** (≈ 77.04° W; the implementation defines a `HOME_CENTER` constant at ≈ 38.91° N, 77.04° W). Vertically it is centered on that band's midpoint (≈ 14°N, since the band skews north), not the equator, so no empty ocean shows below the southernmost land. Longitude runs at the same isotropic scale, so the surface width shows as much as fits; the remaining longitude is reached by horizontal pan (C3.3), with the wraparound placing DC at the middle. A surface wider than the band's aspect would repeat the world horizontally; that case is not special-cased for C3.1.

**Resize.** On resize the surface aspect changes, so the driver recomputes the viewport with `fill_height` around the same center and latitude band. The driver calls it at startup and in its resize handler.

**Where it lives.** The `fill_height` function is a pure `shared::map` helper (no `web_sys`, host-testable). The driver owns the home-view construction (calls `fill_height` with the framing constants + `HOME_CENTER`) at attach and on resize.

> **Deviation (post-review).** The design originally fit the whole world into the surface aspect by *containing* it (expanding the surplus axis). Visual review showed that produced horizontal tiling on a surface wider than the map and vertical paper margins on a narrower one, so it was replaced with fill-vertically (`Viewport::fill_height`) and the `Viewport::fit` contain helper was removed (a contain-style variant returns with C3.4). A second visual pass showed the ±85° framing left a large empty band at the bottom — the southern ocean below ~56°S, with no Antarctica in the layer. The home view was briefly reframed to the geometry's own bounding box, but that couples the composition to the data (adding polar features would silently reframe it), so it was settled on the deliberate `HOME_VIEW_MIN_LAT`/`HOME_VIEW_MAX_LAT` constants above; the `WORLD_BOUNDS` constant was removed.

### Testing

- `projection.rs`: existing round-trip test still holds (it inverts `project`/`unproject`); update the two tests that assert the raw degree `x`. Add a case pinning `project`'s `x` to `lon.to_radians()`.
- `fill_height` helper: host tests over square, wide, and tall surface dimensions — the returned viewport pins the vertical half-extent, has the surface aspect, and is centered on the requested center.
- The renderer's CPU-side crossing test `is_antimeridian_wrap` and the shader's `wrap_direction` are the two halves of one seam test; both move to `±π`, and `is_antimeridian_wrap`'s unit test moves to radian-scale viewports.
- `country_mesh`'s triangulation test derives the rectangle's projected x-width from `project` rather than a degree literal.
- Run `cargo test -p shared --features render` (not just the default features) so the renderer and `country_mesh` (render-gated) tests are exercised — the projection unit change ripples into both.

### Out of scope for C3.1

Pan/zoom (C3.3), the selection/hover render pass (C3.2), any animation (C3.4). C3.1 ships a fixed, aspect-correct, DC-centered whole-world view.

### PR description (draft)

**shared** — Make the projected plane isotropic: `project`/`unproject` carry longitude in radians (geographic coordinates stay degrees), and the WGSL wraparound constants become `±π` / `2π`. Add `Viewport::fill_height`, which frames a vertical extent into a surface-aspect viewport that fills the surface vertically.

**web** — The map viewport now fills the canvas vertically with a fixed latitude band (−56°..84°, chosen to enclose the drawn content, so no empty ocean shows below the southernmost land) and is recomputed on resize; the home view centers horizontally on Washington DC's longitude, with longitude beyond the canvas width reached by panning.

## C3.2 — selection/hover render pass

- Affected crates: `shared` (map: `renderer`, `country_mesh`, `gpu_types`, `map.wgsl`); `web` unchanged (the driver already tracks `selected_region` / `hovered_region`).

### Problem

The GPU has no per-country identity: `positions`, `fill` indices, and `border` indices are each one concatenated buffer, drawn in a single `draw_indexed`. Country identity lives only CPU-side in `renderer`'s `spans` (keyed by iso3). Both C3.2 effects need the vertex shader to know, per vertex, which country it belongs to and that country's centroid and highlight state:

- a discrete lift (scale-up) on the hovered and selected countries, around each country's own centroid;
- a 2px black outline on the selected country.

Hit-testing must keep reading the unscaled source polygon — it already runs on a separate CPU path in `hit_test` — so a country lifting under the cursor never changes which region is hit.

### Design

**Per-country identity on the GPU (FR-017's uniform-indexed-by-country).**

- Bake a per-vertex `country_index: u32` at mesh build, assigned in country build order, as a new vertex attribute in its own buffer (leaving the static `positions` buffer untouched). Compute each country's **centroid** at build time as the mean of its vertices — a pivot for the lift, not a label point, so the arithmetic mean is sufficient.
- A per-country **state uniform buffer**: `array<CountryState, CAP>` indexed by `country_index`, each entry `{ centroid: vec2<f32>, lift_px: f32 }` padded to 16 bytes, rewritten each frame from `frame_state.selected_region` / `hovered_region` (`lift_px` is the lift amount if hovered or selected, else 0). It is a uniform buffer, not storage, because the renderer supports a WebGL2 backend (`ForceGl`) which has no storage buffers; `CAP` is a fixed cap ≥ the loaded country count. Roughly 300 × 16 B is a few KB, well within uniform limits.
- `renderer`'s `CountrySpan` gains `region_code` (it currently carries only iso3) so the per-frame update can match `frame_state`'s `RegionCode`.

**Additive, screen-space offset (shared by the lift and the outline).**

A vertex is pushed outward from its country's centroid by a constant *screen-space* amount, so the effect is the same width on every country at every zoom (fixing the "Russia's rim is fat, Luxembourg's is a sliver" problem a multiplicative scale would cause):

```
let d = pos - centroid;
pos_out = pos + normalize(d) * k;   // guarded: if |d| < epsilon, no offset
```

`k` is the projected-space length of a target pixel width, computed per frame from the isotropic projected-units-per-pixel — uniform in every direction precisely because C3.1 made projected space isotropic: `k = target_px * (viewport_span_y / surface_height_px)`. The shader gets the surface height via the viewport uniform (extended) or a sibling uniform.

- **Lift.** Hovered or selected countries offset by `LIFT_PX`, others by 0, in **both** the fill and border vertex shaders so a country and its hairline border lift together.

**Selection outline (black silhouette via the same offset).**

The selected country's fill is redrawn over the base layer, offset outward an extra 2px in black, then its normal fill on top, leaving a 2px black rim that reads over neighbors:

1. Fills — all countries at their lift offset (base layer).
2. Selected outline — the selected country's fill offset by `LIFT_PX + 2px`, forced black, drawn over the base so the rim covers into neighboring fills.
3. Selected fill — the selected country's fill at `LIFT_PX`, its choropleth color, over (2), covering the interior and leaving the 2px rim.
4. Borders — all countries at their lift offset.

Steps 2–3 draw only the selected country, so `CountrySpan` also carries the country's **fill-index range** (`fill_index_start` / `fill_index_count`); the renderer issues `draw_indexed` over just that range when a region is selected. A small per-draw uniform supplies the extra offset (0 or 2px) and whether to force black; the centroid still comes from the per-country array by `country_index`.

The residual limitation of an additive-radial offset — the rim points away from the centroid, not perpendicular to each edge, so an archipelago whose centroid sits in the sea gets an uneven rim — is accepted for v1; a true perpendicular stroke is the fallback if it reads poorly.

**Phasing.** Two commits on the one branch: (1) per-country identity + state buffer + lift (fill and border); (2) the selection outline (fill-index ranges + the three-step draw order).

### Testing

- `country_mesh`: `country_index` assigned per country in build order; centroid is the vertex mean — host test over a two-feature layer asserting the indices and centroids, and that `CountrySpan`'s fill-index ranges tile the fill buffer with no gaps or overlap.
- The screen-space `k` (target_px + viewport span + surface height → projected `k`) is a pure function — host test.
- `renderer` (feature `render`): the per-country state buffer is rewritten from `frame_state` so the selected/hovered `RegionCode` lands on the correct `country_index`'s `lift_px`.
- WGSL is runtime-compiled, so the shader offset and the outline draw order are validated in the browser; note this in the PR.
- Run `cargo test -p shared --features render`.

### Out of scope for C3.2

Pan/zoom (C3.3), animation (C3.4). The lift is discrete and instant; hit-testing is unchanged.

### PR description (draft)

**shared** — Give the GPU per-country identity: a per-vertex country index, per-country centroids, and a per-country state uniform buffer indexed by that index (uniform, not storage, for WebGL2). The fill and border vertex shaders push a country's vertices outward from its centroid by a constant screen-space amount, lifting the hovered and selected countries. The selected country gets a 2px black outline, drawn as a black silhouette offset an extra 2px with its normal fill on top. Hit-testing is unchanged.

**web** — No change; the driver already tracks the selected and hovered regions.

## C3.3 — manual pan/zoom

Deferred detail; scope fixed: wheel-zoom and drag-pan mutating the `Viewport` in projected space, clamped to world bounds, reusing C2's pointer listeners and the on-demand redraw. Detailed when picked up.

- **Reimplement `Viewport::fill_height` on top of the zoom primitive.** Once a zoom function exists (set the viewport to a scale/zoom level around a center, with clamping), "fill the surface height with `min_y..max_y`" is just "zoom to the level at which that vertical extent spans the surface height, centered on `center_x`." `fill_height` should compute that zoom level and delegate, rather than construct the viewport directly as it does now, so the home view and manual zoom share one construction-and-clamp path.
- The zoom-out clamp is the home framing (`HOME_VIEW_MIN_LAT`..`HOME_VIEW_MAX_LAT`): the home view is the maximum zoom-out, so a user can zoom in and pan but not zoom out past the opening view.

## C3.4 — animated zoom-to-country

Deferred detail; scope fixed: the `Camera` state machine per `docs/architecture/overview.md` §Zoom-to-country, advanced by a self-scheduling `requestAnimationFrame` loop, framing the clicked country's bounding box (adding a contain-style fit alongside `fill_height` when it is picked up). Updates `006-core-renderer/spec.md` and the design README to move zoom-to-country into v1. Detailed when picked up.
