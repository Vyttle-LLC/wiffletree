//! A press that wobbles a pixel or two before its release is a click, as elsewhere on macOS.
//!
//! The window's text selection takes any movement under a held press as a drag: one pixel
//! selects a sliver of text. A link in a message opens only on a release with nothing selected,
//! so a click whose hand moved at all, as a slow or trackpad click usually does, opened nothing.
use gpui::*;
use std::{cell::Cell, rc::Rc};

/// How far, in pixels, a held press may move and still be a click.
const SLOP: f64 = 4.;

/// Where the held left press went down, while it is still a click.
#[derive(Clone, Default)]
pub(super) struct ClickSlop(Rc<Cell<Option<Point<Pixels>>>>);

impl ClickSlop {
    /// Holds back the pointer's moves under a left press until they leave [`SLOP`] of where it
    /// went down; from there on it is a drag and every move passes. Paint it once per window,
    /// before its content; it draws nothing.
    pub(super) fn element(&self) -> impl IntoElement {
        let pressed_at = self.0.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let down = pressed_at.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, _| {
                    if phase.capture() {
                        down.set((event.button == MouseButton::Left).then_some(event.position));
                    }
                });
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    let Some(at) = pressed_at.get().filter(|_| phase.capture()) else {
                        return;
                    };
                    let held = event.pressed_button == Some(MouseButton::Left);
                    if held && (event.position - at).magnitude() <= SLOP {
                        cx.stop_propagation();
                    } else {
                        pressed_at.set(None);
                    }
                });
            },
        )
        .absolute()
    }
}
