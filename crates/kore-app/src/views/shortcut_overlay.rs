//! Keyboard shortcut overlay (spec 01 §15). Shown while
//! `AppState::shortcuts_open`; `?`/Esc/backdrop close it (root handles keys).

use crate::automation::Automation;
use crate::state::AppState;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::text::caps;
use gpui_kit::*;

type Shortcut = (&'static str, &'static [&'static str]);

const GROUPS: [(&str, &[Shortcut]); 4] = [
    (
        "Navigation",
        &[
            ("Move down", &["j"]),
            ("Move up", &["k"]),
            ("Enter detail view", &["l"]),
            ("Go back", &["h"]),
            ("Switch tabs", &["1", "2", "3", "4", "5"]),
        ],
    ),
    (
        "Actions",
        &[
            ("Delete resource", &["d"]),
            ("Command palette", &[crate::keys::MOD, "K"]),
            ("Refresh resources", &[crate::keys::MOD, "R"]),
            ("Search / filter", &["/"]),
            ("Search logs", &[crate::keys::MOD, "F"]),
        ],
    ),
    (
        "RBAC",
        &[
            ("Open RBAC view", &[crate::keys::MOD, crate::keys::SHIFT, "R"]),
            ("Toggle impersonation", &["i"]),
            ("Switch RBAC tabs", &["1", "2", "3", "4"]),
        ],
    ),
    (
        "General",
        &[("Close / go back", &["Esc"]), ("Select / confirm", &["Enter"]), ("Toggle this overlay", &["?"])],
    ),
];

pub struct ShortcutOverlay {
    app: Entity<AppState>,
}

impl ShortcutOverlay {
    pub fn new(app: Entity<AppState>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        ShortcutOverlay { app }
    }
}

/// Overlay Kbd: min-w 24, h 24, px 6, bg slate-800/80, radius 4, 11px mono slate-300.
fn kbd(text: &str, size: f32) -> Div {
    div()
        .flex_none()
        .min_w(px(24.))
        .h(px(24.))
        .px(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::hexa(0x1e293b, 0.8))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::hexa(0x334155, 0.5))
        .shadow(vec![BoxShadow {
            color: theme::hexa(0x334155, 0.5).into(),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: false,
        }])
        .font_mono()
        .text_size(px(size))
        .text_color(slate::S300)
        .child(text.to_string())
}

fn group(title: &str, items: &[Shortcut], cx: &App) -> impl IntoElement {
    let _ = cx;
    div()
        // Equal grid columns (`grid-cols-2`): basis 0, allowed to shrink.
        .flex_1()
        .min_w_0()
        .p(px(16.))
        .child(div().mb(px(8.)).text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S100).child(caps(title, 10.)))
        .child(div().flex().flex_col().gap(px(2.)).children(items.iter().map(|(desc, keys)| {
            div()
                .id(SharedString::from(format!("sc-{desc}")))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .py(px(6.))
                .px(px(8.))
                .rounded(px(4.))
                .hover(|s| s.bg(theme::hexa(0xffffff, 0.02)))
                .child(div().min_w_0().text_xs_tw().text_color(slate::S400).child(desc.to_string()))
                .child(div().flex_none().ml(px(16.)).flex().flex_row().items_center().gap(px(2.)).children(keys.iter().map(|k| kbd(k, 11.))))
                .auto_id(format!("shortcuts.item.{desc}"), "listitem", keys.join(" "))
        })))
        .auto_id(format!("shortcuts.group.{}", title.to_lowercase()), "group", title.to_string())
}

fn footer_text(t: &'static str) -> Div {
    div().relative().top(px(-0.5)).child(t)
}

impl Render for ShortcutOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top = window.viewport_size().height * 0.15;
        let accent = theme::accent(cx);
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(20.))
            .py(px(14.))
            .border_b_1()
            .border_color(theme::hexa(0x1e293b, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(div().size(px(28.)).rounded(px(8.)).flex().items_center().justify_center().child(icon("keyboard", 16.).text_color(accent)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("Keyboard Shortcuts"))
                            .child(div().text_10().text_color(slate::S500).child("Quick reference for all shortcuts")),
                    ),
            )
            .child(
                div()
                    .id("shortcuts-close")
                    .group("shortcuts-close")
                    .p(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hexa(0x1e293b, 0.5)))
                    .child(icon("x", 16.).text_color(slate::S400).group_hover("shortcuts-close", |s| s.text_color(slate::S200)))
                    .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |a, cx| a.set_shortcuts_open(false, cx))))
                    .auto_id("shortcuts.close", "button", "Close"),
            );
        let body = div()
            .flex()
            .flex_col()
            .bg(theme::hexa(0x1e293b, 0.3))
            .child(div().flex().flex_row().child(group(GROUPS[0].0, GROUPS[0].1, cx)).child(group(GROUPS[1].0, GROUPS[1].1, cx)))
            .child(div().flex().flex_row().mt(px(1.)).child(group(GROUPS[2].0, GROUPS[2].1, cx)).child(group(GROUPS[3].0, GROUPS[3].1, cx)));
        let footer = div()
            .px(px(20.))
            .py(px(10.))
            .border_t_1()
            .border_color(theme::hexa(0x1e293b, 0.5))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .text_10()
                    .text_color(slate::S600)
                    // The design puts the 15px text line half a pixel higher inside
                    // the 24px kbd row than GPUI's centering does.
                    .child(footer_text("Press"))
                    .child(kbd("?", 9.))
                    .child(footer_text("or"))
                    .child(kbd("Esc", 9.))
                    .child(footer_text("to close")),
            );
        // Deferred: painted after the whole window so no underlying text
        // (e.g. table cells) can end up above the backdrop.
        deferred(
        div()
            .id("shortcuts-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme::hexa(0, crate::views::command_palette::BACKDROP_ALPHA))
            .flex()
            .flex_col()
            .items_center()
            .pt(top)
            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |a, cx| a.set_shortcuts_open(false, cx))))
            .child(
                div()
                    .id("shortcuts")
                    .w_full()
                    .max_w(px(512.))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .glass()
                    .bg(theme::GLASS_OVER_DIM)
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(header)
                    .child(body)
                    .child(footer),
            )
            .auto_id("shortcuts", "dialog", "Keyboard Shortcuts"),
        )
        .with_priority(1)
    }
}
