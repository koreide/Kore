//! Custom scrollbars (spec 01 §0.2): 6px, transparent track, thumb `#334155`
//! radius 3 (hover `#475569`). They take layout space; callers reserve [`WIDTH`] when [`Gutter::overflows`].
//!
//! The thumb is painted by a canvas placed *after* the scrolled element, so it
//! uses the current frame's scroll metrics. Dragging the thumb scrolls.

use crate::theme;
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;

pub const WIDTH: f32 = 6.;

/// Tracks whether the content overflowed in the last painted frame, so the
/// owner can reserve the gutter (re-renders once when it flips).
#[derive(Clone, Default)]
pub struct Gutter {
    rendered: Rc<Cell<bool>>,
    measured: Rc<Cell<bool>>,
    drag: Rc<Cell<Option<(Pixels, Pixels)>>>,
}

impl Gutter {
    pub fn overflows(&self) -> bool {
        let v = self.measured.get();
        self.rendered.set(v);
        v
    }
}

/// Vertical thumb overlay for `handle`. Place it as an absolutely positioned
/// sibling covering the scroll viewport (`absolute().inset_0()` parent).
pub fn vertical(handle: ScrollHandle, gutter: Gutter) -> impl IntoElement {
    let g0 = gutter.clone();
    let h0 = handle.clone();
    canvas(
        move |bounds, window, _| {
            let viewport = h0.bounds();
            let max = h0.max_offset();
            let overflow = max.y > px(0.5);
            g0.measured.set(overflow);
            if g0.rendered.get() != overflow {
                window.refresh();
            }
            if !overflow || viewport.size.height <= px(0.) {
                return None;
            }
            let vh = viewport.size.height;
            let content = vh + max.y;
            let thumb_h = (vh * (vh / content)).max(px(20.));
            let off = -h0.offset().y;
            let track = vh - thumb_h;
            let y = if max.y > px(0.) { track * (off / max.y) } else { px(0.) };
            let x = bounds.right() - px(WIDTH);
            let thumb = Bounds::new(point(x, viewport.top() + y), size(px(WIDTH), thumb_h));
            let hitbox = window.insert_hitbox(thumb, HitboxBehavior::Normal);
            Some((thumb, hitbox, track, max.y))
        },
        move |_, state, window, _| {
            let Some((thumb, hitbox, track, max_y)) = state else { return };
            let hovered = hitbox.is_hovered(window) || gutter.drag.get().is_some();
            let color = if hovered { theme::hex(0x475569) } else { theme::hex(0x334155) };
            window.paint_quad(fill(thumb, color).corner_radii(px(3.)));

            let (g, h) = (gutter.clone(), handle.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && e.button == MouseButton::Left && hitbox.is_hovered(window) {
                    g.drag.set(Some((e.position.y, -h.offset().y)));
                    cx.stop_propagation();
                }
            });
            let (g, h) = (gutter.clone(), handle.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, _| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                if let Some((start_y, start_off)) = g.drag.get() {
                    if e.pressed_button != Some(MouseButton::Left) {
                        g.drag.set(None);
                        return;
                    }
                    let delta = e.position.y - start_y;
                    let scale = if track > px(0.) { max_y / track } else { 0. };
                    let next = (start_off + delta * scale).clamp(px(0.), max_y);
                    h.set_offset(point(h.offset().x, -next));
                    window.refresh();
                }
            });
            let g = gutter.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, _, _, _| g.drag.set(None));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
