/// An sRGB color; each component is in `[0, 1]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
// bytemuck is a render-only optional dependency; gate its derives so this stays buildable without it.
#[cfg_attr(feature = "render", derive(bytemuck::Pod, bytemuck::Zeroable))]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    fn lerp(self, to: Rgba, t: f32) -> Rgba {
        Rgba {
            r: lerp(self.r, to.r, t),
            g: lerp(self.g, to.g, t),
            b: lerp(self.b, to.b, t),
            a: lerp(self.a, to.a, t),
        }
    }
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

/// RGBA fill for one country: a continuous lerp with the accent at `statistic_min` and white at
/// `statistic_max` (the TFR direction, where the most-saturated red marks the lowest value). A
/// `None` value (no data at the active period) is the no-data grey, kept distinct from the white
/// max-value endpoint so absent data does not read as a high value.
pub fn choropleth_fill(value: Option<f64>, statistic_min: f64, statistic_max: f64) -> Rgba {
    let Some(value) = value else {
        return NO_DATA_FILL;
    };

    let range: f64 = statistic_max - statistic_min;
    let normalized: f64 = if range == 0.0 {
        0.0
    } else {
        ((value - statistic_min) / range).clamp(0.0, 1.0)
    };

    ACCENT_FILL.lerp(WHITE_FILL, normalized as f32)
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
    fn choropleth_fill_maps_none_to_no_data_gray() {
        assert_fill_approx(choropleth_fill(None, 1.0, 3.0), NO_DATA_FILL);
    }

    #[test]
    fn choropleth_fill_maps_minimum_to_accent() {
        assert_fill_approx(choropleth_fill(Some(1.0), 1.0, 3.0), ACCENT_FILL);
    }

    #[test]
    fn choropleth_fill_maps_maximum_to_white() {
        assert_fill_approx(choropleth_fill(Some(3.0), 1.0, 3.0), WHITE_FILL);
    }

    #[test]
    fn choropleth_fill_lerps_the_midpoint() {
        let midpoint_fill: Rgba = choropleth_fill(Some(2.0), 1.0, 3.0);
        let expected: Rgba = Rgba {
            r: (ACCENT_FILL.r + WHITE_FILL.r) / 2.0,
            g: (ACCENT_FILL.g + WHITE_FILL.g) / 2.0,
            b: (ACCENT_FILL.b + WHITE_FILL.b) / 2.0,
            a: 1.0,
        };
        assert_fill_approx(midpoint_fill, expected);
    }

    #[test]
    fn choropleth_fill_clamps_out_of_range_values() {
        assert_fill_approx(choropleth_fill(Some(0.0), 1.0, 3.0), ACCENT_FILL);
        assert_fill_approx(choropleth_fill(Some(9.0), 1.0, 3.0), WHITE_FILL);
    }
}
