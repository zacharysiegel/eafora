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

- Affected crates: `shared` (map: `renderer`, `country_mesh`, `gpu_types`, `pipeline`, `map.wgsl`); `web` unchanged for rendering (the driver already tracks `selected_region` / `hovered_region`, and now requests a redraw when either changes).

### Problem

The GPU has no per-country identity: `positions`, `fill` indices, and `border` indices are each one concatenated buffer, drawn in a single `draw_indexed`. Country identity lives only CPU-side in `renderer`'s `spans`. The render pass needs the vertex shader to know, per vertex, which country it belongs to, that country's highlight state, and which way to push each boundary vertex, to produce:

- a discrete raise on the **hovered** country;
- a black outline on the hovered country (thin) and the selected country (bold).

Hit-testing must keep reading the unscaled source polygon — it runs on a separate CPU path in `hit_test` — so a country raising under the cursor never changes which region is hit.

### Design

**Per-country identity and boundary normals (FR-017's uniform-indexed-by-country).**

- Bake two per-vertex attributes at mesh build, in a buffer alongside the static `positions`: `country_index: u32` (assigned in country build order) and `outward_normal: vec2<f32>` — the unit miter of the vertex's two adjacent edges, oriented from the ring's signed-area winding so it points *out* of the solid area, and *into* hole rings. The miter is unit length (no `1/cos` scaling), so a sharp cape rounds slightly rather than shooting a spike; no clamping needed.
- A per-country **state uniform buffer**, `array<CountryState, COUNTRY_STATE_CAP>` indexed by `country_index`, each entry `{ lift_px: f32, outline_px: f32 }` padded to 16 bytes, rewritten each frame. Uniform, not storage, because the renderer supports a WebGL2 backend (`ForceGl`) which has no storage buffers. `COUNTRY_STATE_CAP = 512` (≥ the loaded country count; the upload errors if exceeded). The WGSL struct is padded to a full 16-byte stride: a bare-`f32` element is under-sized for a uniform array and reads misaligned on stricter WGSL validators (WebKit), rendering nothing highlighted.
- `CountrySpan` gains `region_code` (to match `frame_state`'s `RegionCode` when writing state) and the country's **fill-index range** (to redraw just that country on top).

**Perpendicular, screen-space offset.**

A vertex is pushed outward along its boundary normal by a constant *screen-space* amount, so the width is uniform on every country, at every zoom, and on every shape — compact, elongated, or multi-island — because each edge moves perpendicular to *itself*, not away from a shared centroid (an archipelago's islands each inflate around their own coastline instead of fleeing a centroid out in the sea):

```
pos_out = pos + outward_normal * ((lift_px + extra_px) * projected_per_pixel)
```

`projected_per_pixel = viewport_span_y / surface_height_px` — uniform in every direction precisely because C3.1 made projected space isotropic. `surface_size` is carried in the viewport uniform. `extra_px` is 0 in the fill and border vertex shaders and the country's `outline_px` in the outline vertex shader.

**Hover and selection state (written per frame).**

- Selected region → `outline_px = SELECTED_OUTLINE_PX` (6px), no lift.
- Hovered region → `lift_px = HOVER_LIFT_PX` (4px) and `outline_px = max(outline_px, HOVER_OUTLINE_PX)` (2px).

So selection is a bold outline with no raise (its persistent on-map marker, alongside the detail panel and the coming zoom-to-country); hover raises the country with a thin outline; a country that is both raises and keeps the bold outline.

**Outline as an inflated black silhouette.**

A third triangle pipeline (`outline`) shares the fill pipeline's vertex layout; its vertex shader inflates each vertex by `outline_px` beyond the lift, and its fragment shader is a constant black. It is a filled silhouette, not stroked line segments, so the rim is uniform even on multi-island countries, with no missing-border artifact.

Draw order in the map pass:

1. Base fills — all countries (each raised by its `lift_px`).
2. Base borders — all countries (each raised by its `lift_px`).
3. For each emphasized country (selected first, hovered last, deduplicated when the same country): its black silhouette (inflated by `outline_px`), then its fill (at `lift_px`) on top.

Redrawing the emphasized country on top places it above its neighbors; its fill covers the silhouette's interior — leaving only the outline rim — and covers its own hairline border (replaced by the rim). The renderer draws only that country's fill-index range for step 3.

### Testing

- `country_mesh`: `country_index` per country in build order; `outward_normal` is the winding-correct outward miter — host tests over hand-built CCW and CW rings (winding-agnostic), a hole ring (flips inward), and the Testland mesh (each normal points away from the interior).
- `renderer` (feature `render`): `viewport.to_gpu` carries the projected bounds and the surface size.
- WGSL is compiled at runtime, so the shader offset, the outline pipeline, and the draw order are validated in the browser (confirmed in Chrome and Safari).
- Run `cargo test -p shared --features render` (the renderer and `country_mesh` are `render`-gated).

### Out of scope for C3.2

Pan/zoom (C3.3), animation (C3.4). The raise and outline are discrete and instant; hit-testing is unchanged.

> **Deviation from the pre-implementation design.** The written plan had "a discrete lift on hovered and selected, and a 2px outline on selected," via a per-draw uniform toggling black and the offset. In building it: (a) a constant-pixel offset is a good uniform *outline* but a poor *lift* — invisible on a large country, self-intersecting on a small one — so the emphasis is carried by drawing the country on top plus the outline, with only a subtle lift; (b) selection no longer lifts (redundant with the panel and zoom-to-country) and instead gets a bolder outline; (c) the outline is a dedicated black-silhouette *pipeline* driven by a per-country `outline_px`, not a per-draw uniform; (d) the WGSL `CountryState` needed explicit 16-byte padding for WebKit. Tunable constants: `HOVER_LIFT_PX = 4.0`, `HOVER_OUTLINE_PX = 2.0`, `SELECTED_OUTLINE_PX = 6.0`.

### PR description (draft)

**shared** — Add the hover/selection render pass. Each vertex gains an outward boundary normal and a country index; a per-country `CountryState` uniform carries a lift and an outline width, rewritten each frame from the hovered and selected regions. The vertex shader offsets a vertex outward along its normal by a constant screen-space amount (via the isotropic projected-units-per-pixel). Hover raises a country with a thin black outline; selection gives a bolder outline and no raise. The outline is an inflated black fill silhouette drawn behind the normal fill, so only a uniform rim shows (clean on multi-island countries); the hovered and selected countries are redrawn on top so they are not overdrawn by neighbors. Per-vertex boundary normals are computed in the country mesh (a winding-agnostic unit miter; hole rings inward). Hit-testing is unchanged.

**web** — No change; the driver already tracked the hovered and selected regions, and now requests a redraw when either changes.

## C3.3 — manual pan/zoom

Deferred detail; scope fixed: wheel-zoom and drag-pan mutating the `Viewport` in projected space, clamped to world bounds, reusing C2's pointer listeners and the on-demand redraw. Detailed when picked up.

- **Reimplement `Viewport::fill_height` on top of the zoom primitive.** Once a zoom function exists (set the viewport to a scale/zoom level around a center, with clamping), "fill the surface height with `min_y..max_y`" is just "zoom to the level at which that vertical extent spans the surface height, centered on `center_x`." `fill_height` should compute that zoom level and delegate, rather than construct the viewport directly as it does now, so the home view and manual zoom share one construction-and-clamp path.
- The zoom-out clamp is the home framing (`HOME_VIEW_MIN_LAT`..`HOME_VIEW_MAX_LAT`): the home view is the maximum zoom-out, so a user can zoom in and pan but not zoom out past the opening view.

## C3.4 — animated zoom-to-country

Deferred detail; scope fixed: the `Camera` state machine per `docs/architecture/overview.md` §Zoom-to-country, advanced by a self-scheduling `requestAnimationFrame` loop, framing the clicked country's bounding box (adding a contain-style fit alongside `fill_height` when it is picked up). Updates `006-core-renderer/spec.md` and the design README to move zoom-to-country into v1. Detailed when picked up.
