//! Small stateless primitives: Kbd, status badge, spinner, skeleton bar,
//! restart sparkline.

use crate::state::resource_item::{status_variant, StatusVariant};
use crate::theme::{self, amber, emerald, misc, red, slate, KoreStyled};
use crate::ui::icon::icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;

/// Header/footer `Kbd` (spec 01 §6): px 6 py 2, bg slate-800/80, radius 4,
/// 10px mono slate-400, 1px border slate-700/50.
pub fn kbd(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(2.))
        .bg(theme::hexa(0x1e293b, 0.8))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::hexa(0x334155, 0.5))
        .text_10()
        // An inline <kbd>: padding does not grow the line, the box is the
        // font's content area (~1.2em) plus padding and border.
        .line_height(px(12.))
        .font_mono()
        .text_color(slate::S400)
        .child(text.into())
}

/// Status badge colors: (dot/text, text, bg, pulse).
pub fn status_colors(status: Option<&str>) -> (Rgba, Rgba, Rgba, bool) {
    match status_variant(status) {
        StatusVariant::Running => (emerald::E400, emerald::E400, theme::hexa(0x10b981, 0.1), true),
        StatusVariant::Pending => (amber::A400, amber::A400, theme::hexa(0xf59e0b, 0.1), false),
        StatusVariant::Failed => (red::R400, red::R400, theme::hexa(0xef4444, 0.1), false),
        StatusVariant::Terminating => (misc::ORANGE400, misc::ORANGE400, theme::hexa(0xf97316, 0.1), false),
        StatusVariant::Default => (slate::S400, slate::S300, theme::hexa(0x64748b, 0.1), false),
    }
}

/// Status badge (spec 01 §7.7): pill, 6px dot (+ping halo for running), 12px medium.
pub fn status_badge(id: impl Into<ElementId>, status: Option<&str>) -> Div {
    status_badge_impl(id, status, false)
}

/// Status badge inside mono text (table cells): the label is drawn at the 0.601em
/// SF Mono pitch (see `ui::mono_text`).
pub fn status_badge_mono(id: impl Into<ElementId>, status: Option<&str>) -> Div {
    status_badge_impl(id, status, true)
}

fn status_badge_impl(id: impl Into<ElementId>, status: Option<&str>, mono: bool) -> Div {
    let (dot, text, bg, pulse) = status_colors(status);
    let label: SharedString = status.unwrap_or("-").to_string().into();
    let label: AnyElement = if mono {
        crate::ui::mono_text::mono_text(label).into_any_element()
    } else {
        label.into_any_element()
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(2.))
        .rounded_full()
        .bg(bg)
        .text_xs_tw()
        .font_weight(FontWeight::MEDIUM)
        .text_color(text)
        .child(
            div()
                .relative()
                .size(px(6.))
                .flex_none()
                .when(pulse, |d| d.child(ping(id, dot)))
                .child(div().absolute().inset_0().rounded_full().bg(dot)),
        )
        .child(label)
}

/// `animate-ping` halo: scale 1→2, opacity .75→0 over the last quarter of 1s.
pub fn ping(id: impl Into<ElementId>, color: Rgba) -> impl IntoElement {
    div().absolute().inset_0().with_animation(
        id,
        Animation::new(Duration::from_millis(1000)).repeat(),
        move |el, t| {
            // keyframes: 75%,100% { transform: scale(2); opacity: 0 } from opacity .75 scale 1.
            let p = (t / 0.75).min(1.0);
            let e = ease_out_cubic(p);
            let scale = 1.0 + e;
            let opacity = 0.75 * (1.0 - e);
            let grow = 3.0 * (scale - 1.0);
            el.child(
                div()
                    .absolute()
                    .top(px(-grow))
                    .left(px(-grow))
                    .size(px(6.0 * scale))
                    .rounded_full()
                    .bg(Rgba { a: opacity, ..color }),
            )
        },
    )
}

fn ease_out_cubic(t: f32) -> f32 {
    // cubic-bezier(0,0,0.2,1) ≈ ease-out
    1.0 - (1.0 - t).powi(3)
}

/// Spinner: 2px ring with an accent arc, rotating 1s linear (`animate-spin`).
pub fn spinner(id: impl Into<ElementId>, size: f32, ring: Rgba, arc: Rgba) -> impl IntoElement {
    div()
        .relative()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .border_2()
        .border_color(ring)
        .child(
            svg()
                .path("kore/arc.svg")
                .absolute()
                .top(px(-2.))
                .left(px(-2.))
                .size(px(size))
                .text_color(arc)
                .with_animation(id, Animation::new(Duration::from_millis(1000)).repeat(), |s, t| {
                    s.with_transformation(Transformation::rotate(percentage(t)))
                }),
        )
}

/// Spinning lucide icon (`animate-spin` on e.g. `refresh-cw`, `loader-2`).
pub fn spinning_icon(id: impl Into<ElementId>, name: &str, size: f32, color: Rgba) -> impl IntoElement {
    icon(name, size)
        .text_color(color)
        .with_animation(id, Animation::new(Duration::from_millis(1000)).repeat(), |s, t| {
            s.with_transformation(Transformation::rotate(percentage(t)))
        })
}

/// Skeleton shimmer bar (spec 01 §0.2): moving 3-stop gradient, 1.8s.
pub fn skeleton_bar(id: impl Into<ElementId>, width: DefiniteLength, height: f32) -> impl IntoElement {
    let dark = theme::hexa(0x1e293b, 0.4);
    let light = theme::hexa(0x334155, 0.4);
    div()
        .w(width)
        .h(px(height))
        .rounded(px(4.))
        .overflow_hidden()
        .bg(dark)
        .with_animation(id, Animation::new(Duration::from_millis(1800)).repeat().with_easing(ease_in_out), move |el, t| {
            // background-size 200%: the light band (at 50% of a 2x-wide gradient)
            // sweeps across; approximate with a translucent band.
            let pos = -1.0 + 2.0 * t; // -1 → 1 (fraction of width)
            el.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(pos))
                    .w(relative(1.0))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(dark, 0.0),
                        linear_color_stop(light, 0.5),
                    ))
                    .child(div()),
            )
        })
}

/// RestartSparkline (spec 01 §7.10). `data` = per-point restart counts.
pub fn restart_sparkline(data: &[i64]) -> AnyElement {
    let latest = data.last().copied().unwrap_or(0);
    let all_zero = data.iter().all(|c| *c == 0);
    if data.len() < 2 || all_zero {
        return div()
            .font_mono()
            .text_xs_tw()
            .text_color(if latest == 0 { slate::S500 } else { amber::A500 })
            .child(latest.to_string())
            .into_any_element();
    }
    let prev = data[data.len() - 2];
    let color = if latest == 0 {
        slate::S500
    } else if latest > prev {
        red::R500
    } else {
        amber::A500
    };
    let (w, h, pad) = (60.0f32, 16.0f32, 1.0f32);
    let min = *data.iter().min().unwrap_or(&0) as f32;
    let max = *data.iter().max().unwrap_or(&0) as f32;
    let range = if max - min == 0.0 { 1.0 } else { max - min };
    let n = data.len();
    let pts: Vec<(f32, f32)> = data
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let x = pad + (i as f32 / (n - 1) as f32) * (w - 2.0 * pad);
            let y = pad + (h - 2.0 * pad) - ((*c as f32 - min) / range) * (h - 2.0 * pad);
            (x, y)
        })
        .collect();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let mut b = PathBuilder::stroke(px(1.5));
                    for (i, (x, y)) in pts.iter().enumerate() {
                        let p = point(bounds.origin.x + px(*x), bounds.origin.y + px(*y));
                        if i == 0 {
                            b.move_to(p);
                        } else {
                            b.line_to(p);
                        }
                    }
                    if let Ok(path) = b.build() {
                        window.paint_path(path, color);
                    }
                },
            )
            .w(px(w))
            .h(px(h))
            .flex_none(),
        )
        .child(div().font_mono().text_xs_tw().line_height(relative(1.)).text_color(color).child(latest.to_string()))
        .into_any_element()
}
