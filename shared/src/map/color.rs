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

/// The no-data grey, `#e6e6e6`.
const NO_DATA_FILL: Rgba = Rgba {
    r: 230.0 / 255.0,
    g: 230.0 / 255.0,
    b: 230.0 / 255.0,
    a: 1.0,
};

pub struct ColorRamp {
    low: Rgba,
    high: Rgba,
    no_data: Rgba,
    interpolate: fn(Rgba, Rgba, f32) -> Rgba,
}

impl ColorRamp {
    /// The ramp color at normalized position `t` in `[0, 1]` (`low` at 0, `high` at 1). Lets a consumer
    /// (the legend) sample the exact ramp the choropleth paints.
    pub fn sample(&self, t: f32) -> Rgba {
        (self.interpolate)(self.low, self.high, t)
    }

    pub fn no_data(&self) -> Rgba {
        self.no_data
    }

    /// The fill for `value` normalized against `[min, max]`. A `None` value is the no-data color, kept
    /// distinct from the `high` endpoint so absent data does not read as a high value.
    pub fn fill(&self, value: Option<f64>, min: f64, max: f64) -> Rgba {
        let Some(value) = value else {
            return self.no_data;
        };

        let range: f64 = max - min;
        let normalized: f64 = if range == 0.0 {
            0.0
        } else {
            ((value - min) / range).clamp(0.0, 1.0)
        };

        self.sample(normalized as f32)
    }
}

/// The choropleth ramp: accent red at the statistic minimum, white at the maximum (the TFR direction,
/// where the most-saturated red marks the lowest value), grey for no-data. Point `interpolate` at a
/// different function to move the ramp off the per-channel sRGB lerp.
pub const CHOROPLETH_RAMP: ColorRamp = ColorRamp {
    low: ACCENT_FILL,
    high: WHITE_FILL,
    no_data: NO_DATA_FILL,
    interpolate: srgb_lerp,
};

/// Per-channel linear interpolation in sRGB space.
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

    fn assert_fill_approx(actual: Rgba, expected: Rgba) {
        assert!(
            (actual.r - expected.r).abs() < TOLERANCE,
            "r: {} vs {}",
            actual.r,
            expected.r
        );
        assert!(
            (actual.g - expected.g).abs() < TOLERANCE,
            "g: {} vs {}",
            actual.g,
            expected.g
        );
        assert!(
            (actual.b - expected.b).abs() < TOLERANCE,
            "b: {} vs {}",
            actual.b,
            expected.b
        );
        assert!(
            (actual.a - expected.a).abs() < TOLERANCE,
            "a: {} vs {}",
            actual.a,
            expected.a
        );
    }

    #[test]
    fn fill_maps_none_to_no_data_gray() {
        assert_fill_approx(CHOROPLETH_RAMP.fill(None, 1.0, 3.0), NO_DATA_FILL);
    }

    #[test]
    fn fill_maps_minimum_to_accent() {
        assert_fill_approx(CHOROPLETH_RAMP.fill(Some(1.0), 1.0, 3.0), ACCENT_FILL);
    }

    #[test]
    fn fill_maps_maximum_to_white() {
        assert_fill_approx(CHOROPLETH_RAMP.fill(Some(3.0), 1.0, 3.0), WHITE_FILL);
    }

    #[test]
    fn fill_lerps_the_midpoint() {
        let midpoint_fill: Rgba = CHOROPLETH_RAMP.fill(Some(2.0), 1.0, 3.0);
        let expected: Rgba = Rgba {
            r: (ACCENT_FILL.r + WHITE_FILL.r) / 2.0,
            g: (ACCENT_FILL.g + WHITE_FILL.g) / 2.0,
            b: (ACCENT_FILL.b + WHITE_FILL.b) / 2.0,
            a: 1.0,
        };
        assert_fill_approx(midpoint_fill, expected);
    }

    #[test]
    fn fill_clamps_out_of_range_values() {
        assert_fill_approx(CHOROPLETH_RAMP.fill(Some(0.0), 1.0, 3.0), ACCENT_FILL);
        assert_fill_approx(CHOROPLETH_RAMP.fill(Some(9.0), 1.0, 3.0), WHITE_FILL);
    }

    #[test]
    fn sample_maps_endpoints() {
        assert_fill_approx(CHOROPLETH_RAMP.sample(0.0), ACCENT_FILL);
        assert_fill_approx(CHOROPLETH_RAMP.sample(1.0), WHITE_FILL);
    }
}
