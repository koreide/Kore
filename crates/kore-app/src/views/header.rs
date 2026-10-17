//! Main header (spec 01 §6) + LabelFilterBar (§8). Shown only in the table
//! and details views.

use crate::automation::Automation;
use crate::state::app_state::AiPanelRequest;
use crate::state::types::{AppView, KindExt};
use crate::state::{AppEvent, AppState};
use crate::theme::{self, emerald, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::widgets::kbd;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

pub struct Header {
    app: Entity<AppState>,
    search: Entity<TextInput>,
    label_input: Entity<TextInput>,
    _subs: Vec<Subscription>,
}

impl Header {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(
                "Search...",
                InputStyle {
                    text_color: slate::S100,
                    placeholder_color: slate::S600,
                    font_size: 14.,
                    line_height: 20.,
                    padding: (40., 40., 6., 6.),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let label_input = cx.new(|cx| {
            TextInput::new(
                "app=name",
                InputStyle {
                    text_color: slate::S300,
                    placeholder_color: slate::S600,
                    font_size: 12.,
                    line_height: 16.,
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let subs = vec![
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::Change(s) = ev {
                    let s = s.clone();
                    this.app.update(cx, |a, cx| a.set_search(s, cx));
                }
                cx.notify();
            }),
            cx.subscribe(&label_input, |this, input, ev: &InputEvent, cx| match ev {
                InputEvent::Enter { .. } => {
                    let v = input.read(cx).text().trim().to_string();
                    let dup = this.app.read(cx).label_filters.contains(&v);
                    if !v.is_empty() && !dup {
                        this.app.update(cx, |a, cx| a.add_label(v, cx));
                        input.update(cx, |i, cx| i.set_text("", cx));
                    }
                }
                InputEvent::BackspaceEmpty => {
                    let n = this.app.read(cx).label_filters.len();
                    if n > 0 {
                        this.app.update(cx, |a, cx| a.remove_label(n - 1, cx));
                    }
                }
                _ => cx.notify(),
            }),
            cx.subscribe_in(&app, window, |this, app, ev: &AppEvent, window, cx| match ev {
                AppEvent::ContextChanged => {
                    let s = app.read(cx).search.clone();
                    this.search.update(cx, |i, cx| i.set_text(s, cx));
                }
                AppEvent::FocusSearch => this.search.update(cx, |i, cx| i.focus(window, cx)),
                _ => {}
            }),
        ];
        Header { app, search, label_input, _subs: subs }
    }

    pub fn focus_search(&self, window: &mut Window, cx: &mut App) {
        let h = self.search.focus_handle(cx);
        window.focus(&h, cx);
    }
}

fn focus_ring(cx: &App) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: theme::Theme::get(cx).focus_ring().into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(2.),
        inset: false,
    }]
}

impl Render for Header {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        // Context switches clear the search; keep the input in sync.
        if self.app.read(cx).search.is_empty() && !self.search.read(cx).text().is_empty() {
            self.search.update(cx, |i, cx| i.set_text("", cx));
        }
        let app = self.app.read(cx);
        let is_table = app.view == AppView::Table;
        let is_details = app.view == AppView::Details;
        let kind = app.kind;
        let count = app.visible_rows(cx).len();
        let selected = app.selected.clone();
        let filter_active = app.show_label_filter || !app.label_filters.is_empty();
        let labels = app.label_filters.clone();
        let status_filter = app.status_filter;
        let context = app.context.clone();
        let connected = app.connection.as_ref().map(|c| c.connected).unwrap_or(false);
        let search_focused = self.search.read(cx).is_focused(window);
        let label_focused = self.label_input.read(cx).is_focused(window);
        let wide = window.viewport_size().width >= px(640.);

        let breadcrumb = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .text_sm_tw()
            .min_w_0()
            .flex_none()
            .child(div().text_color(slate::S400).font_weight(FontWeight::MEDIUM).child(kind.header_label()))
            .map(|d| match (&selected, is_details) {
                (Some(sel), true) => d
                    .child(icon("chevron-right", 14.).text_color(slate::S600))
                    .child(
                        div()
                            .text_color(slate::S100)
                            .font_weight(FontWeight::MEDIUM)
                            .max_w(px(200.))
                            .truncate()
                            .child(sel.name.clone()),
                    ),
                _ => d.child(
                    div()
                        .text_10()
                        .bg(theme::hexa(0x1e293b, 0.8))
                        .text_color(slate::S400)
                        .px(px(6.))
                        .py(px(2.))
                        .rounded_full()
                        .font_weight(FontWeight::MEDIUM)
                        .child(count.to_string())
                        .auto_id("header.count", "text", count.to_string()),
                ),
            })
            // Palette status quick filter chip (`:running` / `:failed` / `:pending`), table only.
            .when_some(status_filter.filter(|_| is_table), |d, v| {
                let label = crate::state::app_state::status_filter_label(v);
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .px(px(8.))
                        .py(px(2.))
                        .rounded_full()
                        .text_xs_tw()
                        .text_color(accent)
                        .border_1()
                        .border_color(theme::PREFLIGHT_BORDER)
                        .child(icon("filter", 12.).text_color(accent))
                        .child(format!("Status: {label}"))
                        .child(
                            div()
                                .id("status-filter-x")
                                .group("status-filter-x")
                                .cursor_pointer()
                                .child(icon("x", 12.).text_color(accent).group_hover("status-filter-x", |s| s.text_color(red::R400)))
                                .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |a, cx| a.set_status_filter(None, cx))))
                                .auto_id("header.status-filter.clear", "button", "Clear status filter"),
                        )
                        .auto_id("header.status-filter", "chip", format!("Status: {label}")),
                )
            })
            .auto_id("header.breadcrumb", "text", kind.header_label());

        let search = div()
            .relative()
            .flex_1()
            .max_w(px(448.))
            .mx_auto()
            .child(
                div()
                    .w_full()
                    .h(px(34.))
                    .bg(theme::hexa(0x101828, 0.6))
                    .border_1()
                    .border_color(slate::S800)
                    // The field keeps its 8px radius under the focus ring.
                    .rounded(px(8.))
                    .when(search_focused, |d| d.shadow(focus_ring(cx)))
                    .child(self.search.clone()),
            )
            .child(
                div()
                    .absolute()
                    .left(px(12.))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(icon("search", 16.).text_color(slate::S500)),
            )
            .child(div().absolute().right(px(12.)).top_0().bottom_0().flex().items_center().child(kbd("/")))
            .auto_id("header.search", "textbox", "Search...");

        let filter_btn = is_table.then(|| {
            div()
                .id("filter-toggle")
                .group("filter-toggle")
                .px(px(8.))
                .py(px(6.))
                .rounded(px(8.))
                .border_1()
                .cursor_pointer()
                .border_color(if filter_active { theme::PREFLIGHT_BORDER } else { slate::S800 })
                .child(icon("filter", 14.).map(|s| {
                    if filter_active {
                        s.text_color(accent)
                    } else {
                        s.text_color(slate::S400).group_hover("filter-toggle", |s| s.text_color(slate::S200))
                    }
                }))
                .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |a, cx| a.toggle_label_filter(cx))))
                .auto_id("header.filter", "toggle", "Filter")
                .selected(filter_active)
        });

        let ctx_label: SharedString = context.clone().unwrap_or_else(|| "No context".into()).into();
        let context_pill = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .rounded(px(8.))
            .border_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.6))
            .text_xs_tw()
            .text_color(slate::S300)
            .child(div().size(px(6.)).flex_none().rounded_full().bg(if connected { emerald::E400 } else { red::R400 }))
            .child(div().font_mono().child(crate::ui::mono_text::mono_text(ctx_label.clone()).truncate(px(140.))))
            .auto_id("header.context", "text", ctx_label);

        let ai_btn = div()
            .id("ask-ai")
            .group("ask-ai")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(8.))
            .border_1()
            .border_color(slate::S800)
            .text_xs_tw()
            .text_color(slate::S400)
            .hover(move |s| s.text_color(accent))
            .cursor_pointer()
            .child(icon("sparkles", 14.).text_color(slate::S400).group_hover("ask-ai", move |s| s.text_color(accent)))
            .when(wide, |d| d.child("AI"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.app.update(cx, |a, cx| {
                    let req = match (&a.selected, a.view == AppView::Details) {
                        (Some(sel), true) => AiPanelRequest {
                            kind: Some(a.kind.as_str().to_string()),
                            namespace: Some(if sel.namespace.is_empty() { "default".into() } else { sel.namespace.clone() }),
                            name: Some(sel.name.clone()),
                        },
                        _ => AiPanelRequest::default(),
                    };
                    a.open_ai_panel(req, cx)
                })
            }))
            .auto_id("header.ai", "button", "Ask AI");

        let right = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .flex_none()
            .children(filter_btn)
            .child(context_pill)
            .child(ai_btn)
            .child(kbd(crate::keys::MOD_K));

        let row = div().flex().flex_row().items_center().gap(px(12.)).child(breadcrumb).child(search).child(right);

        let show_bar = filter_active && is_table;
        let bar = show_bar.then(|| {
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(icon("filter", 14.).text_color(slate::S500))
                .children(labels.iter().enumerate().map(|(ix, l)| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .px(px(8.))
                        .py(px(2.))
                        .rounded_full()
                        .text_xs_tw()
                        .text_color(accent)
                        .border_1()
                        .border_color(theme::PREFLIGHT_BORDER)
                        .child(div().font_mono().child(l.clone()))
                        .child(
                            div()
                                .id(("label-x", ix))
                                .group("label-x")
                                .cursor_pointer()
                                .child(icon("x", 12.).text_color(accent).group_hover("label-x", |s| s.text_color(red::R400)))
                                .on_click(cx.listener(move |this, _, _, cx| this.app.update(cx, |a, cx| a.remove_label(ix, cx))))
                                .auto_id(format!("labelbar.remove.{l}"), "button", format!("Remove {l}")),
                        )
                        .auto_id(format!("labelbar.chip.{l}"), "chip", l.clone())
                }))
                .child(
                    div()
                        .w(px(if labels.is_empty() { 128. } else { 96. }))
                        .h(px(16.))
                        .rounded(px(if label_focused { 4. } else { 0. }))
                        .when(label_focused, |d| d.shadow(focus_ring(cx)))
                        .child(self.label_input.clone())
                        .auto_id("labelbar.input", "textbox", "app=name"),
                )
        });

        div().flex().flex_col().gap(px(8.)).child(row).children(bar)
    }
}
