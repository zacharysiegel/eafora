// Only the `dom` submodule measures anything, and the ssr build does not compile it.
#![cfg_attr(not(feature = "hydrate"), allow(dead_code))]

use leptos::ev::{PointerEvent, WheelEvent};
use leptos::html::Div;
use leptos::prelude::*;

/// Keeps the thumb clear of the panel's corners at its fullest extent.
const TRACK_MARGIN: f64 = 6.0;

/// A thumb shorter than this cannot be grabbed, so a very long panel's thumb stops shrinking here and stops
/// being proportional to what it represents.
const MINIMUM_LENGTH: f64 = 24.0;

/// `clientHeight` and `scrollHeight` are each rounded to whole pixels, so a panel whose content fits can still
/// report a pixel of overflow once the browser's zoom makes the layout fractional.
const MINIMUM_OVERFLOW: f64 = 1.0;

/// Where the thumb sits and how long it is, in pixels down the panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbGeometry {
    top: f64,
    length: f64,
}

impl ThumbGeometry {
    fn style(&self) -> String {
        format!("top: {:.1}px; height: {:.1}px;", self.top, self.length)
    }
}

/// A pointer holding the thumb, and where along the thumb it took hold, so a drag moves the thumb with the
/// cursor rather than jumping the thumb's start to it.
#[derive(Debug, Clone, Copy)]
pub struct Grab {
    pointer_id: i32,
    offset_in_thumb: f64,
}

/// What one panel's thumb needs to know, owned by the panel it belongs to.
#[derive(Clone, Copy)]
pub struct ScrollThumbState {
    scroller: NodeRef<Div>,
    geometry: RwSignal<Option<ThumbGeometry>>,
    grab: RwSignal<Option<Grab>>,
}

impl ScrollThumbState {
    pub fn scroller(&self) -> NodeRef<Div> {
        self.scroller
    }
}

pub fn create_state() -> ScrollThumbState {
    ScrollThumbState {
        scroller: NodeRef::new(),
        geometry: RwSignal::new(None),
        grab: RwSignal::new(None),
    }
}

pub fn view(state: ScrollThumbState) -> impl IntoView {
    view! {
        {move || state.geometry.get().map(|geometry| view! {
            <div
                class="region-dock-thumb"
                class:is-held=move || state.grab.get().is_some()
                style=geometry.style()
                on:pointerdown=move |event: PointerEvent| take_hold(state, geometry, event)
                on:pointermove=move |event: PointerEvent| drag_to(state, geometry, event)
                on:pointerup=move |_| state.grab.set(None)
                on:pointercancel=move |_| state.grab.set(None)
                on:wheel=move |event: WheelEvent| scroll_by_wheel(state, event)
            ></div>
        })}
    }
}

/// `None` when the content fits, which is when a thumb would be reporting nothing.
fn geometry_for(visible_length: f64, content_length: f64, scroll_top: f64) -> Option<ThumbGeometry> {
    let scrollable: f64 = content_length - visible_length;
    if scrollable <= MINIMUM_OVERFLOW {
        return None;
    }

    let track: f64 = visible_length - 2.0 * TRACK_MARGIN;
    let length: f64 = (track * visible_length / content_length).max(MINIMUM_LENGTH);
    let travelled: f64 = (scroll_top / scrollable).clamp(0.0, 1.0);

    Some(ThumbGeometry {
        top: TRACK_MARGIN + travelled * (track - length),
        length,
    })
}

/// The scroll position a thumb dragged so its top lands at `requested_top` is asking for. `None` when the
/// thumb has no room to travel, which is when the content fits.
fn scroll_top_for(
    requested_top: f64,
    visible_length: f64,
    content_length: f64,
    thumb_length: f64,
) -> Option<f64> {
    let scrollable: f64 = content_length - visible_length;
    let travel: f64 = visible_length - 2.0 * TRACK_MARGIN - thumb_length;

    if scrollable <= MINIMUM_OVERFLOW || travel <= 0.0 {
        return None;
    }

    let travelled: f64 = ((requested_top - TRACK_MARGIN) / travel).clamp(0.0, 1.0);

    Some(travelled * scrollable)
}

#[cfg(feature = "hydrate")]
pub use dom::*;

#[cfg(feature = "hydrate")] // measures live elements
mod dom {
    use leptos::ev::{PointerEvent, WheelEvent};
    use leptos::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::Element;

    use super::{geometry_for, scroll_top_for, Grab, ScrollThumbState, ThumbGeometry};

    /// Recomputed on scroll and on the pointer entering the panel. Entering covers a viewport resize, since
    /// the thumb cannot be reached without it.
    pub fn refresh(state: ScrollThumbState) {
        let Some(scroller) = state.scroller().get()
        else {
            return;
        };

        let geometry: Option<ThumbGeometry> = geometry_for(
            scroller.client_height() as f64,
            scroller.scroll_height() as f64,
            scroller.scroll_top() as f64,
        );

        state.geometry.set(geometry);
    }

    /// Capturing the pointer keeps the drag alive once the cursor leaves the thumb, which it will: the thumb is
    /// a few pixels wide and the gesture is long.
    pub fn take_hold(state: ScrollThumbState, geometry: ThumbGeometry, event: PointerEvent) {
        let Some(scroller) = state.scroller().get()
        else {
            return;
        };

        let thumb_top_in_viewport: f64 = scroller.get_bounding_client_rect().top() + geometry.top;

        state.grab.set(Some(Grab {
            pointer_id: event.pointer_id(),
            offset_in_thumb: event.client_y() as f64 - thumb_top_in_viewport,
        }));

        let capture_target: Option<Element> = event.target().and_then(|target| target.dyn_into::<Element>().ok());
        if let Some(target) = capture_target {
            let _ = target.set_pointer_capture(event.pointer_id());
        }

        event.prevent_default();
    }

    /// The thumb sits over the panel's border rather than inside the scrolling element, so a wheel over it has
    /// nothing to scroll and the gesture is handed to the scroller the thumb represents.
    pub fn scroll_by_wheel(state: ScrollThumbState, event: WheelEvent) {
        let Some(scroller) = state.scroller().get()
        else {
            return;
        };

        scroller.set_scroll_top(scroller.scroll_top() + event.delta_y() as i32);

        event.prevent_default();
    }

    pub fn drag_to(state: ScrollThumbState, geometry: ThumbGeometry, event: PointerEvent) {
        let Some(grab) = state.grab.get()
        else {
            return;
        };

        if grab.pointer_id != event.pointer_id() {
            return;
        }

        let Some(scroller) = state.scroller().get()
        else {
            return;
        };

        let requested_top: f64 =
            event.client_y() as f64 - grab.offset_in_thumb - scroller.get_bounding_client_rect().top();
        let scroll_top: Option<f64> = scroll_top_for(
            requested_top,
            scroller.client_height() as f64,
            scroller.scroll_height() as f64,
            geometry.length,
        );

        if let Some(scroll_top) = scroll_top {
            scroller.set_scroll_top(scroll_top as i32);
        }
    }
}

#[cfg(not(feature = "hydrate"))]
pub use ssr::*;

#[cfg(not(feature = "hydrate"))] // the ssr build has no element to measure or drag
mod ssr {
    use leptos::ev::{PointerEvent, WheelEvent};

    use super::{ScrollThumbState, ThumbGeometry};

    pub fn refresh(_state: ScrollThumbState) {}

    pub fn scroll_by_wheel(_state: ScrollThumbState, _event: WheelEvent) {}

    pub fn take_hold(_state: ScrollThumbState, _geometry: ThumbGeometry, _event: PointerEvent) {}

    pub fn drag_to(_state: ScrollThumbState, _geometry: ThumbGeometry, _event: PointerEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_for_is_absent_when_the_content_fits() {
        assert_eq!(geometry_for(400.0, 400.0, 0.0), None);
        assert_eq!(geometry_for(400.0, 320.0, 0.0), None);
    }

    /// Browser zoom makes the layout fractional, and the two measurements round independently, so a panel that
    /// fits can report a pixel of overflow. A thumb then appears with nowhere to travel.
    #[test]
    fn geometry_for_treats_a_single_rounded_pixel_as_fitting() {
        assert_eq!(geometry_for(538.0, 539.0, 0.0), None);
        assert!(geometry_for(538.0, 540.0, 0.0).is_some());
    }

    #[test]
    fn geometry_for_starts_the_thumb_at_the_track_margin() {
        let geometry: ThumbGeometry = geometry_for(400.0, 1200.0, 0.0).unwrap();

        assert_eq!(geometry.top, TRACK_MARGIN);
    }

    /// The margin has to hold at both ends, or the thumb touches the panel's corner at one extreme.
    #[test]
    fn geometry_for_ends_the_thumb_a_margin_clear_of_the_bottom() {
        let visible_length: f64 = 400.0;
        let geometry: ThumbGeometry = geometry_for(visible_length, 1200.0, 800.0).unwrap();

        assert!((geometry.top + geometry.length - (visible_length - TRACK_MARGIN)).abs() < 1e-9);
    }

    #[test]
    fn geometry_for_sizes_the_thumb_to_the_visible_proportion() {
        let geometry: ThumbGeometry = geometry_for(400.0, 1600.0, 0.0).unwrap();

        let track: f64 = 400.0 - 2.0 * TRACK_MARGIN;
        assert!((geometry.length - track / 4.0).abs() < 1e-9);
    }

    #[test]
    fn geometry_for_stops_shrinking_the_thumb_at_the_grabbable_minimum() {
        let geometry: ThumbGeometry = geometry_for(400.0, 400_000.0, 0.0).unwrap();

        assert_eq!(geometry.length, MINIMUM_LENGTH);
    }

    #[test]
    fn scroll_top_for_maps_the_track_ends_to_the_content_ends() {
        let visible_length: f64 = 400.0;
        let content_length: f64 = 1200.0;
        let thumb_length: f64 = geometry_for(visible_length, content_length, 0.0).unwrap().length;

        let at_start: f64 =
            scroll_top_for(TRACK_MARGIN, visible_length, content_length, thumb_length).unwrap();
        let at_end: f64 = scroll_top_for(
            visible_length - TRACK_MARGIN - thumb_length,
            visible_length,
            content_length,
            thumb_length,
        )
        .unwrap();

        assert_eq!(at_start, 0.0);
        assert!((at_end - (content_length - visible_length)).abs() < 1e-9);
    }

    /// A drag past either end of the track asks for a position beyond the content, which has to clamp rather
    /// than overscroll.
    #[test]
    fn scroll_top_for_clamps_a_drag_past_the_track() {
        let thumb_length: f64 = geometry_for(400.0, 1200.0, 0.0).unwrap().length;

        assert_eq!(scroll_top_for(-500.0, 400.0, 1200.0, thumb_length), Some(0.0));
        assert_eq!(scroll_top_for(5000.0, 400.0, 1200.0, thumb_length), Some(800.0));
    }

    #[test]
    fn scroll_top_for_is_absent_when_the_content_fits() {
        assert_eq!(scroll_top_for(20.0, 400.0, 400.0, 24.0), None);
    }
}
