use crate::canonical::StatisticKind;

/// An sRGB color; each component is in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// The accent red, `#e60019`.
const ACCENT_FILL: Rgba = Rgba {
    r: 230.0 / 255.0,
    g: 0.0,
    b: 25.0 / 255.0,
    a: 1.0,
};

/// The white base, `#fff`.
const WHITE_FILL: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

/// The no-data grey, `#666666`.
const NO_DATA_FILL: Rgba = Rgba {
    r: 102.0 / 255.0,
    g: 102.0 / 255.0,
    b: 102.0 / 255.0,
    a: 1.0,
};

pub struct ColorScale {
    low: Rgba,
    high: Rgba,
    no_data: Rgba,
    interpolator: fn(Rgba, Rgba, f32) -> Rgba,
}

impl ColorScale {
    /// The scale's color at normalized position `t` in `[0, 1]` (`low` at 0, `high` at 1). A `StatisticColorTransform`
    /// produces `t` from a raw value; the legend samples this to match what the map paints.
    pub fn sample(&self, t: f32) -> Rgba {
        (self.interpolator)(self.low, self.high, t)
    }

    pub fn no_data(&self) -> Rgba {
        self.no_data
    }
}

/// The choropleth color scale: accent red at position 0, white at position 1 (the TFR direction, where
/// the most-saturated red marks the lowest value), grey for no-data.
/// The value → position mapping is a separate, per-statistic `StatisticColorTransform`.
pub const CHOROPLETH_SCALE: ColorScale = ColorScale {
    low: ACCENT_FILL,
    high: WHITE_FILL,
    no_data: NO_DATA_FILL,
    interpolator: srgb_lerp,
};

/// Maps a raw statistic value to a position in `[0, 1]` along a `ColorScale`. The per-statistic knob that
/// decides how values spread across the palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatisticColorTransform {
    /// Linear normalization against the observed data range: `min` → 0, `max` → 1, clamped. Data-relative.
    Linear,
    /// A C² curve keyed to absolute values: a convex cubic on `[0, x0]` meeting a concave arctan tail at the
    /// inflection `x0`, through the origin and asymptotic to 1. `y0` is the inflection height (vertically
    /// draggable); `toe` in `[0, 1]` sets the toe convexity (0 linear, 1 flat start), normalized so any `y0`
    /// stays monotonic. `min`/`max` are ignored.
    PiecewiseCubicArctan { x0: f64, y0: f64, toe: f64 },
}

impl StatisticColorTransform {
    /// The position in `[0, 1]` for `value`. `min`/`max` are used only by `Linear`.
    pub fn position(&self, value: f64, min: f64, max: f64) -> f32 {
        let position: f64 = match self {
            StatisticColorTransform::Linear => linear_normalization(value, min, max),
            StatisticColorTransform::PiecewiseCubicArctan { x0, y0, toe } => piecewise_cubic_arctan(value, *x0, *y0, *toe),
        };

        position as f32
    }

    /// The value where the curve pivots (color changes fastest): `Some(x0)` for `PiecewiseCubicArctan`,
    /// `None` for `Linear`. The legend marks it generically.
    pub fn inflection(&self) -> Option<f64> {
        match self {
            StatisticColorTransform::Linear => None,
            StatisticColorTransform::PiecewiseCubicArctan { x0, .. } => Some(*x0),
        }
    }
}

pub fn transform_for(statistic: StatisticKind) -> StatisticColorTransform {
    match statistic {
        StatisticKind::Tfr => StatisticColorTransform::PiecewiseCubicArctan { x0: 2.1, y0: 0.65, toe: 0.5 },
        StatisticKind::TestAlpha => StatisticColorTransform::Linear,
    }
}

fn linear_normalization(value: f64, min: f64, max: f64) -> f64 {
    let range: f64 = max - min;
    if range == 0.0 {
        return 0.0;
    }

    ((value - min) / range).clamp(0.0, 1.0)
}

/// The piecewise transform at `value`, clamped to `[0, 1]`. Cubic `h` on `[0, x0]` and arctan `g` on
/// `(x0, ∞)` are solved to agree in value, slope, and curvature at `x0` (C²): `h(0)=0`, `h(x0)=y0`,
/// `h'(x0)=g'(x0)=s`, `h''(x0)=g''(x0)=0`. The inflection slope `s` is derived from the normalized `toe`
/// so it always lands inside the band that keeps `h` monotonic and convex.
fn piecewise_cubic_arctan(value: f64, x0: f64, y0: f64, toe: f64) -> f64 {
    if value <= 0.0 {
        return 0.0;
    }

    let inflection_slope: f64 = (y0 / x0) * (1.0 + toe / 2.0);

    let position: f64 = if value <= x0 {
        let a3: f64 = (y0 - inflection_slope * x0) / (x0 * x0 * x0);
        let a2: f64 = -3.0 * a3 * x0;
        let a1: f64 = inflection_slope + 3.0 * a3 * x0 * x0;

        a3 * value * value * value + a2 * value * value + a1 * value
    } else {
        let amplitude: f64 = 2.0 * (1.0 - y0) / std::f64::consts::PI;
        let steepness: f64 = inflection_slope / amplitude;

        amplitude * (steepness * (value - x0)).atan() + y0
    };

    position.clamp(0.0, 1.0)
}

fn srgb_lerp(from: Rgba, to: Rgba, t: f32) -> Rgba {
    Rgba {
        r: lerp(from.r, to.r, t),
        g: lerp(from.g, to.g, t),
        b: lerp(from.b, to.b, t),
        a: lerp(from.a, to.a, t),
    }
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + t * (to - from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 1e-6;

    const TFR_X0: f64 = 2.1;
    const TFR_Y0: f64 = 0.65;
    const TFR_TOE: f64 = 0.5;

    fn assert_color_approx(actual: Rgba, expected: Rgba) {
        assert!((actual.r - expected.r).abs() < TOLERANCE, "r: {} vs {}", actual.r, expected.r);
        assert!((actual.g - expected.g).abs() < TOLERANCE, "g: {} vs {}", actual.g, expected.g);
        assert!((actual.b - expected.b).abs() < TOLERANCE, "b: {} vs {}", actual.b, expected.b);
        assert!((actual.a - expected.a).abs() < TOLERANCE, "a: {} vs {}", actual.a, expected.a);
    }

    fn tfr(value: f64) -> f64 {
        piecewise_cubic_arctan(value, TFR_X0, TFR_Y0, TFR_TOE)
    }

    #[test]
    fn sample_maps_endpoints_to_accent_and_white() {
        assert_color_approx(CHOROPLETH_SCALE.sample(0.0), ACCENT_FILL);
        assert_color_approx(CHOROPLETH_SCALE.sample(1.0), WHITE_FILL);
    }

    #[test]
    fn no_data_is_the_mid_dark_grey() {
        assert_color_approx(CHOROPLETH_SCALE.no_data(), NO_DATA_FILL);
    }

    #[test]
    fn linear_matches_range_normalization() {
        let linear: StatisticColorTransform = StatisticColorTransform::Linear;

        assert!((linear.position(1.0, 1.0, 3.0) - 0.0).abs() < TOLERANCE);
        assert!((linear.position(3.0, 1.0, 3.0) - 1.0).abs() < TOLERANCE);
        assert!((linear.position(2.0, 1.0, 3.0) - 0.5).abs() < TOLERANCE);
    }

    #[test]
    fn linear_clamps_out_of_range_and_handles_zero_range() {
        let linear: StatisticColorTransform = StatisticColorTransform::Linear;

        assert!((linear.position(0.0, 1.0, 3.0) - 0.0).abs() < TOLERANCE);
        assert!((linear.position(9.0, 1.0, 3.0) - 1.0).abs() < TOLERANCE);
        assert!((linear.position(5.0, 4.0, 4.0) - 0.0).abs() < TOLERANCE);
    }

    #[test]
    fn piecewise_passes_through_origin_and_inflection() {
        assert!(tfr(0.0).abs() < 1e-9);
        assert!((tfr(TFR_X0) - TFR_Y0).abs() < 1e-9);
    }

    #[test]
    fn piecewise_is_continuous_in_value_slope_and_curvature_at_the_seam() {
        let h: f64 = 1e-4;

        // C0: the cubic just below and the arctan just above the seam agree in value.
        assert!((tfr(TFR_X0 - h) - tfr(TFR_X0 + h)).abs() < 1e-4);

        // C1: one-sided slopes agree, and both equal the derived inflection slope s.
        let inflection_slope: f64 = (TFR_Y0 / TFR_X0) * (1.0 + TFR_TOE / 2.0);
        let slope_left: f64 = (tfr(TFR_X0) - tfr(TFR_X0 - h)) / h;
        let slope_right: f64 = (tfr(TFR_X0 + h) - tfr(TFR_X0)) / h;
        assert!((slope_left - slope_right).abs() < 1e-3);
        assert!((slope_left - inflection_slope).abs() < 1e-3);

        // C2: one-sided second differences both vanish at the seam.
        let curvature_left: f64 = (tfr(TFR_X0) - 2.0 * tfr(TFR_X0 - h) + tfr(TFR_X0 - 2.0 * h)) / (h * h);
        let curvature_right: f64 = (tfr(TFR_X0 + 2.0 * h) - 2.0 * tfr(TFR_X0 + h) + tfr(TFR_X0)) / (h * h);
        assert!(curvature_left.abs() < 1e-2, "left curvature {curvature_left}");
        assert!(curvature_right.abs() < 1e-2, "right curvature {curvature_right}");
    }

    #[test]
    fn piecewise_is_convex_before_and_concave_after_the_inflection() {
        let h: f64 = 1e-3;
        let second_difference = |x: f64| (tfr(x + h) - 2.0 * tfr(x) + tfr(x - h)) / (h * h);

        assert!(second_difference(1.0) > 0.0, "expected convex below the inflection");
        assert!(second_difference(3.5) < 0.0, "expected concave above the inflection");
    }

    #[test]
    fn piecewise_is_monotonically_increasing() {
        let mut previous: f64 = tfr(0.0);
        let mut value: f64 = 0.01;
        while value <= 8.0 {
            let current: f64 = tfr(value);
            assert!(current >= previous, "not monotonic at {value}: {current} < {previous}");
            previous = current;
            value += 0.01;
        }
    }

    #[test]
    fn piecewise_approaches_one_and_stays_clamped() {
        assert!(tfr(6.9) > 0.9);
        assert!(tfr(100.0) <= 1.0);
        assert!(tfr(100.0) > 0.99);
    }

    #[test]
    fn piecewise_stays_valid_across_a_sweep_of_inflection_height_and_toe() {
        for y0_step in 1..=18 {
            let y0: f64 = y0_step as f64 * 0.05; // 0.05 .. 0.90
            for toe_step in 0..=10 {
                let toe: f64 = toe_step as f64 * 0.1; // 0.0 .. 1.0
                let mut previous: f64 = 0.0;
                let mut value: f64 = 0.0;
                while value <= 8.0 {
                    let current: f64 = piecewise_cubic_arctan(value, TFR_X0, y0, toe);
                    assert!(
                        current >= previous - 1e-12 && (0.0..=1.0).contains(&current),
                        "invalid at value={value} y0={y0} toe={toe}: {current} (prev {previous})"
                    );
                    previous = current;
                    value += 0.02;
                }
            }
        }
    }

    #[test]
    fn inflection_is_some_for_piecewise_and_none_for_linear() {
        assert_eq!(StatisticColorTransform::Linear.inflection(), None);
        assert_eq!(
            StatisticColorTransform::PiecewiseCubicArctan { x0: TFR_X0, y0: TFR_Y0, toe: TFR_TOE }.inflection(),
            Some(TFR_X0),
        );
    }

    #[test]
    fn transform_for_selects_the_piecewise_curve_only_for_tfr() {
        assert_eq!(
            transform_for(StatisticKind::Tfr),
            StatisticColorTransform::PiecewiseCubicArctan { x0: 2.1, y0: 0.65, toe: 0.5 },
        );
        assert_eq!(transform_for(StatisticKind::TestAlpha), StatisticColorTransform::Linear);
    }
}
