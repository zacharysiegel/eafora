//! Choropleth fill color: a continuous single-hue lerp between the accent red at the statistic's
//! minimum and white at its maximum, per `docs/design/README.md` §Map. No data renders white.

/// The saturated accent (`#e60019`), mapped to the statistic's minimum value.
const ACCENT_FILL: [f32; 4] = [230.0 / 255.0, 0.0, 25.0 / 255.0, 1.0];

/// The white base (`#fff`), mapped to the statistic's maximum value and to no data.
const WHITE_FILL: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// RGBA fill for one country. `None` (no value at the active period) is white; otherwise a
/// continuous lerp with the accent at `statistic_min` and white at `statistic_max` — the direction
/// the TFR design uses, where the most-saturated red marks the lowest value.
pub fn choropleth_fill(value: Option<f64>, statistic_min: f64, statistic_max: f64) -> [f32; 4] {
    let Some(value) = value else {
        return WHITE_FILL;
    };

    let range: f64 = statistic_max - statistic_min;
    let normalized: f64 = if range == 0.0 {
        0.0
    } else {
        ((value - statistic_min) / range).clamp(0.0, 1.0)
    };

    lerp(ACCENT_FILL, WHITE_FILL, normalized as f32)
}

fn lerp(from: [f32; 4], to: [f32; 4], t: f32) -> [f32; 4] {
    [
        from[0] + t * (to[0] - from[0]),
        from[1] + t * (to[1] - from[1]),
        from[2] + t * (to[2] - from[2]),
        from[3] + t * (to[3] - from[3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 1e-6;

    fn assert_fill_approx(actual: [f32; 4], expected: [f32; 4]) {
        for channel in 0..4 {
            assert!(
                (actual[channel] - expected[channel]).abs() < TOLERANCE,
                "channel {channel}: {} vs {}",
                actual[channel],
                expected[channel],
            );
        }
    }

    #[test]
    fn choropleth_fill_maps_none_to_white() {
        assert_fill_approx(choropleth_fill(None, 1.0, 3.0), WHITE_FILL);
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
        let midpoint_fill: [f32; 4] = choropleth_fill(Some(2.0), 1.0, 3.0);
        let expected: [f32; 4] = [
            (ACCENT_FILL[0] + WHITE_FILL[0]) / 2.0,
            (ACCENT_FILL[1] + WHITE_FILL[1]) / 2.0,
            (ACCENT_FILL[2] + WHITE_FILL[2]) / 2.0,
            1.0,
        ];
        assert_fill_approx(midpoint_fill, expected);
    }

    #[test]
    fn choropleth_fill_clamps_out_of_range_values() {
        assert_fill_approx(choropleth_fill(Some(0.0), 1.0, 3.0), ACCENT_FILL);
        assert_fill_approx(choropleth_fill(Some(9.0), 1.0, 3.0), WHITE_FILL);
    }
}
