//! ConfirmDialog (spec 01 §10). Driven by `AppState::confirm`; any view can
//! open one with `app.update(cx, |s, cx| s.confirm(ConfirmRequest { … }, cx))`.
//!
//! Solid surface panel, red 50% border, enter animation only. Esc (root) and
//! a click on the backdrop cancel it, unless the confirm action is running.

use crate::automation::Automation;
use crate::state::app_state::{ConfirmRequest, ConfirmVariant};
use crate::state::AppState;
use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::icon::icon;
use gpui_kit::*;
use std::time::Duration;

pub fn render(app: &Entity<AppState>, req: &ConfirmRequest) -> impl IntoElement {
    let (border, icon_bg, icon_color, btn_bg, btn_hover, btn_border) = match req.variant {
        ConfirmVariant::Danger => (
            theme::hexa(0xef4444, 0.5),
            theme::hexa(0xef4444, 0.2),
            red::R400,
            red::R600,
            red::R700,
            red::R500,
        ),
        ConfirmVariant::Warning => (
            theme::hexa(0xeab308, 0.5),
            theme::hexa(0xeab308, 0.2),
            theme::hex(0xfacc15),
            theme::hex(0xca8a04),
            theme::hex(0xa16207),
            theme::hex(0xeab308),
        ),
        ConfirmVariant::Info => (
            theme::hexa(0x3b82f6, 0.5),
            theme::hexa(0x3b82f6, 0.2),
            theme::hex(0x60a5fa),
            theme::hex(0x2563eb),
            theme::hex(0x1d4ed8),
            theme::hex(0x3b82f6),
        ),
    };
    let on_confirm = req.on_confirm.clone();
    let app_ok = app.clone();
    let app_backdrop = app.clone();
    let app_cancel = app.clone();
    let app_x = app.clone();
    let panel = div()
        .w_full()
        .max_w(px(448.))
        // Rendered CSS order: `bg-yellow-500/10` beats `bg-surface` for warning
        // (translucent panel, spec 03 §0.5); danger/info stay opaque surface.
        // The design shows the 4px-blurred page through it; without backdrop blur
        // we use the composite over the black/60 backdrop on the app background
        // (#0b1221) so underlying text does not bleed through.
        .bg(if req.variant == ConfirmVariant::Warning { theme::hex(0x1b180d) } else { theme::SURFACE })
        .border_1()
        .border_color(border)
        .rounded(px(8.))
        // Occlude so a press on the panel never reaches the backdrop's close handler.
        .id("confirm-panel")
        .occlude()
        .shadow(vec![
            BoxShadow {
                color: theme::hexa(0, 0.1).into(),
                offset: point(px(0.), px(20.)),
                blur_radius: px(25.),
                spread_radius: px(-5.),
                inset: false,
            },
            BoxShadow {
                color: theme::hexa(0, 0.1).into(),
                offset: point(px(0.), px(8.)),
                blur_radius: px(10.),
                spread_radius: px(-6.),
                inset: false,
            },
        ])
        .p(px(24.))
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(16.))
                .mb(px(16.))
                .child(
                    div()
                        .p(px(8.))
                        .rounded_full()
                        .bg(icon_bg)
                        .flex_none()
                        .child(icon("alert-triangle", 24.).text_color(icon_color)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_lg_tw()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(slate::S100)
                                .mb(px(8.))
                                .child(req.title.clone()),
                        )
                        .child(
                            div()
                                .text_sm_tw()
                                .text_color(slate::S300)
                                .child(crate::ui::text::prose_text(req.message.clone()))
                                .auto_id("confirm.message", "text", req.message.clone()),
                        ),
                )
                .child(
                    div()
                        .id("confirm-x")
                        .flex_none()
                        .cursor_pointer()
                        .group("confirm-x")
                        .child(icon("x", 20.).text_color(slate::S400).group_hover("confirm-x", |s| s.text_color(slate::S200)))
                        .on_click(move |_, _, cx| app_x.update(cx, |s, cx| s.close_confirm(cx)))
                        .auto_id("confirm.close", "button", "Close"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap(px(12.))
                .mt(px(24.))
                .child(
                    div()
                        .id("confirm-cancel")
                        .px(px(16.))
                        .py(px(8.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(slate::S800)
                        .text_sm_tw()
                        .text_color(slate::S100)
                        .cursor_pointer()
                        .hover(|s| s.border_color(slate::S700).bg(theme::hexa(0x1d2939, 0.4)))
                        .child(req.cancel_label.clone())
                        .on_click(move |_, _, cx| app_cancel.update(cx, |s, cx| s.close_confirm(cx)))
                        .auto_id("confirm.cancel", "button", req.cancel_label.clone()),
                )
                .child(
                    div()
                        .id("confirm-ok")
                        .px(px(16.))
                        .py(px(8.))
                        .rounded(px(4.))
                        .bg(btn_bg)
                        .border_1()
                        .border_color(btn_border)
                        .text_sm_tw()
                        .text_color(theme::hex(0xffffff))
                        .cursor_pointer()
                        .hover(move |s| s.bg(btn_hover))
                        .child(req.confirm_label.clone())
                        .on_click(move |_, window, cx| {
                            app_ok.update(cx, |s, _| s.confirm_busy = true);
                            on_confirm(window, cx)
                        })
                        .auto_id("confirm.ok", "button", req.confirm_label.clone()),
                ),
        )
        .with_animation(
            "confirm-in",
            Animation::new(Duration::from_millis(250)).with_easing(ease_out_quint()),
            |el, t| el.opacity(t).mt(px(20. * (1. - t))),
        );

    div()
        .id("confirm-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .bg(theme::hexa(0, 0.6))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            app_backdrop.update(cx, |s, cx| {
                s.dismiss_confirm(cx);
            });
        })
        .flex()
        .items_center()
        .justify_center()
        .p(px(16.))
        .child(panel)
        .auto_id("confirm", "dialog", req.title.clone())
}
