use crate::map::{SurfaceDimensions, Viewport};
use crate::math;

/// An animated transition between two viewports, interpolated in center-and-height space by
/// `Viewport::interpolate_to`. Does no scheduling and reads the clock only through the `now_ms` argument
/// to `sample`.
#[derive(Debug, Clone, Copy)]
pub struct ViewportTransition {
    from: Viewport,
    target: Viewport,
    /// When the transition began, on the same clock the caller passes to `sample`.
    start_time_ms: f64,
    duration_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationProgress {
    Animating,
    Finished,
}

impl ViewportTransition {
    pub fn new(from: Viewport, target: Viewport, start_time_ms: f64, duration_ms: f64) -> ViewportTransition {
        ViewportTransition { from, target, start_time_ms, duration_ms }
    }

    /// The interpolated viewport at `now_ms` and whether the transition has finished. At or past the end
    /// (or for a non-positive duration) it returns `target` verbatim with `Finished`, so the final frame
    /// is bit-identical to the caller's clamped target and a tab backgrounded mid-transition snaps to the
    /// end on its first frame back.
    pub fn sample(&self, now_ms: f64, surface: SurfaceDimensions) -> (Viewport, AnimationProgress) {
        let elapsed_ms: f64 = now_ms - self.start_time_ms;

        if self.duration_ms <= 0.0 || elapsed_ms >= self.duration_ms {
            return (self.target, AnimationProgress::Finished);
        }

        let eased_t: f64 = math::cubic_ease_in_out((elapsed_ms / self.duration_ms).clamp(0.0, 1.0));

        (self.from.interpolate_to(self.target, eased_t, surface), AnimationProgress::Animating)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::ProjectedPoint;

    const SURFACE: SurfaceDimensions = SurfaceDimensions { width: 100, height: 100 };

    // Aspect 1 (matching SURFACE); from center (0, 0) height 2, target center (2, 1.75) height 0.5.
    fn from_viewport() -> Viewport {
        Viewport { min: ProjectedPoint { x: -1.0, y: -1.0 }, max: ProjectedPoint { x: 1.0, y: 1.0 } }
    }

    fn target_viewport() -> Viewport {
        Viewport { min: ProjectedPoint { x: 1.75, y: 1.5 }, max: ProjectedPoint { x: 2.25, y: 2.0 } }
    }

    #[test]
    fn sample_at_start_returns_the_from_view_and_animating() {
        let transition: ViewportTransition = ViewportTransition::new(from_viewport(), target_viewport(), 1000.0, 600.0);
        let (viewport, progress): (Viewport, AnimationProgress) = transition.sample(1000.0, SURFACE);

        assert_eq!(progress, AnimationProgress::Animating);
        assert!(((viewport.min.x + viewport.max.x) / 2.0).abs() < 1e-9, "center x is the from center");
        assert!((viewport.max.y - viewport.min.y - 2.0).abs() < 1e-9, "height is the from height");
    }

    #[test]
    fn sample_at_and_past_the_end_returns_the_target_verbatim_and_finished() {
        let transition: ViewportTransition = ViewportTransition::new(from_viewport(), target_viewport(), 1000.0, 600.0);

        assert_eq!(transition.sample(1600.0, SURFACE), (target_viewport(), AnimationProgress::Finished));
        // Far past the end (a backgrounded-then-foregrounded tab) snaps to the target.
        assert_eq!(transition.sample(999_999.0, SURFACE), (target_viewport(), AnimationProgress::Finished));
    }

    #[test]
    fn sample_with_non_positive_duration_finishes_immediately() {
        let transition: ViewportTransition = ViewportTransition::new(from_viewport(), target_viewport(), 1000.0, 0.0);

        assert_eq!(transition.sample(1000.0, SURFACE), (target_viewport(), AnimationProgress::Finished));
    }

    #[test]
    fn sample_midway_lies_between_the_endpoints_at_the_surface_aspect() {
        let transition: ViewportTransition = ViewportTransition::new(from_viewport(), target_viewport(), 1000.0, 600.0);
        let (viewport, progress): (Viewport, AnimationProgress) = transition.sample(1300.0, SURFACE);
        let center_x: f64 = (viewport.min.x + viewport.max.x) / 2.0;
        let height: f64 = viewport.max.y - viewport.min.y;

        assert_eq!(progress, AnimationProgress::Animating);
        assert!(center_x > 0.0 && center_x < 2.0, "center x between the endpoints");
        assert!(height < 2.0 && height > 0.5, "height between the endpoints");
        assert!(((viewport.max.x - viewport.min.x) / height - 1.0).abs() < 1e-9, "surface aspect held");
    }
}
