//! EventsTimeline (spec 02 §9): the Events tab of every detail view. Live
//! mode lists the resource's events; History mode (Fix 10: the backend now
//! stores events) queries the SQLite store over 1h/6h/24h/7d.

use crate::automation::Automation;
use crate::backend;
use crate::state::AppState;
use crate::theme::{self, amber, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::scrollbar::{self, Gutter};
use crate::ui::widgets::skeleton_bar;
use crate::views::detail_ui::time_ago;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct EventItem {
    pub reason: Option<String>,
    pub message: Option<String>,
    pub count: Option<i64>,
    /// `lastTimestamp || metadata.creationTimestamp`.
    pub time: Option<String>,
    pub type_: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Live,
    History,
}

const RANGES: [(&str, i64); 4] = [("1h", 3600), ("6h", 6 * 3600), ("24h", 24 * 3600), ("7d", 7 * 24 * 3600)];

fn ts_millis(t: &Option<String>) -> i64 {
    t.as_deref()
        .and_then(crate::state::resource_item::parse_ts)
        .map(|d| d.timestamp_millis())
        .unwrap_or(i64::MIN)
}

/// Live events → items, newest first (`lastTimestamp` falling back to creation).
pub fn live_items(raw: &[Value]) -> Vec<EventItem> {
    let s = |v: &Value| v.as_str().map(str::to_string);
    let mut items: Vec<EventItem> = raw
        .iter()
        .map(|e| EventItem {
            reason: s(&e["reason"]),
            message: s(&e["message"]),
            count: e["count"].as_i64(),
            time: s(&e["lastTimestamp"]).or_else(|| s(&e["metadata"]["creationTimestamp"])),
            type_: s(&e["type"]),
        })
        .collect();
    items.sort_by_key(|e| std::cmp::Reverse(ts_millis(&e.time)));
    items
}

pub struct EventsTimeline {
    app: Entity<AppState>,
    kind: String,
    namespace: String,
    name: String,
    mode: Mode,
    range: usize,
    events: Vec<EventItem>,
    loading: bool,
    error: Option<String>,
    scroll: ScrollHandle,
    gutter: Gutter,
    _task: Option<Task<()>>,
}

impl EventsTimeline {
    pub fn new(
        app: Entity<AppState>,
        kind: String,
        namespace: String,
        name: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = EventsTimeline {
            app,
            kind,
            namespace,
            name,
            mode: Mode::Live,
            range: 0,
            events: Vec::new(),
            loading: true,
            error: None,
            scroll: ScrollHandle::new(),
            gutter: Gutter::default(),
            _task: None,
        };
        this.fetch(cx);
        this
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();
        let (kind, ns, name) = (self.kind.clone(), self.namespace.clone(), self.name.clone());
        match self.mode {
            Mode::Live => {
                self._task = Some(backend::run(
                    cx,
                    move |core| async move { core.list_events_for_resource(kind, ns, name).await },
                    |this: &mut Self, res, cx| {
                        this.loading = false;
                        match res {
                            Ok(v) => this.events = live_items(&v),
                            Err(e) => this.error = Some(backend::format_error(&e)),
                        }
                        cx.notify();
                    },
                ));
            }
            Mode::History => {
                let now = chrono::Utc::now();
                let since = (now - chrono::Duration::seconds(RANGES[self.range].1)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let until = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let ns_opt = (ns != "*").then_some(ns);
                let _ = &self.app;
                self._task = Some(backend::run(
                    cx,
                    move |core| async move { core.query_stored_events(since, until, ns_opt).await },
                    move |this: &mut Self, res, cx| {
                        this.loading = false;
                        match res {
                            Ok(stored) => {
                                let mut items: Vec<EventItem> = stored
                                    .into_iter()
                                    .filter(|e| this.name.is_empty() || e.involved_object.contains(&this.name))
                                    .map(|e| EventItem {
                                        reason: Some(e.reason),
                                        message: Some(e.message),
                                        count: Some(e.count),
                                        time: Some(e.last_seen),
                                        type_: Some(e.event_type),
                                    })
                                    .collect();
                                items.sort_by_key(|e| std::cmp::Reverse(ts_millis(&e.time)));
                                this.events = items;
                            }
                            Err(e) => this.error = Some(backend::format_error(&e)),
                        }
                        cx.notify();
                    },
                ));
            }
        }
    }

    fn set_mode(&mut self, m: Mode, cx: &mut Context<Self>) {
        if self.mode != m {
            self.mode = m;
            self.fetch(cx);
        }
    }

    fn card(&self, i: usize, e: &EventItem) -> impl IntoElement {
        let (border, bg, ic, ic_color) = match e.type_.as_deref() {
            Some("Warning") => (theme::hexa(0xf59e0b, 0.3), theme::hexa(0xf59e0b, 0.05), "alert-triangle", amber::A400),
            Some("Error") => (theme::hexa(0xef4444, 0.3), theme::hexa(0xef4444, 0.05), "info", red::R400),
            _ => (theme::hexa(0x3b82f6, 0.3), theme::hexa(0x3b82f6, 0.05), "info", theme::hex(0x60a5fa)),
        };
        let reason = e.reason.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "Unknown".into());
        let message = e.message.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "-".into());
        let label = format!("{reason}: {message}");
        div()
            .id(ElementId::Name(format!("event-{i}").into()))
            .border_1()
            .border_color(border)
            .bg(bg)
            .rounded(px(8.))
            .p(px(12.))
            .hover(|s| s.bg(theme::hexa(0xffffff, 0.02)))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(12.))
                    .child(icon(ic, 16.).text_color(ic_color).mt(px(2.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .mb(px(4.))
                                    .child(
                                        div().text_xs_tw().font_weight(FontWeight::MEDIUM).text_color(slate::S200).child(reason),
                                    )
                                    .when(e.count.unwrap_or(0) > 1, |d| {
                                        d.child(
                                            div()
                                                .text_10()
                                                .line_height(px(15.))
                                                .bg(slate::S800)
                                                .text_color(slate::S400)
                                                .px(px(6.))
                                                .py(px(2.))
                                                .rounded_full()
                                                .child(format!("x{}", e.count.unwrap_or(0))),
                                        )
                                    })
                                    .child(
                                        div()
                                            .ml_auto()
                                            .flex_none()
                                            .text_10()
                                            .line_height(px(15.))
                                            .text_color(slate::S500)
                                            .child(time_ago(e.time.as_deref()))
                                            .auto_id(format!("events.card.{i}.time"), "text", time_ago(e.time.as_deref())),
                                    ),
                            )
                            .child(div().text_xs_tw().text_color(slate::S400).child(message)),
                    ),
            )
            .auto_id(format!("events.card.{i}"), "article", label)
    }
}

impl Render for EventsTimeline {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        if self.loading {
            let mut d = div().p(px(16.)).flex().flex_col().gap(px(12.));
            for i in 0..4 {
                d = d.child(
                    div().opacity(1. - i as f32 * 0.15).child(div().rounded(px(8.)).overflow_hidden().child(skeleton_bar(
                        ElementId::Name(format!("ev-skel-{i}").into()),
                        relative(1.),
                        64.,
                    ))),
                );
            }
            return d.into_any_element();
        }
        let mode_btn = |id: &'static str, label: &'static str, ic: &'static str, active: bool| {
            let color = if active { accent } else { slate::S400 };
            div()
                .id(id)
                .group(id)
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .px(px(12.))
                .py(px(6.))
                .rounded(px(6.))
                .text_xs_tw()
                .border_1()
                .cursor_pointer()
                .map(|d| {
                    if active {
                        d.border_color(theme::PREFLIGHT_BORDER).text_color(accent)
                    } else {
                        d.border_color(slate::S700)
                            .text_color(slate::S400)
                            .hover(|s| s.text_color(slate::S200).border_color(slate::S600))
                    }
                })
                .child(
                    icon(ic, 12.)
                        .text_color(color)
                        .when(!active, |s| s.group_hover(id, |s| s.text_color(slate::S200))),
                )
                .child(label)
        };
        let mut toggle = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .mb(px(4.))
            .child(
                mode_btn("events-live", "Live", "radio", self.mode == Mode::Live)
                    .on_click(cx.listener(|this, _, _, cx| this.set_mode(Mode::Live, cx)))
                    .auto_id("events.live", "button", "Live")
                    .selected(self.mode == Mode::Live),
            )
            .child(
                mode_btn("events-history", "History", "history", self.mode == Mode::History)
                    .on_click(cx.listener(|this, _, _, cx| this.set_mode(Mode::History, cx)))
                    .auto_id("events.history", "button", "History")
                    .selected(self.mode == Mode::History),
            );
        if self.mode == Mode::History {
            let mut group = div().flex().flex_row().items_center().gap(px(4.)).ml(px(8.));
            for (i, (label, _)) in RANGES.iter().enumerate() {
                let active = i == self.range;
                group = group.child(
                    div()
                        .id(ElementId::Name(format!("events-range-{label}").into()))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(4.))
                        .text_10()
                        .line_height(px(15.))
                        .font_mono()
                        .cursor_pointer()
                        .map(|d| if active { d.text_color(accent) } else { d.text_color(slate::S500).hover(|s| s.text_color(slate::S300)) })
                        .child(*label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.range != i {
                                this.range = i;
                                this.fetch(cx);
                            }
                        }))
                        .auto_id(format!("events.range.{label}"), "button", *label)
                        .selected(active),
                );
            }
            toggle = toggle.child(group);
        }
        if let Some(err) = self.error.clone() {
            // The toggle stays available so the user can switch modes.
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().px(px(16.)).pt(px(16.)).child(toggle))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.))
                        .p(px(16.))
                        .text_sm_tw()
                        .text_color(red::R400)
                        .child(icon("alert-circle", 16.).text_color(red::R400))
                        .child(format!("Failed to load events: {err}"))
                        .auto_id("events.error", "alert", format!("Failed to load events: {err}")),
                )
                .into_any_element();
        }
        if self.events.is_empty() {
            // The Live/History toggle is always rendered, so an empty mode
            // never traps the user.
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().flex_none().px(px(16.)).pt(px(16.)).child(toggle))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(8.))
                        .py(px(48.))
                        .text_color(slate::S400)
                        .child(icon("clock", 32.).text_color(slate::S600))
                        .child(div().text_sm_tw().child("No events found"))
                        .auto_id("events.empty", "status", "No events found"),
                )
                .into_any_element();
        }
        let mut list = div().p(px(16.)).flex().flex_col().gap(px(8.)).child(toggle);
        for (i, e) in self.events.iter().enumerate() {
            list = list.child(self.card(i, e));
        }
        let reserve = self.gutter.overflows();
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("events-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .when(reserve, |d| d.pr(px(scrollbar::WIDTH)))
                    .child(list),
            )
            .child(div().absolute().inset_0().child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone())))
            .auto_id("view.events", "region", "Events")
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::live_items;
    use serde_json::json;

    #[test]
    fn sorts_newest_first_with_fallback() {
        let raw = vec![
            json!({"reason": "A", "lastTimestamp": "2024-01-01T00:00:00Z"}),
            json!({"reason": "B", "metadata": {"creationTimestamp": "2024-01-02T00:00:00Z"}}),
            json!({"reason": "C"}),
        ];
        let v = live_items(&raw);
        assert_eq!(v.iter().map(|e| e.reason.clone().unwrap()).collect::<Vec<_>>(), vec!["B", "A", "C"]);
    }
}
