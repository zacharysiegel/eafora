use shared::map::SurfacePoint;

/// Left mouse button / primary contact. Right and middle clicks are not map gestures.
const PRIMARY_POINTER_BUTTON: i16 = 0;

/// A pointer in contact (a held mouse button or a touching finger), tracked by `pointerId` so pan and
/// pinch can follow the right one across moves.
#[derive(Debug, Clone, Copy)]
pub struct PointerState {
    pub pointer_id: i32,
    pub position: SurfacePoint,
}

/// The in-progress pointer gesture. Each variant owns exactly the state its stage needs, so combinations
/// like a press origin with no pointer down cannot arise. Only `Tap` selects on release; `Pan` and
/// `Pinch` do not.
#[derive(Debug, Clone, Copy)]
pub enum Gesture {
    Idle,
    /// One pointer down, not yet past the tap threshold. `origin` is where it pressed, for that test.
    Tap { pointer: PointerState, origin: SurfacePoint },
    /// One pointer dragged past the threshold: panning.
    Pan { pointer: PointerState },
    /// Two pointers down: pinch-zoom. A third finger is ignored, not tracked.
    Pinch { first: PointerState, second: PointerState },
}

/// Whether releasing a pointer completed a tap (so the caller selects) or ended a pan/pinch (so it does
/// not).
pub enum PointerRelease {
    Tap,
    NoSelect,
}

impl Gesture {
    pub fn is_active(&self) -> bool {
        !matches!(self, Gesture::Idle)
    }

    /// Transitions on a newly-pressed pointer: the first press is a tap candidate; a second distinct
    /// pointer turns the gesture into a pinch; a third while pinching is ignored. A second press of the
    /// same `pointer_id` is not a second contact (the browser dropped the matching release, e.g. a
    /// right-click context menu); it restarts as a tap so one pointer cannot pinch against itself.
    pub fn begin(&mut self, pointer_id: i32, position: SurfacePoint) {
        let pointer: PointerState = PointerState { pointer_id, position };

        *self = match *self {
            Gesture::Idle => Gesture::Tap { pointer, origin: position },
            Gesture::Tap { pointer: existing, .. } | Gesture::Pan { pointer: existing }
                if existing.pointer_id == pointer_id =>
            {
                Gesture::Tap { pointer, origin: position }
            },
            Gesture::Tap { pointer: existing, .. } | Gesture::Pan { pointer: existing } => {
                Gesture::Pinch { first: existing, second: pointer }
            },
            Gesture::Pinch { first, second } => {
                if first.pointer_id == pointer_id {
                    Gesture::Pinch { first: pointer, second }
                } else if second.pointer_id == pointer_id {
                    Gesture::Pinch { first, second: pointer }
                } else {
                    Gesture::Pinch { first, second }
                }
            },
        };
    }

    /// Transitions on a released or canceled pointer: a single-pointer gesture ends, a pinch collapses to
    /// a pan of the remaining pointer (no jump, since that pointer keeps its position). Reports whether a
    /// tap completed so the caller can decide to select; a pan or pinch reports `NoSelect`.
    pub fn release(&mut self, pointer_id: i32) -> PointerRelease {
        match *self {
            Gesture::Tap { pointer, .. } if pointer.pointer_id == pointer_id => {
                *self = Gesture::Idle;
                PointerRelease::Tap
            },
            Gesture::Pan { pointer } if pointer.pointer_id == pointer_id => {
                *self = Gesture::Idle;
                PointerRelease::NoSelect
            },
            Gesture::Pinch { first, second } if first.pointer_id == pointer_id || second.pointer_id == pointer_id => {
                let remaining: PointerState = if first.pointer_id == pointer_id { second } else { first };
                *self = Gesture::Pan { pointer: remaining };
                PointerRelease::NoSelect
            },
            _ => PointerRelease::NoSelect,
        }
    }

    pub fn clear(&mut self) {
        *self = Gesture::Idle;
    }

}

pub fn is_map_gesture_button(button: i16) -> bool {
    button == PRIMARY_POINTER_BUTTON
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64) -> SurfacePoint {
        SurfacePoint { x, y }
    }

    #[test]
    fn begin_same_pointer_id_does_not_become_pinch() {
        let mut gesture: Gesture = Gesture::Idle;

        gesture.begin(1, point(10.0, 10.0));
        gesture.begin(1, point(20.0, 20.0));

        assert!(matches!(
            gesture,
            Gesture::Tap { pointer, .. } if pointer.pointer_id == 1
        ));
    }

    #[test]
    fn begin_second_pointer_becomes_pinch() {
        let mut gesture: Gesture = Gesture::Idle;

        gesture.begin(1, point(10.0, 10.0));
        gesture.begin(2, point(30.0, 30.0));

        assert!(matches!(
            gesture,
            Gesture::Pinch { first, second } if first.pointer_id == 1 && second.pointer_id == 2
        ));
    }

    #[test]
    fn is_map_gesture_button_accepts_primary_only() {
        assert!(is_map_gesture_button(0));
        assert!(!is_map_gesture_button(1));
        assert!(!is_map_gesture_button(2));
    }

    #[test]
    fn clear_returns_to_idle() {
        let mut gesture: Gesture = Gesture::Idle;

        gesture.begin(1, point(10.0, 10.0));
        gesture.clear();

        assert!(matches!(gesture, Gesture::Idle));
    }

}
