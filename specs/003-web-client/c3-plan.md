# Phase C3 — map camera, interaction, and the selection/hover render pass

- Feature: `003-web-client`, Phase C3 (follows C2, which closed with C2.7 + C2.3).
- Affected crates: `shared` (map: projection, viewport, renderer, pipelines, WGSL), `web` (driver, canvas).

## Scope and decomposition

C3 completes the map's interaction model. It is four sub-slices, built in order, each its own PR (and, for the render pass and camera work, its own detailed design section added here as it is picked up):

- **C3.1 — viewport aspect correction.** Fit the world to the canvas without stretching; home view centered on the prime meridian (Greenwich). Detailed below.
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

**Home view.** The home viewport fills the surface vertically with a **fixed latitude band** (`HOME_VIEW_MIN_LAT`..`HOME_VIEW_MAX_LAT`, −56°..84°, chosen to enclose the drawn continents from Tierra del Fuego to northern Greenland), **horizontally centered on the prime meridian** (Greenwich, longitude 0°; the implementation defines a `HOME_CENTER` constant at longitude 0°). Vertically it is centered on that band's midpoint (≈ 14°N, since the band skews north), not the equator, so no empty ocean shows below the southernmost land. Longitude runs at the same isotropic scale, so the surface width shows as much as fits; the remaining longitude is reached by horizontal pan (C3.3), with the wraparound placing the prime meridian at the middle. A surface wider than the band's aspect would repeat the world horizontally; that case is not special-cased for C3.1.

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

Pan/zoom (C3.3), the selection/hover render pass (C3.2), any animation (C3.4). C3.1 ships a fixed, aspect-correct, prime-meridian-centered whole-world view.

### PR description (draft)

**shared** — Make the projected plane isotropic: `project`/`unproject` carry longitude in radians (geographic coordinates stay degrees), and the WGSL wraparound constants become `±π` / `2π`. Add `Viewport::fill_height`, which frames a vertical extent into a surface-aspect viewport that fills the surface vertically.

**web** — The map viewport now fills the canvas vertically with a fixed latitude band (−56°..84°, chosen to enclose the drawn content, so no empty ocean shows below the southernmost land) and is recomputed on resize; the home view centers horizontally on the prime meridian (Greenwich), with longitude beyond the canvas width reached by panning.

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

## C3.3 — manual pan/zoom (wheel, drag, and pinch)

- Affected crates: `shared` (map: `viewport`, `hit_test`) and `web` (driver: `canvas/driver.rs`; one added `web-sys` feature). No renderer or shader change: the viewport already reaches both the GPU and the hit-test through the `Driver::viewport` field the driver mutates, and `write_viewport_uniform` re-reads it each frame.

### Problem

The viewport is fixed. `home_viewport` builds it at attach and rebuilds it on resize; nothing else mutates `Driver::viewport`. A user cannot zoom in on a region or pan to the longitude the home framing leaves off-screen. C3.3 adds cursor-anchored wheel-zoom, drag-pan, and two-finger pinch-zoom, each updating the projected-space viewport and then calling `request_redraw()` (the same on-demand redraw hover and selection already use — there is no animation loop; a redraw happens only in response to an input event).

### Invariants every viewport must satisfy

The GPU path assumes four things, so the zoom/pan primitives are defined to preserve all four:

1. `min.{x,y} < max.{x,y}`. `project_to_clip` divides by `span = max - min`.
2. The viewport aspect equals the surface aspect (`(max.x - min.x) / (max.y - min.y) == surface.width / surface.height`). `emphasis_offset` derives one isotropic projected-units-per-pixel from the y-span alone, trusting x to match; a broken match would distort the hover lift and outline in x versus y.
3. Width never exceeds `2π`. `wrap_direction` returns on the first crossed bound and assumes at most one antimeridian seam is crossed; a wider viewport crosses two and the shader shifts the wrong instance, blanking one side.
4. Surface dimensions are positive (already guaranteed upstream).

All pan/zoom math is pure `shared::map` on `Viewport` in isotropic projected radians, host-testable with no `web_sys`. The driver owns event wiring and converts a pointer position into a `SurfacePoint` and a scalar, then calls the pure primitives.

### The pure primitives (`shared/src/map/viewport.rs`)

Derived accessors `center()`, `half_height()`, `half_width()` (`(min + max) / 2`, `(max - min) / 2`), `pub(crate)`.

A single vertical-extent clamp holds both zoom limits in one place:

```rust
fn clamp_half_height(requested: f64, max_half_height: f64) -> f64 {
    debug_assert!(max_half_height >= MIN_ZOOM_IN_HALF_HEIGHT, "zoom-out ceiling below the zoom-in floor");
    requested.clamp(MIN_ZOOM_IN_HALF_HEIGHT, max_half_height)
}
```

- `MIN_ZOOM_IN_HALF_HEIGHT` is the zoom-in floor (smallest half-height; approx. one degree of latitude filling the screen, `project(0.5, 0.0).y ≈ 0.0087` rad; encode the computed literal). `max_half_height` is the zoom-out ceiling, passed in by the driver (the home framing, capped for wide surfaces; see below). Names state direction so the `clamp(floor, ceiling)` reads correctly.

- `zoom_to_half_height(&self, half_height, max_half_height, surface) -> Viewport` is the construction-and-clamp core everything routes through. It clamps the half-height, re-derives `half_width = clamped_half_height * (surface.width / surface.height)` (never reads the receiver's width, so a zero-width seed is fine), and rebuilds `min`/`max` around `self.center()`. This is the one place the vertical extent clamp is applied.

- `zoom_about(&self, factor, anchor, max_half_height, home_min_y, home_max_y, surface) -> Viewport` is the anchored zoom (`factor > 1` zooms in). It holds the projected `anchor` fixed on screen, with the vertical position clamp folded into the recenter so there is no post-step that could move the anchor:
  1. `target = self.half_height() / factor`; `clamped = clamp_half_height(target, max_half_height)`; `achieved = clamped / self.half_height()` (the achieved ratio, not `1/factor`, keeps the anchor fixed when clamped at the ceiling).
  2. `new_center = anchor + (self.center() - anchor) * achieved` (isotropic, so one scalar on both axes).
  3. Fold the vertical position clamp into the center's y only: `center_y = new_center.y.clamp(home_min_y + clamped, home_max_y - clamped)`. The home latitude range is an absolute bound; when a zoom-out near the top or bottom edge would push the view outside it, the view is shifted back inside and the anchor yields. Horizontal anchoring is exact for every zoom; vertical anchoring is exact except within one range-height of the top/bottom edge, where the anchor's screen-y drifts by the (bounded) clamp amount. The range invariant wins over the anchor. At full zoom-out the clamp interval collapses to the range midpoint (guard the collapse with a tolerance so full zoom-out sits exactly on the range with no jitter or `clamp` panic).
  4. Build the box from `new_center.x`, `center_y`, `clamped`, and the aspect-derived `half_width`.

- `pan_by(&self, dx, dy) -> Viewport` translates both corners; width/height/aspect unchanged; no clamp on x.
- `clamp_vertical(&self, home_min_y, home_max_y) -> Viewport` clamps only the vertical position (extent unchanged), sharing the step-3 rule via a common private helper. At full zoom-out (extent == range height) it centers on the range; zoomed in, the view slides freely inside it. The pan path calls this explicitly; the zoom path has it folded in.
- `normalize_longitude_turns(&self) -> Viewport` shifts the viewport by whole turns of `2π` so `center().x` lands in `[-π, π]`. This keeps the two-instance antimeridian model valid for arbitrarily long horizontal pans (the renderer draws only the natural copy plus one copy shifted by `±1` turn, covering `[-π, 3π]`; without re-normalization a long east/west pan slides past that union and the map blanks). A whole-turn shift changes the resolved longitude only by a multiple of 360°, which the hit-test's `wrap_longitude` (`rem_euclid(360)`) folds away, so hover/selection are unaffected.

### Horizontal behavior

Horizontal position is unclamped but re-normalized by whole turns after every pan/zoom (`normalize_longitude_turns`), so the world scrolls continuously and infinitely while the two-instance model stays valid. Every driver path applies the re-normalization adjacent to the viewport-x mutation.

### Zoom-out ceiling and the width ≤ 2π conflict (ultrawide surfaces)

The home latitude range half-height is approx. `1.549` rad, so a full-zoom-out width of `2 * 1.549 * aspect` exceeds `2π` once the surface aspect passes approx. `2.029:1`. Three goals — home range is the maximum zoom-out, aspect locked, width ≤ `2π` — are jointly unsatisfiable above that aspect. Resolution (owner-chosen option b): cap the zoom-out ceiling at `max_half_height = min(home_range_half_height, π / aspect)`. Aspect stays locked and the width never exceeds one world turn. The consequence on a surface wider than approx. `2.029:1` is that furthest zoom-out shows the full world width once, centered, and a vertical slice of the home range that the user pans through vertically — the visible latitude shrinks; there are no empty margins and the world is not repeated horizontally. The driver computes the cap (it owns the framing constants and the surface aspect) and passes the capped `max_half_height` into the primitives:

```rust
fn zoom_out_ceiling_half_height(surface: SurfaceDimensions) -> f64 {
    home_range_half_height().min(std::f64::consts::PI * (surface.height as f64 / surface.width as f64))
}
```

`home_viewport`/`fill_height` are routed through the same cap: this fixes a latent bug where the existing `fill_height` already produces a `> 2π` home view on such surfaces.

### `fill_height` reimplemented on the primitive

`Viewport::fill_height(center_x, min_y, max_y, surface)` is reframed as "zoom to the level at which `min_y..max_y` spans the surface height, centered on `center_x`," delegating to `zoom_to_half_height` (so the home view and manual zoom share one construction-and-clamp path) and capping the width at `2π`:

```rust
pub fn fill_height(center_x: f64, min_y: f64, max_y: f64, surface: SurfaceDimensions) -> Viewport {
    let requested_half_height: f64 = (max_y - min_y) / 2.0;
    let center_y: f64 = (min_y + max_y) / 2.0;
    let width_cap_half_height: f64 = std::f64::consts::PI * (surface.height as f64 / surface.width as f64);
    let ceiling: f64 = requested_half_height.min(width_cap_half_height);
    let seed: Viewport = Viewport { min: ProjectedPoint { x: center_x, y: center_y - requested_half_height }, max: ProjectedPoint { x: center_x, y: center_y + requested_half_height } };
    seed.zoom_to_half_height(requested_half_height, ceiling, surface)
}
```

On a normal (≤ approx. 2.029:1) surface `ceiling == requested_half_height`, so `fill_height` produces the identical viewport it does today (the existing host tests are the behavior-preserving guard); on an ultrawide surface the width is capped at `2π` and the view no longer fills height.

### Screen-to-projected helper (shared with the hit-test)

Extract `hit_test::surface_to_projected(viewport, surface_dimensions, surface_point) -> ProjectedPoint` — the body of `surface_to_geo` minus the final `unproject`. Reimplement `surface_to_geo` as `unproject(surface_to_projected(...))`, so the device-pixel → `[0,1]` → projected normalization (with the y-inversion) lives in one place and the gesture anchors and the hit-test can never disagree. The wheel/pinch/pan anchors are all computed with this helper against the current viewport, so the projected-space math the driver does is the exact inverse of what the hit-test does to resolve a pixel: a pixel held fixed by a gesture keeps resolving to the same country.

### Input model — Pointer Events

Migrate the driver's input from `MouseEvent` to the Pointer Events API for one model that also serves touch. C2's hover and click wiring move to pointer events too. Add `"PointerEvent"` to the `web-sys` features.

- Listeners installed like the existing ones: `pointerdown`, `pointermove`, `pointerup`, `pointercancel`, `pointerleave` (each a stored `Closure` held on `Driver` for lifetime). On `pointerdown` the driver calls `set_pointer_capture(pointer_id)` so a gesture keeps receiving moves after the pointer leaves the canvas; the matching `release_pointer_capture` runs on up/cancel.
- Hover (lift/outline) applies only to `pointer_type() == "mouse"` and only when no gesture is active; it is suppressed for the whole duration of any drag or pinch and restored afterward. `pointerleave` with a mouse pointer clears hover.
- Selection is resolved on `pointerup` for a single-pointer gesture that never moved past the drag threshold and never became a multi-pointer gesture — this replaces the `click` handler and removes the click-after-release ordering hazard entirely.

### Multi-pointer state machine (pan + pinch)

The driver tracks active pointers by `pointer_id` (position per id; at most the first two matter) and a gesture that is one of `Idle`, `Pan`, or `Pinch`, plus `gesture_moved: bool` (set once movement passes the threshold or a second pointer joins; reset only at the next `pointerdown` that starts from `Idle`).

- `pointerdown`:
  - From `Idle` → `Pan { anchor = this point }`, record the press origin for the tap-vs-drag test, `gesture_moved = false`, capture.
  - From `Pan` (a second pointer arrives) → `Pinch`; record the baseline as the two pointers' current positions (so the first pinch move computes a correct `now/previous` ratio, never a garbage first frame); set `gesture_moved = true` (a multi-pointer gesture never selects). Capture the second pointer. No viewport change on the transition itself.
- `pointermove` updates the stored position of that pointer, then:
  - `Pan` (one pointer): `from = surface_to_projected(viewport, anchor)`, `to = surface_to_projected(viewport, now)`, `viewport = viewport.pan_by(from.x - to.x, from.y - to.y).clamp_vertical(home_min_y, home_max_y).normalize_longitude_turns()`; advance `anchor = now`; set `gesture_moved` if the cursor is past the threshold from the press origin; skip the hover path; `request_redraw()`.
  - `Pinch` (two pointers): compute previous and current midpoints and distances in surface space; `factor = distance_now / distance_previous` (incremental). Anchor the zoom at the projected point currently under the previous midpoint, then translate so that point tracks the new midpoint:
    ```
    anchor = surface_to_projected(viewport, previous_midpoint)
    zoomed = viewport.zoom_about(factor, anchor, ceiling, home_min_y, home_max_y, surface)
    from = surface_to_projected(zoomed, previous_midpoint)   // == anchor (zoom_about holds it fixed)
    to   = surface_to_projected(zoomed, current_midpoint)
    viewport = zoomed.pan_by(from.x - to.x, from.y - to.y).clamp_vertical(home_min_y, home_max_y).normalize_longitude_turns()
    ```
    Then store the current pointer positions as the next step's "previous." This scales by the finger-distance ratio and pins the midpoint (a similarity transform without rotation — the two finger points stay pinned along the line between them; finger rotation is not corrected, which is the standard non-rotating pinch). Because each step re-seeds from the current positions and applies against the current viewport, there is no compounding drift and the translation is applied once. The same ceiling, floor, home-range clamp, and longitude re-normalization apply as for wheel zoom.
- `pointerup` / `pointercancel` / `pointerleave`: remove the pointer from the active set and release its capture.
  - `Pinch` → one pointer left: switch to `Pan` and re-seed `anchor` from the remaining pointer's current position, so the next pan move computes an incremental delta from where the finger actually is — no jump. `gesture_moved` stays `true`.
  - `Pan` → zero pointers: if this was a `pointerup`, not `pointercancel`, and `gesture_moved` is `false`, resolve selection at the release point; then go `Idle`.
  - `pointercancel` never selects.

### Driver wheel zoom

`handle_wheel(event)` calls `prevent_default()`, converts the cursor with the existing surface-point conversion (`WheelEvent` extends `MouseEvent`, so offset + DPR scaling apply), and:

```rust
let clamped_delta: f64 = event.delta_y().clamp(-MAX_WHEEL_DELTA, MAX_WHEEL_DELTA);
let factor: f64 = (-clamped_delta * WHEEL_ZOOM_SENSITIVITY).exp();   // scroll up (delta_y < 0) zooms in
let anchor: ProjectedPoint = hit_test::surface_to_projected(self.viewport, self.surface_dimensions, surface_point);
let zoomed: Viewport = self.viewport.zoom_about(factor, anchor, zoom_out_ceiling_half_height(self.surface_dimensions), home_min_y, home_max_y, self.surface_dimensions);
self.viewport = zoomed.normalize_longitude_turns();
self.request_redraw();
```

The exponential map makes zoom multiplicative and symmetric. `MAX_WHEEL_DELTA` caps one event's magnitude so a page-mode or high-resolution delta cannot zoom absurdly far in one notch (deltaMode varies by browser; a per-event cap is the desktop-first v1 guard, with deltaMode-aware scaling as a later option). `home_min_y`/`home_max_y` and `home_range_half_height` come from shared driver helpers that project `HOME_VIEW_MIN_LAT`/`HOME_VIEW_MAX_LAT` (reused by `home_viewport`, wheel, drag, and pinch rather than hand-rolled at each site).

### Flow to the GPU and hit-testing

`draw` passes `self.viewport` to `draw_frame`; `write_viewport_uniform` re-reads the current surface size each frame, so a mutated viewport reaches the shader on the next scheduled frame with no extra wiring, and the four invariants keep `project_to_clip`, `emphasis_offset`, and `wrap_direction` correct. Hover and selection read the live `self.viewport`, so they resolve against the mutated view. DPR is baked into the backing-store size and the `SurfacePoint` and normalized away in `surface_to_projected`; the viewport math is entirely in projected radians and needs no DPR handling.

### Constants (tunable in-browser)

`MIN_ZOOM_IN_HALF_HEIGHT` (shared), `WHEEL_ZOOM_SENSITIVITY`, `MAX_WHEEL_DELTA`, and `DRAG_SELECT_SUPPRESS_PX` (approx. 5 device pixels) are named constants; the design does not depend on their exact values.

### Testing

Pure `shared::map` host tests (no `web_sys`, no render feature — `viewport.rs`/`hit_test.rs` are not render-gated):

- `clamp_half_height`: clamps up to the floor, down to the ceiling, passes through in range; `#[should_panic]` (debug assertions) when the ceiling is below the floor.
- `zoom_to_half_height`: extent clamps both ways; aspect equals the surface aspect on square/wide/tall surfaces; center preserved for a pure zoom; width derived from aspect even from a zero-width seed.
- `zoom_about`: horizontal anchor exact for both the unclamped and vertically-clamped cases; both-axes anchor exact when the result does not touch the range edge; near the range edge, assert the actual behavior — screen-x unchanged, box pinned to `home_max_y`/`home_min_y`, and the anchor's screen-y drift equals the clamp amount; achieved-ratio path (zoom out past the ceiling about an off-center anchor keeps x fixed and the half-height at the ceiling).
- `pan_by`: pure translation; x may exceed `±π`.
- `clamp_vertical`: pulled inside the range from above/below; centered at full zoom-out (equal-height edge case with tolerance); untouched when inside; extent never changed.
- `normalize_longitude_turns`: a far-east/-west center is shifted into `[-π, π]`, width preserved, shift a whole multiple of `2π`; a fixed surface point unprojects to the same `wrap_longitude`-folded longitude before and after (the hit-test-invariance property).
- `fill_height`: existing tests pass verbatim; a delegation test pins `fill_height == seed.zoom_to_half_height(...)`; a 32:9 test asserts width ≤ `2π` (the ultrawide cap) with aspect still matching.
- `surface_to_projected`: `surface_to_geo == unproject(surface_to_projected(...))`; the surface center maps to the viewport center.
- Pinch math: extract the pure part (`factor`, `previous_midpoint`, `current_midpoint`, and the compose `zoom_about` → `pan_by`) into a `pub(crate)` helper on `Viewport` taking previous/current midpoints and distances, and host-test that both midpoints and the distance ratio are honored (the world point under the previous midpoint lands under the current midpoint; the half-height scales by the inverse distance ratio, subject to the clamps) and that a lone incremental step does not drift.

Driver wiring (the pointer state machine, capture, hover suppression, tap-vs-drag) is thin glue over the pure primitives and is verified manually in the browser (per the wasm-test convention): wheel zooms toward the cursor and stops at the home view; drag pans and the grabbed point tracks; a long east/west drag keeps the world visible and wrapping; vertical pan is bounded to the home range; a two-finger pinch zooms about the midpoint and does not select; lifting one finger continues the pan without a jump; a tap under the threshold selects, a drag over it does not. `cargo test -p shared` covers the pure tests (`viewport`/`hit_test` are not render-gated); `cargo check --target wasm32-unknown-unknown -p web` confirms the driver and the new `web-sys` feature compile.

### Out of scope for C3.3

Animated zoom-to-country and the `Camera` state machine (C3.4). Inertial/momentum pan (backlog; it needs a time-driven decay loop, which C3.3's event-driven redraw deliberately avoids). Two-finger rotation (pinch is scale + translate only). Keyboard/button zoom controls.

### PR description (draft)

**shared** — Add the pan/zoom primitives to `Viewport`: `zoom_to_half_height` (the construction-and-clamp core), `zoom_about` (anchored multiplicative zoom holding a projected point fixed, with the home-range vertical clamp folded into the recenter so horizontal anchoring stays exact and vertical yields only at the range edge), `pan_by`, `clamp_vertical` (keeps the view inside the home latitude range), and `normalize_longitude_turns` (re-centers x within one antimeridian turn so long horizontal pans never blank the map). The zoom-out ceiling is the home framing capped so the width never exceeds one world turn (a vertical slice on surfaces wider than approx. 2:1), and a zoom-in floor caps how far in a user can zoom. `Viewport::fill_height` is reimplemented on `zoom_to_half_height` and now caps its width at `2π`. Extract `hit_test::surface_to_projected` so the gesture anchors and the hit-test share one normalization.

**web** — Wheel-zoom toward the cursor, drag-pan, and two-finger pinch-zoom, mutating the projected-space viewport and redrawing on demand (no animation loop). Zoom-out stops at the home view; vertical pan is bounded to the home latitude range; horizontal pan wraps across the antimeridian and stays visible for any distance. Input moves to the Pointer Events API with pointer capture (one model for mouse and touch); hover is mouse-only and suppressed during a gesture; a drag or multi-pointer gesture past a small pixel threshold does not select. Adds the `PointerEvent` web-sys feature.

> **Deviation notes.** Implemented as designed, with these specifics. A review after implementation renamed the primitives from half-height to the full visible **height** (`half_height`→`height`, `max_half_height`→`max_height`, `zoom_to_half_height`→`zoom_to_height`, `zoom_out_ceiling_half_height`→`zoom_out_ceiling_height`), so the half-height pseudo-code above reads doubled in the shipped code: the zoom-in floor is `MIN_ZOOM_IN_HEIGHT = 0.01745343429705546` (the full one-degree band, `project(0.5, 0.0).y - project(-0.5, 0.0).y`), and the width cap is `2π/aspect` (was `π/aspect` in half-height terms). Other specifics: `WHEEL_ZOOM_SENSITIVITY = 0.0015`, `MAX_WHEEL_DELTA = 240.0`, and `DRAG_SELECT_SUPPRESS_PX = 7.0`. `surface_to_projected` landed in `hit_test.rs`, and the pinch, pan, and wheel math were extracted there too as pure host-tested helpers (`pan`, `zoom_at_surface_point`, `pinch`), so the driver is thin event plumbing. The `2π` width cap lives in `fill_height` itself (not a separate `home_viewport` path), so `home_viewport` gets the ultrawide fix for free; the existing wide-surface `fill_height` test moved from 4:1 to 2:1 (4:1 now trips the cap) and a 32:9 cap test was added. Input is the Pointer Events API with `Element::set_pointer_capture`; the gesture is a `Gesture` enum state machine (`Idle`/`Tap`/`Pan`/`Pinch`); selection resolves on `pointerup` for an unmoved `Tap`, replacing the `click` handler; `pointerleave` clears hover only; `touch-action: none` on the canvas lets touch drags reach the driver. The home view centers on the prime meridian (Greenwich), not Washington DC.

## C3.4 — animated zoom-to-country

> **Deviation (post-review renames).** The names below reflect the original design; a review pass renamed several. The `Camera` type shipped as `ViewportTransition` in `shared/src/map/viewport_transition.rs` (`Camera` is a poor name for a transient in-progress transition), the `Progress` enum as `AnimationProgress`, and the pure numeric helpers (`lerp`, `inverse_lerp`, the geometric blend renamed `geometric_lerp`→`geometric_interpolate`, and `cubic_ease_in_out`) live in `shared::math`. Read `Camera`→`ViewportTransition` and `shared::map::camera`→`shared::map::viewport_transition` throughout this section.

- Affected crates: `shared` (map: a new `camera` module, `viewport`, `hit_test`; geometry: a centroid helper in `artifact/geometry.rs`) and `web` (driver: `canvas/driver.rs`). No renderer, shader, `country_mesh`, or `gpu_types` change: `draw_frame(viewport, ..)` already takes the viewport per call and writes it to the uniform (`write_viewport_uniform`) and the renderer owns no camera state, so animation is just the driver supplying a different viewport on successive frames. The shader recomputes the antimeridian wrap direction from the drawn viewport each frame (`wrap_direction`), so every interpolated frame renders its own two-instance coverage correctly, as long as the interpolated width never exceeds one world turn (it cannot: height is bounded by the zoom-out ceiling, whose width is capped at `2π`).

### Problem

C3.3 gave manual pan/zoom but left no way to bring a chosen country into frame in one action. A tap selects the region and publishes the detail panel, but the viewport does not move; a small country stays small and off-center. C3.4 adds a zoom that frames the tapped country's bounding box with a margin, centered on the country's centroid, over a short cubic-eased transition. It fires on re-tapping the already-selected country (a second tap, or a double-click), not on a plain first selection, so exploration and comparison clicks never move the map unasked. This is the one animation permitted through v1 (`docs/design/README.md` §Animation); a jump-cut to a distant, rescaled viewport loses the eye's bearings on the globe in a way this transition prevents.

### Where the pieces live

Pure `shared::map` math (host-testable, no `web_sys`) plus driver wiring, mirroring C3.3:

- A `Camera` state machine in a new `shared/src/map/camera.rs` (`pub mod camera;` + `pub use camera::*;`, non-render-gated). It holds the from-viewport, target-viewport, start time, and duration, and exposes one function that samples the interpolated `Viewport` at a given time and reports whether the transition has finished. The architecture's aspirational `core::geometry::animation::Camera` maps to `shared::map::camera::Camera` here.
- `Viewport::fit_bounds`, the contain-style counterpart to `fill_height` (removed in C3.1, returns here): frames a projected rectangle within the surface aspect with a margin, clamped.
- `Viewport::interpolate_to`, the per-frame blend the `Camera` calls.
- `region_at_point` returns the tapped country's framing data in its `RegionHit`. `RegionHit` gains the country's projected bounding rectangle and projected centroid, computed in the single hit-test pass that already resolves the `CountryFeature` by point-in-polygon. This is the chosen shape (over exposing a surface-to-geographic helper and re-querying the geometry at the tap point): the tap resolves everything in one pass, no second geometry query. The selection path ignores the new fields.
- A pure area-weighted centroid helper in `artifact/geometry.rs` (no centroid exists today), used to fill `RegionHit`'s centroid.
- The driver owns the `requestAnimationFrame` loop, the target-viewport construction (it holds the framing constants, the surface aspect, and the ceiling), the time source, and the cancellation wiring.

### (a) The `Camera` state machine

Pure: no scheduling, no `web_sys`, reads the clock only through a time value the caller passes in. The driver stores it in an `Option<Camera>`.

```rust
/// An in-progress animated viewport transition. Interpolation happens in center-and-height space, so
/// both endpoints are stored as full viewports and the intermediate width is re-derived from the surface
/// aspect each frame rather than lerped. `start_time_ms` is the DOMHighResTimeStamp the driver passes from
/// requestAnimationFrame (the performance.now() clock); the camera does no scheduling.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    from: Viewport,
    target: Viewport,
    start_time_ms: f64,
    duration_ms: f64,
}

/// Whether a sampled frame is mid-transition (schedule another frame) or the last one (landed on the
/// target; stop the loop).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    Animating,
    Finished,
}

impl Camera {
    pub fn new(from: Viewport, target: Viewport, start_time_ms: f64, duration_ms: f64) -> Camera {
        Camera { from, target, start_time_ms, duration_ms }
    }

    /// The interpolated viewport at `now_ms` and whether the transition has finished. At or past the end
    /// (or a non-positive duration) it returns `target` verbatim with `Finished`, so the final frame is
    /// bit-identical to the caller's clamped target and a backgrounded-then-foregrounded tab snaps to the
    /// end on its first frame back.
    pub fn sample(&self, now_ms: f64, surface: SurfaceDimensions) -> (Viewport, Progress) {
        let elapsed_ms: f64 = now_ms - self.start_time_ms;

        if self.duration_ms <= 0.0 || elapsed_ms >= self.duration_ms {
            return (self.target, Progress::Finished);
        }

        let eased_t: f64 = ease_in_out_cubic((elapsed_ms / self.duration_ms).clamp(0.0, 1.0));

        (self.from.interpolate_to(self.target, eased_t, surface), Progress::Animating)
    }
}

fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let f: f64 = -2.0 * t + 2.0;
        1.0 - f * f * f / 2.0
    }
}
```

The final frame returns `self.target` verbatim, not `interpolate_to(.., 1.0, ..)`: rounding through the blend could land a fraction off, and on the antimeridian short-way case (see §d) `interpolate_to` at `t == 1` can differ from `target` by a whole `2π` turn. Tests assert the landing via `sample` at `Finished`, never against `interpolate_to` at `t == 1`. The camera does not re-clamp per frame: both endpoints are valid (the from-viewport is the live one C3.3 keeps valid; the target is clamped once in §b), and interpolating center-and-height between two points in the convex clamp region stays in the region (see §d).

### (b) Computing the target viewport

Computed once when the tap resolves, then handed to `Camera::new`.

1. **Framing data from the hit (Option B).** `region_at_point` already locates the `CountryFeature` by point-in-polygon; it now also computes, for the hit, the country's projected bounding rectangle and projected centroid and returns them in `RegionHit`. Both are computed from the feature's polygon vertices in one pass, after **unwrapping the longitudes into a contiguous frame** so antimeridian-crossing countries are handled uniformly, with no special-case branch and no fixed span. Natural Earth splits an antimeridian-crossing country (Russia, Fiji, the US Aleutians, NZ Chathams, Kiribati) into pieces near `+180` and `−180`, so a naive min/max longitude spans nearly 360° and would frame the whole globe. The unwrap fixes this via a **largest-longitude-gap cut**: gather the country's vertex longitudes, find the largest gap between adjacent values around the circle (including the wrap gap across ±180), and shift every longitude into the 360° window that starts just after that gap (`lon' = base + (lon − base).rem_euclid(360)`, where `base` is the occupied-arc start). For a non-crossing country the largest gap is the far-side wrap, so the unwrap is a no-op and the box is the ordinary min/max; for Fiji it rejoins the `−178°` piece to the `+179°` piece as `+179°..+182°` (a 3° span). This makes no reference-vertex assumption, so it is correct for any longitude span. The **projected rectangle** is then the min/max of `project(lat, lon')` over the vertices (Miller is separable and monotonic), and the **centroid** is the area-weighted centroid of the projected vertices (holes subtracted, multi-polygon components weighted by area) in the same unwrapped-then-projected frame, so it lands on the country's landmass rather than being averaged toward longitude 0. A seam-crossing country's projected rectangle and centroid may sit slightly past ±π in x; step 2's `normalize_longitude_turns` pulls the target back within one turn.
2. **The driver builds the target.** From the `RegionHit`'s projected rectangle and centroid, the driver calls `Viewport::fit_bounds` (§c) with the margin fraction, the surface, and the same zoom-out ceiling the manual path uses, then chains `clamp_vertical(home_min_y, home_max_y)` and `normalize_longitude_turns()` exactly as the wheel/pan/pinch paths do. This keeps the target inside the home latitude range and within one antimeridian turn.

The longitude-unwrap, projected-rectangle, and centroid together are a pure helper (unit-testable without the driver), living with the geometry it reads. Computing it in `region_at_point` means the hover path (which calls `region_at_point` every move) recomputes it each hit; the cost is sorting and one pass over the hit feature's vertices, comparable to the containment test already done for it, so it is acceptable for v1 (memoize per iso3 if a profile ever shows it).

### (c) The `Viewport` contain-fit helper

```rust
impl Viewport {
    /// A viewport containing the projected rectangle `[min_x, max_x] x [min_y, max_y]` centered on
    /// `center`, with `margin_fraction` padding, at the surface aspect and never stretched. The
    /// counterpart to `fill_height`: where `fill_height` frames only the vertical extent (letting the
    /// horizontal overflow), this frames the whole rectangle by taking whichever of its height or its
    /// width-over-aspect is larger, so both axes fit. Height is clamped into `[MIN_ZOOM_IN_HEIGHT, max_height]`.
    pub fn fit_bounds(
        min_x: f64,
        max_x: f64,
        min_y: f64,
        max_y: f64,
        center: ProjectedPoint,
        margin_fraction: f64,
        max_height: f64,
        surface: SurfaceDimensions,
    ) -> Viewport {
        let aspect: f64 = surface.width as f64 / surface.height as f64;
        let contain_height: f64 = (max_y - min_y).max((max_x - min_x) / aspect);
        let padded_height: f64 = contain_height * (1.0 + margin_fraction);

        let seed: Viewport = Viewport { min: center, max: center };

        seed.zoom_to_height(padded_height, max_height, surface)
    }
}
```

Centers on the passed `center` (the centroid), which may differ from the rectangle's geometric center. Width is re-derived by `zoom_to_height` from the padded height and aspect, so it is never stretched. The position clamps are chained by the caller (§b), matching how `zoom_to_height` is a construction primitive. A degenerate zero-extent rectangle clamps up to `MIN_ZOOM_IN_HEIGHT`; no special case.

### (d) The interpolation

```rust
impl Viewport {
    /// This viewport blended toward `target` by eased `t` in `[0, 1]`, interpolating center and visible
    /// height and re-deriving width from the surface aspect so no intermediate frame is stretched. Height
    /// is blended geometrically (constant perceived zoom rate); center linearly. The horizontal blend
    /// takes the short way across the antimeridian: the target center-x is unwrapped to within half a
    /// world turn of this center-x first.
    pub fn interpolate_to(&self, target: Viewport, t: f64, surface: SurfaceDimensions) -> Viewport {
        let from_center: ProjectedPoint = self.center();
        let target_center: ProjectedPoint = target.center();

        let center_x: f64 = lerp(from_center.x, unwrap_nearest(from_center.x, target_center.x), t);
        let center_y: f64 = lerp(from_center.y, target_center.y, t);
        let height: f64 = geometric_lerp(self.height(), target.height(), t);

        let seed: Viewport = Viewport {
            min: ProjectedPoint { x: center_x, y: center_y },
            max: ProjectedPoint { x: center_x, y: center_y },
        };

        seed.zoom_to_height(height, height, surface)
    }
}

/// `target` shifted by whole turns of `2π` to land within ±π of `reference` (the shortest horizontal move).
fn unwrap_nearest(reference: f64, target: f64) -> f64 {
    target - ((target - reference) / TAU).round() * TAU
}

/// A blend linear in the logarithm, so the ratio between successive frames is constant. Used for height
/// so the perceived zoom rate is uniform across a large scale change.
fn geometric_lerp(from: f64, to: f64, t: f64) -> f64 {
    from * (to / from).powf(t)
}
```

Interpolating center + height (not corners) and re-deriving width holds C3.3's aspect invariant on every frame. Height blends in log space so a large zoom does not crawl-then-rush; both endpoints are at least `MIN_ZOOM_IN_HEIGHT`, so `geometric_lerp` is well-defined and stays in the clamp interval. `zoom_to_height(height, height, ..)` makes the per-frame ceiling a no-op (the floor still applies harmlessly). Center-y stays in the home range without a per-frame `clamp_vertical`: both endpoints' center-y lie in the convex feasible set `{ min_y + h/2 <= center_y <= max_y - h/2 }`, and a linear center-y with monotone height traces a path that stays feasible. `unwrap_nearest` makes a seam-crossing zoom sweep at most half a turn instead of unrolling the globe.

### (e) The self-scheduling `requestAnimationFrame` loop

Distinct from the existing coalesced on-demand redraw (`request_redraw`, which schedules one frame per input event). The animation loop schedules a fresh frame at the end of each frame until the camera reports `Finished`.

New `Driver` state:

- `camera: Option<Camera>` — `Some` while a transition is in flight. The source of truth for "is a transition running."
- `animation_frame_pending: bool` — whether an animation frame is currently queued in the browser. This mirrors `redraw_pending` and is what makes exactly one loop exist across cancel/restart interleaving: `camera.is_some()` cannot serve this, because a stale queued frame from a cancelled animation can re-adopt a freshly started camera, so "running" and "frame scheduled" are two distinct facts.
- `animation_callback: Option<Closure<dyn FnMut(f64)>>` — the rAF closure, held for lifetime; it takes the `f64` timestamp, so the loop needs no separate clock read (a different closure type from `redraw_callback`, so a new field).

One scheduling helper `schedule_animation_frame(&mut self)` returns early when `animation_frame_pending` is set, else schedules `animation_callback` and sets the flag only on a successful schedule (the retryable discipline of `request_redraw`).

The loop: (1) **Start** — on a tap resolving to a country with a target, read the timestamp, build `Camera::new(self.viewport, target, now_ms, ANIMATION_DURATION_MS)`, store it, and `schedule_animation_frame` (a no-op if a stale frame is still queued, so no second loop). (2) **Each frame** `advance_animation(now_ms)` first clears `animation_frame_pending = false`; if `camera` is `None`, returns (a cancellation cleared it); else samples, sets `self.viewport`, and calls `self.draw()` directly (not `request_redraw`, which would fight the loop's scheduling); on `Animating` calls `schedule_animation_frame`, on `Finished` clears `self.camera = None` and stops. The stale-frame double-loop is impossible: scheduling is gated on `animation_frame_pending`, so a stale frame firing after a new camera started sees the flag false at its top, samples the current camera, draws, and reschedules once, while the start path's `schedule_animation_frame` was a no-op. No `cancelAnimationFrame` handle-tracking needed.

Coexistence with the coalesced redraw: a reactive redraw mid-animation (e.g. a statistic change) schedules its own one-shot frame, which draws the current `self.viewport` with the new frame state; both paths only read/write `self.viewport` and call `draw_frame`, so at most one extra draw per animation frame in the rare overlap. Tab backgrounding: rAF pauses; the first frame back is far past the end, so `sample` returns the target/`Finished` and the transition snaps to its end (acceptable; the glide's only value is continuity for a watching user).

Cancellation — set `self.camera = None` the instant the user takes manual control, so a gesture never fights the camera:

- Manual pan: at the top of `pan_view`, before the viewport mutation.
- Manual wheel/trackpad zoom: at the top of `zoom_at`.
- Manual pinch: at the top of `pinch_view`.
- A new press: in `handle_pointer_down`'s driver-borrow block (not in `Gesture::begin`, which is a pure enum method with no `Driver` access), so a press cancels any in-flight camera; if the press turns into a tap on another country, that release starts a fresh animation from the current (cancelled) viewport, a smooth redirect.
- Resize: `resize` refits `self.viewport` to the new aspect; a camera in flight holds old-aspect endpoints, so `resize` clears `self.camera = None` (continuing to interpolate old-aspect endpoints at a new aspect would briefly distort, since `interpolate_to` assumes both endpoints already have the surface aspect).
- Detach/teardown: the `DRIVER` thread-local holds the whole `Driver`; teardown drops it and the `animation_callback` closure, so no frame fires against a dead driver.

### (f) Click-to-zoom coexisting with the C3.2 selection

The camera start is driven off the raw `RegionHit`, resolved once on `pointerup` for an unmoved tap (`region_at`), and the same hit is handed to `select_region` for the panel publish. The zoom does not fire on a plain first selection; it fires only when the tap lands on the country that is already selected, so a first tap selects and a second tap (or the second click of a double-click) zooms. On such a re-tap the camera computes the target and, if the current viewport differs from it beyond a small tolerance, animates (a re-tap while already framed is a no-op; a re-tap after panning away re-frames). A tap on open ocean, or a first tap on a new country, starts no animation. Selection and animation are independent effects of the tap: the selection state paints on the next frame; the animation moves the viewport over its duration. The camera start stays in the driver (it needs the rAF timestamp), inside the borrow `handle_pointer_up` already holds.

> Deviations from the original plan:
> - `select_region_at(surface_point)` (which resolved the hit internally) was refactored to `select_region(Option<RegionHit>)` so the tap handler resolves the `RegionHit` exactly once and shares it between the camera start and the selection publish, rather than hit-testing twice.
> - The trigger changed from "a tap animates" to "re-tapping the already-selected country animates" (a first tap only selects). Auto-zooming on every selection hurt the exploration/comparison flow by moving the viewport unasked; gating the zoom behind a second tap keeps plain selection instant while still offering the framing on demand (a double-click zooms because its first click selects and its second re-taps the selection).

### (g) Documentation updates

Three docs assert this feature is out of v1 and move together (matching the C3 scope decision at the top of this plan):

- `docs/design/README.md` §Animation: the blanket "Through v1, there are no animations at all. State changes are instant." carves out the zoom-to-country transition (the only v1 animation: a short cubic-eased viewport move on country selection; all other state changes stay instant). The nearby "no animations in <v2" line is NOT edited: it lives in the `## Origin` provenance quote of the author's 2026-06-15 prompt, and the authoritative §Animation prose above it now carries the carve-out, so the verbatim record is left intact.
- `specs/006-core-renderer/spec.md` §Scope cutoff: the "zoom-to-country Camera ... v1.5+" bullet changes to "implemented in `003-web-client` phase C3.4," noting the camera lives at `shared::map::camera::Camera` (not the aspirational `core::geometry::animation::Camera`) and the contain-fit returns as `Viewport::fit_bounds`. The `hover_scale` bullet stays deferred.
- `specs/006-core-renderer/checklists/requirements.md`: the clause pairing "zoom-to-country Camera state machine" with "v1.5+" is updated to reflect the C3.4 inclusion.

After editing, grep `specs/` and `docs/` for `v1.5` and "no animations" to confirm nothing authoritative still asserts the feature is deferred. Two "no animations" hits are expected to remain: the `## Origin` provenance quote in `docs/design/README.md` (deliberately unedited, see above) and the static-stub descriptions (the stub HTML fixtures genuinely have no animation).

> Deviation from the original plan: the plan called for a parenthetical carve-out on the "no animations in <v2" blockquote. That line is a preserved-for-provenance verbatim author quote, so editing it would corrupt the record; it is left untouched and the authoritative §Animation prose carries the carve-out instead.

### Testing

Pure `shared::map` / geometry host tests (no `web_sys`, no render feature):

- `ease_in_out_cubic`: `p(0) == 0`, `p(1) == 1`, `p(0.5) == 0.5`, monotonic.
- `geometric_lerp`: `t=0` is `from`, `t=1` is `to`, constant ratio across equal steps, stays in `[from, to]`.
- `Camera::sample`: at `start` returns the from-viewport (tolerance) and `Animating`; at/after `start+duration`, and far past it (backgrounded tab), and for a non-positive duration, returns `target` verbatim and `Finished`; a mid sample lies between the endpoints and has the surface aspect. Assert the `Finished` landing against the stored `target`, never `interpolate_to(target, 1.0)`.
- `Viewport::fit_bounds`: contains the rectangle plus margin; aspect matches on square/wide/tall; a wide rectangle frames by width, a tall one by height; a zero-extent rectangle clamps to the floor; height clamps to the ceiling; centered on the passed `center`.
- `Viewport::interpolate_to`: aspect matches the surface at every `t` (no distortion); `t=0`/`t=1` hit the endpoints' center/height; center-y is the linear blend, height the geometric blend; the antimeridian short-way case sweeps center-x less than half a turn.
- `unwrap_nearest`: a far target shifts to within ±π; a near one is unchanged; the shift is a whole `2π` multiple.
- The centroid helper: a convex ring's centroid is its geometric center; a hole subtracts; a multi-polygon weights by component area; an antimeridian-split feature centers on the dominant landmass, not `0°`.
- Target framing: a synthetic feature whose pieces sit near `−179°` and `+179°` (the antimeridian-split shape) does not frame the whole world; the largest-gap unwrap rejoins the pieces so the projected rectangle spans the true few-degree width and the centroid lands between the pieces, not at longitude 0. Extract the longitude-unwrap-plus-projected-rectangle-plus-centroid computation as a pure helper so this is testable without the driver.
- `region_at_point`: the returned `RegionHit` carries the correct projected bbox and centroid for a hit (extend the existing hit-test tests).

Driver wiring (the rAF loop, the `animation_frame_pending` gate, cancellation on each gesture and press and resize, selection on the first tap and animation on a re-tap of the already-selected country, both off the raw hit) is thin glue verified in the browser per the wasm-test convention: a first tap on a new country selects without moving the map; a re-tap (or double-click) of a small country glides it centered and framed; a large one zooms out to frame it; an antimeridian country frames its landmass, not the globe; a mid-animation pan/wheel/pinch stops the glide instantly; a re-tap mid-glide redirects smoothly with no double-speed runaway; re-tapping after panning recenters; re-tapping an already-framed country does nothing; a seam-crossing zoom sweeps the short way; an ocean tap deselects with no move. `cargo test -p shared` covers the pure tests; `cargo check --target wasm32-unknown-unknown -p web` confirms the driver compiles.

### Constants (tunable in-browser)

`ANIMATION_DURATION_MS = 600.0` (the transition length) and `ZOOM_TO_COUNTRY_MARGIN_FRACTION = 0.1` are named driver constants; the design does not depend on their exact values. `MIN_ZOOM_IN_HEIGHT`, the zoom-out ceiling, and the home range are reused unchanged. The antimeridian framing needs no tuning constant: the largest-longitude-gap unwrap derives each country's true extent, so there is no fixed regional span.

### Out of scope for C3.4

Inertial/momentum pan (backlog). Animating anything other than the zoom-to-country transition. A general animation framework. Zoom-to-country from a source other than a map tap (search/deep-link; the `Camera` and target primitives are reusable for that later). A cached by-iso3 geometry index.

### PR description (draft)

**shared** — Add the animated zoom-to-country primitives. A pure `Camera` (`shared::map::camera`) holds the from- and target-viewports, a start time, and a duration, and samples an interpolated viewport at a given time with cubic ease-in-out, reporting when it has finished. `Viewport::fit_bounds` returns as the contain-style counterpart to `fill_height`, framing a projected rectangle within the surface aspect with a margin. `Viewport::interpolate_to` blends two viewports in center-and-height space, re-deriving the width each frame (no distortion), interpolating height geometrically and taking the shortest path across the antimeridian. `region_at_point` now returns the tapped country's projected bounding rectangle and area-weighted centroid in its `RegionHit`, and `artifact::geometry` gains a pure centroid helper; antimeridian-split countries frame off their landmass rather than the globe.

**web** — On a country tap the map both selects the country (unchanged) and animates the viewport to frame it, over a short cubic-eased transition driven by a self-scheduling `requestAnimationFrame` loop distinct from the on-demand redraw and guarded by an `animation_frame_pending` flag so a mid-glide re-tap redirects without a second loop. The camera start is driven off the raw hit, so re-tapping a panned-away country recenters it. The animation cancels the instant the user takes manual control (pan, wheel/trackpad zoom, pinch, a new press, or a resize), handing off from the current viewport. The target is clamped to the same zoom-in floor, zoom-out ceiling, and home latitude range as manual zoom.

**docs** — `docs/design/README.md` §Animation, `specs/006-core-renderer/spec.md` §Scope cutoff, and its requirements checklist move animated zoom-to-country from deferred (v1.5+) into v1.
