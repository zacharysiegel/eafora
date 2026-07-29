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

**Home view.** The home viewport fills the surface vertically with the whole world's latitude (`WORLD_BOUNDS` projected: ±85° latitude), **horizontally centered on Washington DC's longitude** (≈ 77.04° W; the implementation defines a `HOME_CENTER` constant at ≈ 38.91° N, 77.04° W). Longitude runs at the same isotropic scale, so the surface width shows as much of the world as fits; on a surface narrower than the map's ~1.53 aspect the remaining longitude is reached by horizontal pan (C3.3). Horizontal centering on DC falls out of the wraparound: the seam moves to DC's antipode, with DC at the horizontal middle and the world continuing across the edges. Vertically the view is world-centered on the equator. A surface *wider* than ~1.53 would repeat the world horizontally to fill the surplus; that case is not special-cased for C3.1.

**Resize.** On resize the surface aspect changes, so the driver recomputes the viewport with `fill_height` around the same center and latitude extent. The driver calls it at startup and in its resize handler.

**Where it lives.** The `fill_height` function is a pure `shared::map` helper (no `web_sys`, host-testable). The driver owns the home-view construction (calls the helper with `WORLD_BOUNDS`' latitude extent + `HOME_CENTER`) at attach and on resize.

> **Deviation (post-review).** The design originally fit the whole world into the surface aspect by *containing* it (expanding the surplus axis). Visual review showed that produced horizontal tiling on a surface wider than the map and vertical paper margins on a narrower one. It was replaced with fill-vertically (`Viewport::fill_height`): the full latitude always fills the surface height and longitude pans. The `Viewport::fit` contain helper was removed; a contain-style variant returns with C3.4 when zoom-to-country needs it.

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

**web** — The map viewport now fills the canvas vertically with the full latitude and is recomputed on resize; the home view centers horizontally on Washington DC's longitude, with longitude beyond the canvas width reached by panning.

## C3.2 — selection/hover render pass

Deferred detail; scope fixed: 2px black selection outline (line width isn't portable in WebGPU, so via stroked geometry or a black scaled-silhouette underlay — decided at design time), a slight discrete scale-up on hovered and selected countries applied in the vertex shader around each country's centroid, with per-country state fed to the GPU (per FR-017's uniform-indexed-by-country approach). Hit-testing continues to read the unscaled polygon. Detailed when picked up.

## C3.3 — manual pan/zoom

Deferred detail; scope fixed: wheel-zoom and drag-pan mutating the `Viewport` in projected space, clamped to world bounds, reusing C2's pointer listeners and the on-demand redraw. Detailed when picked up.

## C3.4 — animated zoom-to-country

Deferred detail; scope fixed: the `Camera` state machine per `docs/architecture/overview.md` §Zoom-to-country, advanced by a self-scheduling `requestAnimationFrame` loop, framing the clicked country's bounding box (adding a contain-style fit alongside `fill_height` when it is picked up). Updates `006-core-renderer/spec.md` and the design README to move zoom-to-country into v1. Detailed when picked up.
