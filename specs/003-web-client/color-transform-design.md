# Choropleth color-scale transform function — design

- Date: 2026-07-27
- Branch: `web-map-color-transfer` (stacked on `web-map-legend`)
- Affected crates: `shared` (color + renderer), `web` (legend + labels + i18n)

## Problem

The map colors each country by linearly normalizing its value against the shard's observed min/max, then blending accent-red → white across that `[0, 1]` position (`ColorScale::fill`). This makes the coloring data-relative and linear: the palette says nothing about demographically meaningful thresholds, and the same TFR gets a different color as the data range shifts.

We want the value → color-position mapping to be a per-statistic, **absolute**, **nonlinear** curve. For TFR specifically: a curve keyed to literal TFR values, inflecting at replacement (2.1), so color changes fastest around the threshold that matters and compresses the extremes.

## Concept: transform vs. interpolator

The pipeline already has two conceptual steps; today the first is implicit and linear. We make it explicit and swappable:

```
color = interpolator(low, high, transform(value))
```

- **`interpolator`** — blends `low` → `high` given a position `t ∈ [0, 1]`. Universal across statistics (red = low, white = high). The `srgb_lerp` on `CHOROPLETH_SCALE`. Unchanged by this work.
- **`transform`** — maps a raw statistic value → position `t`. Today this is the inline `(value - min) / (max - min)` clamp in `ColorScale::fill`. This is the piece we make per-statistic and (for TFR) nonlinear + absolute.

## `StatisticColorTransform`

A data-bearing sum type (bare name per `docs/conventions/types.md` — it carries parameters and reads as the transform value, not a tag-classification of some `StatisticColorTransform` struct). Lives in `shared/src/map/color.rs`.

```rust
pub enum StatisticColorTransform {
    /// Linear normalization against the observed data range (the pre-existing behavior; the default
    /// for statistics without a dedicated curve). Data-relative: `min`/`max` define the endpoints.
    Linear,
    /// Piecewise cubic (convex) → arctan (concave), C² at the inflection. Absolute: keyed to literal
    /// values, independent of `min`/`max`.
    PiecewiseCubicArctan { x0: f64, y0: f64, toe: f64 },
}

impl StatisticColorTransform {
    /// Value → position in `[0, 1]`. `min`/`max` are used only by `Linear`.
    pub fn position(&self, value: f64, min: f64, max: f64) -> f32 { ... }

    /// The value where the curve pivots (color changes fastest): `Some(x0)` for `PiecewiseCubicArctan`,
    /// `None` for `Linear`. The legend marks this generically.
    pub fn inflection(&self) -> Option<f64> { ... }
}
```

`ColorScale::fill(value, min, max)` is removed; its normalization becomes `StatisticColorTransform::Linear`'s `position`. `ColorScale` keeps `sample(t)` and `no_data()`.

## The `PiecewiseCubicArctan` curve

Parameters (with defaults chosen in the tuner):

- `x0` — inflection value (TFR at the pivot). Default `2.1` (replacement).
- `y0` — inflection height (position at the pivot). Default `0.65`. Vertically draggable.
- `toe ∈ [0, 1]` — toe convexity. Default `0.5`. `0` = linear toe, `1` = flat start.

Derived:

- inflection slope `s = (y0 / x0) · (1 + toe / 2)` — the normalized `toe` keeps `s` inside its valid band `(y0/x0, 1.5·y0/x0)` for **any** `y0 ∈ (0, 1)`, so vertical drag never breaks monotonicity or convexity.
- `A = 2·(1 − y0) / π`, `B = s / A`.

Left piece, `x ≤ x0` (cubic; convex toe):

```
h(x) = a3·x³ + a2·x² + a1·x
a3 = (y0 − s·x0) / x0³
a2 = −3·a3·x0
a1 = s + 3·a3·x0²
```

Right piece, `x > x0` (arctan; concave tail → 1):

```
g(x) = A·arctan(B·(x − x0)) + y0
```

Output clamped to `[0, 1]`. `position(value, _, _)` returns `0` for `value ≤ 0`.

Guaranteed properties (verified numerically during design):

- `f(0) = 0`; monotonic increasing; `f → 1` as `x → ∞`.
- C² at `x0`: value `y0`, slope `s`, curvature `0` match across both pieces (true inflection — `f″ > 0` on the cubic, `f″ < 0` on the arctan).

## Per-statistic selection

```rust
pub fn transform_for(statistic: StatisticKind) -> StatisticColorTransform {
    match statistic {
        StatisticKind::Tfr => StatisticColorTransform::PiecewiseCubicArctan { x0: 2.1, y0: 0.65, toe: 0.5 },
        _                  => StatisticColorTransform::Linear,
    }
}
```

The `Tfr` parameters are compile-time constants, chosen with `docs/design/color-scale-tuner.html`. Runtime / UI tuning is out of scope (the parameterization supports it later; the vertical-drag requirement is satisfied by any `y0` being valid).

## Renderer

`compute_fill_colors` (in `shared/src/map/renderer.rs`) computes each country's fill as:

```rust
let transform: StatisticColorTransform = color::transform_for(frame_state.active_statistic);
// per country:
let position: f32 = transform.position(value, statistic_min, statistic_max);
let fill: FillVertex = color::CHOROPLETH_SCALE.sample(position).to_gpu();
// no value:
let no_data: FillVertex = color::CHOROPLETH_SCALE.no_data().to_gpu();
```

It keeps reading `value_range()` — needed for `Linear`, harmless (unused) for `PiecewiseCubicArctan`.

## Legend

`LegendView` grows from `{ value_range }` to `{ statistic, value_range }`. The legend selects `transform_for(statistic)` and renders per transform variant:

- **`PiecewiseCubicArctan`**: axis `[data_min, data_max]`; gradient sampled through the transform (color at value `v` is `CHOROPLETH_SCALE.sample(transform.position(v, min, max))`, so legend and map agree per value); a hairline **inflection marker** at `transform.inflection()` (drawn only when it falls within `[min, max]`), showing its numeric value; ticks at `min / inflection / max`.
- **`Linear`**: axis `[data_min, data_max]`; linear gradient; no inflection marker (today's legend).
- Title = the active statistic's label (`labels::statistic_label`), not a generic "Legend".
- No-data swatch unchanged.

**Generalizable inflection caption.** The semantic caption under the inflection marker ("replacement" for TFR) is an *optional per-statistic label*, not baked into the legend:

```rust
// web/src/map/labels.rs
pub fn reference_caption(i18n: I18nContext<Locale>, statistic: StatisticKind) -> Option<AnyView> {
    match statistic {
        StatisticKind::Tfr => Some(t!(i18n, legend.replacement).into_any()),
        _                  => None,
    }
}
```

A statistic with a piecewise curve but no meaningful threshold shows the inflection tick + number without a caption; a `Linear` statistic shows no marker at all. Nothing TFR-specific lives in the generic legend.

## i18n additions

Delta against the C2.6 legend keys (`web/locales/en.json`):

- **Add** `legend.replacement` — the TFR reference caption (e.g. "replacement"). Label, no trailing period.
- **Drop** `legend.title` — the title now comes from `labels::statistic_label` (the active statistic's name), not a generic "Legend".
- **Drop** `legend.low` / `legend.high` — replaced by numeric axis ticks (`min / inflection / max`) plus the inflection caption.
- **Keep** `legend.no_data`.

## Testing

- `shared/src/map/color.rs` unit tests:
  - `PiecewiseCubicArctan::position`: `f(0)=0`; `f(x0)=y0`; C² continuity at `x0` (value, slope, curvature match across the seam within tolerance); monotonic on a dense sample; convex before / concave after `x0`; clamp above 1 as `x → large`; validity across a sweep of `y0` and `toe`.
  - `Linear::position`: matches the old `choropleth_fill` normalization (min → 0, max → 1, midpoint, clamp, `range == 0`).
  - `inflection()`: `Some(x0)` for piecewise, `None` for linear.
  - `transform_for(Tfr)` returns the piecewise variant with the documented parameters.
- Legend component: light render check that the piecewise path marks the inflection and the linear path does not. (Web component test only where it adds coverage beyond the pure `position` tests.)

## Out of scope

- **High-value color** — still white; changing it is a separate backlog item (`docs/backlog.md`), independent of this transform work.
- No-data color — already mid-dark grey.
- Runtime / UI parameter tuning and horizontal (`x0`) dragging beyond the parameter existing.
- Multi-statistic reality — only `Tfr` is in production; the `Linear` default and `reference_caption` `None` arms are exercised by the test-only statistics and future additions.

## PR description (draft)

**shared** — Introduce `StatisticColorTransform` (`Linear` | `PiecewiseCubicArctan`), a per-statistic value → color-position mapping selected by `transform_for(StatisticKind)`. The TFR curve is a C² piecewise cubic → arctan keyed to absolute TFR, inflecting at replacement (2.1); the linear normalization is preserved as the default for other statistics. The renderer colors each country through the active statistic's transform.

**web** — The legend renders per transform: for the piecewise curve it samples the gradient through the transform over the data range and marks the inflection with an optional per-statistic caption; linear statistics keep the plain min→max legend. Adds `LegendView { statistic, value_range }` and a `reference_caption` label lookup.
