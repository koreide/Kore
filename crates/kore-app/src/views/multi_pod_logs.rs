//! Deployment Logs tab: aggregated multi-pod logs (spec 02 §4) with Fix 5
//! (the backend streams on the real selector; lines are delivered through a
//! `Subscription`, so they actually appear).

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::AppState;
use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::native_controls::{checkbox, SYSTEM_BLUE};
use crate::ui::mono_text::mono_text;
use crate::ui::scrollbar;
use crate::ui::text::caps;
use crate::ui::text_search_bar::{SearchBarEvent, TextSearchBar};
use crate::ui::toast::{toast, ToastKind};
use crate::views::log_view;
use crate::views::pod_logs::nav_index;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::MultiPodLogLine;
use std::collections::{BTreeSet, HashMap, VecDeque};

/// Buffer cap (oldest entries dropped).
pub const MAX_ENTRIES: usize = 10_000;

/// `#rrggbb` → color (slate-400 fallback).
pub fn parse_hex(c: &str) -> Rgba {
    u32::from_str_radix(c.trim_start_matches('#'), 16).map(theme::hex).unwrap_or(slate::S400)
}

/// `{n} pod(s)` plus ` ({k} hidden)`.
pub fn pod_count_label(n: usize, hidden: usize) -> String {
    let mut s = format!("{n} pod{}", if n != 1 { "s" } else { "" });
    if hidden > 0 {
        s.push_str(&format!(" ({hidden} hidden)"));
    }
    s
}

/// Text copied by Copy: `{i+1} [{pod}] {line}` per visible entry.
pub fn copy_text<'a>(entries: impl Iterator<Item = &'a MultiPodLogLine>) -> String {
    entries.enumerate().map(|(i, e)| format!("{} [{}] {}", i + 1, e.pod, e.line)).collect::<Vec<_>>().join("\n")
}

/// Kubernetes label-selector string for a `LabelSelector` JSON object
/// (`spec.selector` of a Deployment): `matchLabels` as `k=v`, then
/// `matchExpressions` (`k in (a,b)`, `k notin (a,b)`, `k`, `!k`), comma
/// separated. `None` when the selector is missing or empty (it would match
/// every pod).
pub fn label_selector_string(sel: &serde_json::Value) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = sel["matchLabels"].as_object() {
        let mut kv: Vec<(&String, &serde_json::Value)> = m.iter().collect();
        kv.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in kv {
            parts.push(format!("{k}={}", v.as_str().unwrap_or_default()));
        }
    }
    for e in sel["matchExpressions"].as_array().into_iter().flatten() {
        let Some(key) = e["key"].as_str() else { continue };
        let values = || {
            e["values"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(",")
        };
        match e["operator"].as_str() {
            Some("In") => parts.push(format!("{key} in ({})", values())),
            Some("NotIn") => parts.push(format!("{key} notin ({})", values())),
            Some("Exists") => parts.push(key.to_string()),
            Some("DoesNotExist") => parts.push(format!("!{key}")),
            _ => {}
        }
    }
    (!parts.is_empty()).then(|| parts.join(","))
}

pub struct MultiPodLogs {
    #[allow(dead_code)]
    app: Entity<AppState>,
    namespace: String,
    selector: String,
    entries: VecDeque<MultiPodLogLine>,
    /// Indexes into `entries` of entries from visible pods.
    visible: Vec<usize>,
    pod_colors: HashMap<String, String>,
    pods: BTreeSet<String>,
    hidden: BTreeSet<String>,
    loading: bool,
    error: Option<String>,
    auto_scroll: bool,
    filter_open: bool,
    stream: Option<Task<()>>,
    /// Resolves the Deployment selector before the stream starts.
    resolve: Option<Task<()>>,
    list: ListState,
    search: Option<Entity<TextSearchBar>>,
    query: String,
    matches: Vec<usize>,
    match_ix: usize,
    _subs: Vec<Subscription>,
}

impl MultiPodLogs {
    fn with_selector(app: Entity<AppState>, namespace: String, selector: String, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key_sub = keys::on_window_key(cx, |this: &mut Self, key, window, cx| {
            if key.mod_key("f") && !key.shift && !key.alt {
                this.open_search(window, cx);
                return true;
            }
            false
        });
        MultiPodLogs {
            app,
            namespace,
            selector,
            entries: VecDeque::new(),
            visible: Vec::new(),
            pod_colors: HashMap::new(),
            pods: BTreeSet::new(),
            hidden: BTreeSet::new(),
            loading: true,
            error: None,
            auto_scroll: true,
            filter_open: false,
            stream: None,
            resolve: None,
            list: ListState::new(0, ListAlignment::Top, px(400.)),
            search: None,
            query: String::new(),
            matches: Vec::new(),
            match_ix: 0,
            _subs: vec![key_sub],
        }
    }

    /// Logs of a Deployment's pods: the label selector is the Deployment's
    /// real `spec.selector` (fetched first), not a name heuristic.
    pub fn for_deployment(app: Entity<AppState>, namespace: String, deployment: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::with_selector(app, namespace.clone(), String::new(), window, cx);
        this.resolve = Some(cx.spawn(async move |this, cx| {
            let core = cx.update(|cx| backend::core(cx));
            let res = core.describe_resource(kore_core::ResourceKind::Deployments, namespace, deployment).await;
            let selector = match res {
                Ok(v) => label_selector_string(&v["spec"]["selector"])
                    .ok_or_else(|| "Validation error: Deployment has no pod selector".to_string()),
                Err(e) => Err(backend::format_error(&e)),
            };
            this.update(cx, |t, cx| match selector {
                Ok(sel) => {
                    t.selector = sel;
                    t.start(cx);
                    cx.notify();
                }
                Err(err) => {
                    t.error = Some(err);
                    t.loading = false;
                    cx.notify();
                }
            })
            .ok();
        }));
        this
    }

    /// Close the search overlay if open; true when it was.
    pub fn close_search(&mut self, cx: &mut Context<Self>) -> bool {
        if self.search.take().is_none() {
            return false;
        }
        self.query.clear();
        self.matches.clear();
        self.match_ix = 0;
        cx.notify();
        true
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let (ns, sel) = (self.namespace.clone(), self.selector.clone());
        let core = backend::core(cx);
        self.stream = Some(cx.spawn(async move |this, cx| {
            let res = core.stream_multi_pod_logs(ns, sel, None).await;
            let mut lines = match res {
                Ok(s) => {
                    // Connected: an idle stream shows "No logs available", not the skeleton.
                    if this.update(cx, |t, cx| {
                        t.loading = false;
                        cx.notify();
                    })
                    .is_err()
                    {
                        return;
                    }
                    s.lines
                }
                Err(e) => {
                    this.update(cx, |t, cx| {
                        t.error = Some(backend::format_error(&e));
                        t.loading = false;
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            while let Some(first) = lines.recv().await {
                let mut batch = vec![first];
                batch.extend(lines.try_recv_batch(2048));
                if this.update(cx, |t, cx| t.push(batch, cx)).is_err() {
                    break;
                }
            }
        }));
    }

    fn push(&mut self, batch: Vec<MultiPodLogLine>, cx: &mut Context<Self>) {
        self.loading = false;
        for e in batch {
            if !self.pod_colors.contains_key(&e.pod) {
                self.pod_colors.insert(e.pod.clone(), e.color.clone());
                self.pods.insert(e.pod.clone());
            }
            self.entries.push_back(e);
        }
        let mut dropped = 0;
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
            dropped += 1;
        }
        // Incremental list update: drop the visible rows that fell off the
        // front, append the new visible rows (row heights stay cached).
        let old_n = self.visible.len();
        let dropped_visible = self.visible.iter().take_while(|&&i| i < dropped).count();
        self.visible = visible_indexes(&self.entries, &self.hidden);
        let added = self.visible.len() + dropped_visible - old_n;
        self.list.splice(0..dropped_visible, 0);
        let at = old_n - dropped_visible;
        self.list.splice(at..at, added);
        self.after_rows_changed(cx);
    }

    fn after_rows_changed(&mut self, cx: &mut Context<Self>) {
        self.recompute_matches();
        if self.auto_scroll && !self.visible.is_empty() {
            self.list.scroll_to_end();
        }
        cx.notify();
    }

    /// Recompute visible rows, matches and auto-scroll.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.visible = visible_indexes(&self.entries, &self.hidden);
        self.list.reset(self.visible.len());
        self.after_rows_changed(cx);
    }

    fn recompute_matches(&mut self) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }
        let q = self.query.to_lowercase();
        for (vi, &ei) in self.visible.iter().enumerate() {
            let e = &self.entries[ei];
            if log_view::contains_ci(&e.line, &q) || log_view::contains_ci(&e.pod, &q) {
                self.matches.push(vi);
            }
        }
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.search.clone() {
            let t = backend::sleep(cx, std::time::Duration::from_millis(50));
            cx.spawn_in(window, async move |_, cx| {
                t.await;
                cx.update(|window, cx| s.update(cx, |b, cx| b.focus(window, cx))).ok();
            })
            .detach();
            return;
        }
        let bar = cx.new(|cx| {
            let mut b = TextSearchBar::new(window, cx);
            b.set_logs_mode(cx);
            b
        });
        let sub = cx.subscribe(&bar, |this, _, ev: &SearchBarEvent, cx| match ev {
            SearchBarEvent::Query(q) => {
                this.query = q.clone();
                this.match_ix = 0;
                this.recompute_matches();
                cx.notify();
            }
            SearchBarEvent::Next | SearchBarEvent::Prev => {
                let n = this.matches.len();
                if n > 0 {
                    this.match_ix = nav_index(this.match_ix, n, matches!(ev, SearchBarEvent::Next));
                    this.list.scroll_to_reveal_item(this.matches[this.match_ix]);
                    cx.notify();
                }
            }
            SearchBarEvent::Close => {
                this.close_search(cx);
            }
        });
        self._subs.push(sub);
        self.search = Some(bar);
        cx.notify();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = copy_text(self.visible.iter().map(|&i| &self.entries[i]));
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        toast(cx, "Copied to clipboard", ToastKind::Success);
    }

    fn toggle_pod(&mut self, pod: &str, cx: &mut Context<Self>) {
        if !self.hidden.remove(pod) {
            self.hidden.insert(pod.to_string());
        }
        self.rebuild(cx);
    }

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let f = self.filter_open;
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .px(px(16.))
            .py(px(8.))
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_xs_tw()
                            .font_mono()
                            .text_color(slate::S400)
                            .child(format!("{}/{}", self.namespace, self.selector))
                            .auto_id("multilogs.selector", "text", format!("{}/{}", self.namespace, self.selector)),
                    )
                    .child(
                        div()
                            .text_10()
                            .text_color(slate::S600)
                            .child(pod_count_label(self.pods.len(), self.hidden.len()))
                            .auto_id("multilogs.count", "status", pod_count_label(self.pods.len(), self.hidden.len())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .text_sm_tw()
                    .child(
                        log_view::tool_button("mpl-filter", f, accent, 12., 6.)
                            .child(icon("filter", 14.).text_color(if f { accent } else { slate::S300 }))
                            .child("Filter")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.filter_open = !this.filter_open;
                                cx.notify();
                            }))
                            .auto_id("multilogs.filter", "button", "Filter")
                            .selected(f),
                    )
                    .child(
                        log_view::tool_button("mpl-copy", false, accent, 12., 6.)
                            .child(icon("copy", 14.).text_color(slate::S300))
                            .child("Copy")
                            .on_click(cx.listener(|this, _, _, cx| this.copy(cx)))
                            .auto_id("multilogs.copy", "button", "Copy"),
                    )
                    .child(
                        log_view::check_chip("mpl-autoscroll", self.auto_scroll, "Auto-scroll", 14., 8., 12., 6.)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.auto_scroll = !this.auto_scroll;
                                this.rebuild(cx);
                            }))
                            .auto_id("multilogs.autoscroll", "checkbox", "Auto-scroll")
                            .selected(self.auto_scroll),
                    ),
            )
    }

    fn render_filter(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let link = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .text_10()
                .text_color(accent)
                .hover(move |s| s.text_color(Rgba { a: 0.8, ..accent }))
                .cursor_pointer()
                .child(label)
        };
        let chips: Vec<AnyElement> = self
            .pods
            .iter()
            .map(|pod| {
                let vis = !self.hidden.contains(pod);
                let color = parse_hex(self.pod_colors.get(pod).map(String::as_str).unwrap_or("#94a3b8"));
                let p = pod.clone();
                div()
                    .id(SharedString::from(format!("mpl-pod-{pod}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .border_1()
                    .text_xs_tw()
                    .font_mono()
                    .cursor_pointer()
                    .when(vis, |d| d.border_color(slate::S700).bg(theme::hexa(0x1e293b, 0.5)).text_color(slate::S200))
                    .when(!vis, |d| d.border_color(slate::S800).text_color(slate::S600))
                    .child(checkbox(vis, 12., SYSTEM_BLUE))
                    .child(div().size(px(8.)).flex_none().rounded_full().bg(color))
                    .child(div().max_w(px(160.)).truncate().child(pod.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_pod(&p, cx)))
                    .auto_id(format!("multilogs.pod.{pod}"), "checkbox", pod.clone())
                    .selected(vis)
                    .into_any_element()
            })
            .collect();
        div()
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.3))
            .px(px(16.))
            .py(px(8.))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(div().mr(px(4.)).text_10().text_color(slate::S500).child(caps("Pods", 10.)))
            .child(
                link("mpl-all", "All")
                    .mr(px(4.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.hidden.clear();
                        this.rebuild(cx);
                    }))
                    .auto_id("multilogs.all", "button", "All"),
            )
            .child(
                link("mpl-none", "None")
                    .mr(px(8.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.hidden = this.pods.clone();
                        this.rebuild(cx);
                    }))
                    .auto_id("multilogs.none", "button", "None"),
            )
            .children(chips)
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.loading && self.entries.is_empty() {
            return log_view::log_skeleton("multi-logs").into_any_element();
        }
        if let Some(err) = &self.error {
            return div()
                .p(px(16.))
                .text_xs_tw()
                .font_mono()
                .text_color(red::R400)
                .child(err.clone())
                .auto_id("multilogs.error", "alert", err.clone())
                .into_any_element();
        }
        if self.visible.is_empty() {
            let msg = if self.entries.is_empty() { "No logs available" } else { "All pods are hidden. Adjust filters to see logs." };
            return log_view::log_font(div().p(px(16.)).text_color(slate::S500))
                .child(msg)
                .auto_id("multilogs.empty", "status", msg)
                .into_any_element();
        }
        let n = self.visible.len();
        let this = cx.entity().downgrade();
        let list = list(self.list.clone(), move |vi, _window, cx| {
            this.upgrade().map(|e| e.read(cx).render_row(vi)).unwrap_or_else(|| div().into_any_element())
        })
        .flex_1()
        .h_full();
        let overflow = f32::from(self.list.max_offset_for_scrollbar().y) > 0.5;
        log_view::log_font(div())
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .child(list)
            .when(overflow, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(log_view::list_scrollbar(self.list.clone()))
            .auto_id("multilogs.list", "log", format!("{n} rows"))
            .into_any_element()
    }
}

impl MultiPodLogs {
    /// One variable-height row (the line wraps like `pre-wrap`).
    fn render_row(&self, vi: usize) -> AnyElement {
        let Some(e) = self.visible.get(vi).and_then(|&i| self.entries.get(i)) else {
            return div().into_any_element();
        };
        let q = self.query.clone();
        let ql = q.to_lowercase();
        let current = self.matches.get(self.match_ix).copied();
        let line: SharedString = e.line.clone().into();
        let pod: SharedString = e.pod.clone().into();
        let is_match = !q.is_empty() && (log_view::contains_ci(&e.line, &ql) || log_view::contains_ci(&e.pod, &ql));
        let is_current = current == Some(vi);
        log_view::log_font(div())
            .id(("mlog-row", vi))
            .group("mlog-row")
            .w_full()
            .flex()
            .flex_row()
            .hover(|s| s.bg(theme::hexa(0xffffff, 0.02)))
            .when(is_current, |d| d.bg(theme::hexa(0xfbbf24, 0.1)))
            .when(is_match && !is_current, |d| d.bg(theme::hexa(0xfbbf24, 0.05)))
            .child(log_view::gutter_cell(vi + 1, "mlog-row"))
            .child(
                div()
                    .flex_none()
                    .px(px(8.))
                    .py(px(1.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(parse_hex(&e.color))
                    .when(q.is_empty(), |d| d.child(mono_text(pod.clone()).truncate(px(164.))))
                    .when(!q.is_empty(), |d| {
                        d.child(div().max_w(px(164.)).overflow_hidden().child(log_view::log_text(pod.clone(), &q).wrap(false)))
                    }),
            )
            .child(div().py(px(1.)).text_color(slate::S700).child("|"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(px(8.))
                    .py(px(1.))
                    .text_color(log_view::level_color(&e.line, false))
                    .child(log_view::log_text(line, &q)),
            )
            .auto_id(format!("multilogs.row.{vi}"), "row", format!("{} {}", e.pod, e.line))
            .into_any_element()
    }
}

/// Indexes of entries whose pod is not hidden.
pub fn visible_indexes(entries: &VecDeque<MultiPodLogLine>, hidden: &BTreeSet<String>) -> Vec<usize> {
    entries.iter().enumerate().filter(|(_, e)| !hidden.contains(&e.pod)).map(|(i, _)| i).collect()
}

impl Render for MultiPodLogs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(bar) = &self.search {
            let (cur, cnt) = (self.match_ix, self.matches.len());
            bar.update(cx, |b, cx| b.set_matches(cur, cnt, cx));
        }
        let toolbar = self.render_toolbar(cx);
        let show_filter = self.filter_open && !self.pods.is_empty();
        let filter = show_filter.then(|| self.render_filter(cx));
        let body = self.render_body(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .children(filter)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .child(log_view::content_card().child(body))
                    .when_some(self.search.clone(), |d, s| d.child(s)),
            )
            .auto_id("view.multi-pod-logs", "region", "Logs")
    }
}

#[cfg(test)]
mod tests {
    use super::{copy_text, parse_hex, pod_count_label, visible_indexes};
    use crate::theme;
    use kore_core::MultiPodLogLine;

    #[test]
    fn visibility() {
        let mk = |p: &str| MultiPodLogLine { pod: p.into(), color: "#fff".into(), line: "x".into() };
        let e: std::collections::VecDeque<_> = [mk("a"), mk("b"), mk("a")].into_iter().collect();
        let hidden: std::collections::BTreeSet<String> = ["b".to_string()].into_iter().collect();
        assert_eq!(visible_indexes(&e, &hidden), vec![0, 2]);
        assert_eq!(visible_indexes(&e, &Default::default()), vec![0, 1, 2]);
    }

    #[test]
    fn labels() {
        assert_eq!(pod_count_label(0, 0), "0 pods");
        assert_eq!(pod_count_label(1, 0), "1 pod");
        assert_eq!(pod_count_label(2, 1), "2 pods (1 hidden)");
    }

    #[test]
    fn colors_and_copy() {
        assert_eq!(parse_hex("#58d0ff"), theme::hex(0x58d0ff));
        assert_eq!(parse_hex("bogus"), theme::slate::S400);
        let e = [
            MultiPodLogLine { pod: "a".into(), color: "#fff".into(), line: "x".into() },
            MultiPodLogLine { pod: "b".into(), color: "#fff".into(), line: "y".into() },
        ];
        assert_eq!(copy_text(e.iter()), "1 [a] x\n2 [b] y");
    }
}
