//! Pod Logs tab (spec 02 §2.5): toolbar, virtualized log list, search
//! overlay, Copy / Download / Auto-scroll / Previous and the container select.
//!
//! Hosted by `pod_details.rs` (Task 4) below its tabs bar while the Logs tab
//! is active; dropping the entity stops the stream.

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::resource_item::ResourceItem;
use crate::state::AppState;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::native_controls::{NativeSelect, SelectEvent, SelectStyle};
use crate::ui::scrollbar;
use crate::ui::text_search_bar::{SearchBarEvent, TextSearchBar};
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::kbd;
use crate::views::log_view::{self, LogBuffer, ROW_H};
use std::cell::Cell;
use std::rc::Rc;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::PodLogPayload;

/// Log options kept by the pod details view (they survive tab switches): raw select value (`""` = all, may be `init:x`),
/// Previous and Auto-scroll.
#[derive(Clone, Debug, PartialEq)]
pub struct LogOptions {
    pub container: String,
    pub previous: bool,
    pub auto_scroll: bool,
}

impl Default for LogOptions {
    fn default() -> Self {
        LogOptions { container: String::new(), previous: false, auto_scroll: true }
    }
}

/// Select value → API container (`init:` stripped, `""` → all containers).
pub fn strip_init(c: &str) -> Option<String> {
    if c.is_empty() {
        None
    } else {
        Some(c.strip_prefix("init:").unwrap_or(c).to_string())
    }
}

/// Download filename: `{pod}-{ISO timestamp with ':' and '.' → '-'}.log`.
pub fn download_name(pod: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let iso = now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    format!("{pod}-{}.log", iso.replace([':', '.'], "-"))
}

/// Container names from the pod JSON: `init:{name}` first, then regular.
pub fn container_names(pod: &serde_json::Value) -> Vec<String> {
    let mut names = Vec::new();
    let spec = &pod["spec"];
    if let Some(a) = spec["initContainers"].as_array() {
        names.extend(a.iter().filter_map(|c| c["name"].as_str()).map(|n| format!("init:{n}")));
    }
    if let Some(a) = spec["containers"].as_array() {
        names.extend(a.iter().filter_map(|c| c["name"].as_str()).map(String::from));
    }
    names
}

/// Where Download writes, when set (tests); otherwise the user's Downloads folder.
pub struct DownloadDir(pub std::path::PathBuf);

impl Global for DownloadDir {}

pub struct PodLogs {
    app: Entity<AppState>,
    item: ResourceItem,
    namespace: String,
    containers: Vec<String>,
    opts: LogOptions,
    select: Entity<NativeSelect>,
    buf: LogBuffer,
    loading: bool,
    stream: Option<Task<()>>,
    describe: Option<Task<()>>,
    /// Variable-height rows: a wrapped line takes its real height.
    list: ListState,
    /// Row to center once the list has laid it out (search navigation).
    pending_center: Rc<Cell<Option<usize>>>,
    search: Option<Entity<TextSearchBar>>,
    query: String,
    matches: Vec<usize>,
    match_ix: usize,
    _subs: Vec<Subscription>,
}

impl PodLogs {
    pub fn new(app: Entity<AppState>, item: ResourceItem, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let namespace = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let weak = app.downgrade();
        let select = cx.new(|cx| {
            NativeSelect::new(
                "logs.container",
                vec![(String::new(), "All containers".into())],
                String::new(),
                SelectStyle {
                    // The native popup has an intrinsic 20px height.
                    padding: (8., 1.),
                    radius: 6.,
                    font_size: 12.,
                    line_height: 16.,
                    text_color: slate::S300,
                    bg: theme::hexa(0x101828, 0.6),
                    border: slate::S800,
                    full_width: false,
                },
                weak,
                cx,
            )
        });
        let sel_sub = cx.subscribe(&select, |this, _, ev: &SelectEvent, cx| {
            let SelectEvent::Change(v) = ev;
            this.opts.container = v.clone();
            this.start_stream(cx);
        });
        let key_sub = keys::on_window_key(cx, |this: &mut Self, key, window, cx| {
            if key.mod_key("f") && !key.shift && !key.alt {
                this.open_search(window, cx);
                return true;
            }
            false
        });
        let mut this = PodLogs {
            app,
            item,
            namespace,
            containers: Vec::new(),
            opts: LogOptions::default(),
            select,
            buf: LogBuffer::default(),
            loading: true,
            stream: None,
            describe: None,
            list: ListState::new(0, ListAlignment::Top, px(400.)),
            pending_center: Rc::new(Cell::new(None)),
            search: None,
            query: String::new(),
            matches: Vec::new(),
            match_ix: 0,
            _subs: vec![sel_sub, key_sub],
        };
        this.load_containers(cx);
        this.start_stream(cx);
        this
    }

    // ── Public API used by pod_details.rs ──────────────────────────────

    /// Close the log search if it is open (Esc step 1). True when it was open.
    pub fn close_search(&mut self, cx: &mut Context<Self>) -> bool {
        if self.search.is_none() {
            return false;
        }
        self.search = None;
        self.query.clear();
        self.matches.clear();
        self.match_ix = 0;
        cx.notify();
        true
    }

    /// Selected log container with `init:` stripped; `None` = all.
    pub fn selected_container(&self) -> Option<String> {
        strip_init(&self.opts.container)
    }

    pub fn options(&self) -> LogOptions {
        self.opts.clone()
    }

    /// Restore options (call right after `new`); restarts the stream when the
    /// container or Previous changed.
    pub fn set_options(&mut self, opts: LogOptions, cx: &mut Context<Self>) {
        if opts == self.opts {
            return;
        }
        let restart = opts.container != self.opts.container || opts.previous != self.opts.previous;
        self.opts = opts;
        let c = self.opts.container.clone();
        self.select.update(cx, |s, cx| s.set_selected(c, cx));
        if restart {
            self.start_stream(cx);
        }
        cx.notify();
    }

    // ── Data ───────────────────────────────────────────────────────────

    fn load_containers(&mut self, cx: &mut Context<Self>) {
        let (ns, pod) = (self.namespace.clone(), self.item.name.clone());
        self.describe = Some(backend::run(
            cx,
            move |core| async move { core.describe_pod(ns, pod).await },
            |this: &mut Self, res, cx| {
                if let Ok(v) = res {
                    this.containers = container_names(&v);
                    let mut opts = vec![(String::new(), "All containers".to_string())];
                    opts.extend(this.containers.iter().map(|c| (c.clone(), c.clone())));
                    this.select.update(cx, |s, cx| s.set_options(opts, cx));
                    cx.notify();
                }
            },
        ));
    }

    fn start_stream(&mut self, cx: &mut Context<Self>) {
        self.stream = None; // drops the old subscription → cancels the backend stream
        self.buf.clear();
        self.list.reset(0);
        self.loading = true;
        self.recompute_matches();
        let (ns, pod) = (self.namespace.clone(), self.item.name.clone());
        let container = strip_init(&self.opts.container);
        let previous = self.opts.previous;
        let core = backend::core(cx);
        self.stream = Some(cx.spawn(async move |this, cx| {
            let res = core.start_pod_logs_stream(ns, pod, container, Some(previous)).await;
            let mut sub = match res {
                Ok(s) => s,
                Err(e) => {
                    this.update(cx, |t, cx| {
                        t.loading = false;
                        t.buf.replace(&format!("Error starting log stream: {}", backend::format_error(&e)));
                        t.list.reset(t.buf.rows());
                        t.after_change(cx);
                    })
                    .ok();
                    return;
                }
            };
            if this.update(cx, |t, cx| {
                t.loading = false;
                cx.notify();
            })
            .is_err()
            {
                return;
            }
            while let Some(first) = sub.recv().await {
                let mut batch = vec![first];
                batch.extend(sub.try_recv_batch(2048));
                let alive = this.update(cx, |t, cx| {
                    for p in batch {
                        t.apply(p);
                    }
                    t.after_change(cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn apply(&mut self, p: PodLogPayload) {
        if let Some(err) = p.error.filter(|e| !e.is_empty()) {
            self.append(&format!("\n[ERROR] {err}\n"));
            self.loading = false;
        } else if let Some(logs) = p.logs.filter(|l| !l.is_empty()) {
            if p.append.unwrap_or(false) {
                self.append(&logs);
            } else {
                self.buf.replace(&logs);
                self.list.reset(self.buf.rows());
            }
            self.loading = false;
        }
    }

    /// Append to the buffer and splice the list: the changed last row is
    /// re-measured, new rows are added (measured heights of the rest stay cached).
    fn append(&mut self, text: &str) {
        append_rows(&mut self.buf, &self.list, text);
    }

    fn after_change(&mut self, cx: &mut Context<Self>) {
        if !self.query.is_empty() {
            self.recompute_matches();
        }
        self.auto_scroll();
        cx.notify();
    }

    fn auto_scroll(&self) {
        let n = self.buf.rows();
        if self.opts.auto_scroll && n > 0 {
            self.list.scroll_to_end();
        }
    }

    fn recompute_matches(&mut self) {
        // Reported to the search bar in `render`.
        self.matches = self.buf.matches(&self.query);
    }

    // ── Search ─────────────────────────────────────────────────────────

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = &self.search {
            // Already visible: refocus after 50ms.
            let s = s.clone();
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
            SearchBarEvent::Next => this.navigate(true, cx),
            SearchBarEvent::Prev => this.navigate(false, cx),
            SearchBarEvent::Close => {
                this.close_search(cx);
            }
        });
        self._subs.push(sub);
        self.search = Some(bar);
        cx.notify();
    }

    fn navigate(&mut self, next: bool, cx: &mut Context<Self>) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.match_ix = nav_index(self.match_ix, n, next);
        // Put the row at the top (so it is laid out), then center it once its
        // bounds are known (see `render_list`).
        let ix = self.matches[self.match_ix];
        self.list.scroll_to(ListOffset { item_ix: ix, offset_in_item: px(0.) });
        self.pending_center.set(Some(ix));
        cx.notify();
    }

    // ── Actions ────────────────────────────────────────────────────────

    fn copy(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.buf.text()));
        toast(cx, "Copied to clipboard", ToastKind::Success);
    }

    fn download(&mut self, cx: &mut Context<Self>) {
        let name = download_name(&self.item.name, chrono::Utc::now());
        let dir = cx
            .try_global::<DownloadDir>()
            .map(|d| d.0.clone())
            .or_else(dirs::download_dir)
            .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")));
        let text = self.buf.text();
        let ok = dir
            .map(|d| {
                let _ = std::fs::create_dir_all(&d);
                std::fs::write(d.join(&name), text).is_ok()
            })
            .unwrap_or(false);
        if ok {
            toast(cx, "Logs downloaded", ToastKind::Success);
        } else {
            tracing::error!("failed to write {name}");
            toast(cx, format!("Failed to download logs: could not write {name}"), ToastKind::Error);
        }
    }

    // ── Render ─────────────────────────────────────────────────────────

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let search_on = self.search.is_some();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(6.))
            .border_b_1()
            .border_color(theme::hexa(0x1e293b, 0.5))
            .bg(theme::hexa(0x101828, 0.3))
            .text_xs_tw()
            .when(self.containers.len() > 1, |d| d.child(self.select.clone()))
            .child(
                log_view::check_chip("logs-previous", self.opts.previous, "Previous", 12., 6., 8., 4.)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.opts.previous = !this.opts.previous;
                        this.start_stream(cx);
                    }))
                    .auto_id("logs.previous", "checkbox", "Previous")
                    .selected(self.opts.previous),
            )
            .child(
                log_view::check_chip("logs-autoscroll", self.opts.auto_scroll, "Auto-scroll", 12., 6., 8., 4.)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.opts.auto_scroll = !this.opts.auto_scroll;
                        this.auto_scroll();
                        cx.notify();
                    }))
                    .auto_id("logs.autoscroll", "checkbox", "Auto-scroll")
                    .selected(self.opts.auto_scroll),
            )
            .child(div().flex_1())
            .child(
                log_view::tool_button("logs-copy", false, accent, 8., 4.)
                    .child(icon("copy", 14.).text_color(slate::S300))
                    .child("Copy")
                    .on_click(cx.listener(|this, _, _, cx| this.copy(cx)))
                    .auto_id("logs.copy", "button", "Copy logs to clipboard"),
            )
            .child(
                log_view::tool_button("logs-download", false, accent, 8., 4.)
                    .child(icon("download", 14.).text_color(slate::S300))
                    .child("Download")
                    .on_click(cx.listener(|this, _, _, cx| this.download(cx)))
                    .auto_id("logs.download", "button", "Download logs as file"),
            )
            .child(
                log_view::tool_button("logs-search", search_on, accent, 8., 4.)
                    .child(icon("search", 14.).text_color(if search_on { accent } else { slate::S300 }))
                    .child(kbd(crate::keys::MOD_F))
                    .on_click(cx.listener(|this, _, window, cx| this.open_search(window, cx)))
                    .auto_id("logs.search", "button", "Search logs")
                    .selected(search_on),
            )
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.loading && self.buf.is_empty() {
            return log_view::log_skeleton("pod-logs").into_any_element();
        }
        let n = self.buf.rows();
        if n == 0 {
            return log_view::log_font(div().p(px(16.)).text_color(slate::S500))
                .child("No logs available")
                .auto_id("logs.empty", "status", "No logs available")
                .into_any_element();
        }
        let this = cx.entity().downgrade();
        let list = list(self.list.clone(), move |ix, _window, cx| {
            this.upgrade().map(|e| e.read(cx).render_row(ix)).unwrap_or_else(|| div().into_any_element())
        })
        .flex_1()
        .h_full();
        let overflow = f32::from(self.list.max_offset_for_scrollbar().y) > 0.5;
        let (state, pending) = (self.list.clone(), self.pending_center.clone());
        log_view::log_font(div())
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .child(list)
            .when(overflow, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(log_view::list_scrollbar(self.list.clone()))
            .child(
                // Runs after the list's layout: center the navigated-to row.
                canvas(
                    move |_, window, _| {
                        let Some(ix) = pending.take() else { return };
                        let (Some(row), vp) = (state.bounds_for_item(ix), state.viewport_bounds()) else { return };
                        let delta = row.center().y - vp.center().y;
                        if delta.abs() > px(0.5) {
                            state.scroll_by(delta);
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .size_0(),
            )
            .auto_id("logs.list", "log", format!("{n} rows"))
            .into_any_element()
    }

    /// One log row. Its height follows the wrapped text (1px top padding +
    /// 19px per wrapped line), so long lines never paint over the next rows.
    fn render_row(&self, ix: usize) -> AnyElement {
        let q = &self.query;
        let line = self.buf.line(ix);
        let color = log_view::level_color(&line, true);
        let is_match = !q.is_empty() && log_view::contains_ci(&line, &q.to_lowercase());
        let is_current = self.matches.get(self.match_ix).copied() == Some(ix);
        div()
            .id(("log-row", ix))
            .group("log-row")
            .min_h(px(ROW_H))
            .w_full()
            .flex()
            .flex_row()
            .hover(|s| s.bg(theme::hexa(0xffffff, 0.02)))
            .when(is_current, |d| d.bg(theme::hexa(0xfbbf24, 0.1)))
            .when(is_match && !is_current, |d| d.bg(theme::hexa(0xfbbf24, 0.05)))
            .child(log_view::gutter_cell(ix + 1, "log-row"))
            .child(div().flex_1().min_w_0().pl(px(12.)).pt(px(1.)).text_color(color).child(log_view::log_text(line.clone(), q)))
            .auto_id(format!("logs.row.{ix}"), "row", line)
            .into_any_element()
    }
}

/// Append `text` to `buf` and splice `list` so its item count matches the
/// buffer's rows: the (possibly extended) last row is re-measured, new rows added.
pub fn append_rows(buf: &mut LogBuffer, list: &ListState, text: &str) {
    let old = buf.rows();
    let changed = buf.append(text).min(old);
    let new = buf.rows();
    list.splice(changed..old, new - changed);
}

/// Next/previous match index with wrap-around.
pub fn nav_index(ix: usize, n: usize, next: bool) -> usize {
    if n == 0 {
        return 0;
    }
    let ix = ix.min(n - 1);
    if next {
        (ix + 1) % n
    } else {
        (ix + n - 1) % n
    }
}

impl Render for PodLogs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(bar) = &self.search {
            let (cur, cnt) = (self.match_ix, self.matches.len());
            bar.update(cx, |b, cx| b.set_matches(cur, cnt, cx));
        }
        let _ = &self.app;
        let toolbar = self.render_toolbar(cx);
        let list = self.render_list(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .child(log_view::content_card().child(list))
                    .when_some(self.search.clone(), |d, s| d.child(s)),
            )
            .auto_id("view.pod-logs", "region", "Logs")
    }
}

#[cfg(test)]
mod tests {
    use super::{append_rows, container_names, download_name, nav_index, strip_init};
    use crate::views::log_view::LogBuffer;
    use gpui_kit::{px, ListAlignment, ListState};

    #[test]
    fn list_tracks_buffer_rows() {
        let (mut buf, list) = (LogBuffer::default(), ListState::new(0, ListAlignment::Top, px(100.)));
        for chunk in ["a\n", "partial", " line\nb\nc", "", "\n", &"x".repeat(5000), "\n"] {
            append_rows(&mut buf, &list, chunk);
            assert_eq!(list.item_count(), buf.rows(), "after {chunk:?}");
        }
        assert_eq!(buf.line(1).as_ref(), "partial line");
    }
    use chrono::TimeZone;

    #[test]
    fn strip() {
        assert_eq!(strip_init(""), None);
        assert_eq!(strip_init("init:init1").as_deref(), Some("init1"));
        assert_eq!(strip_init("main").as_deref(), Some("main"));
    }

    #[test]
    fn filename() {
        let t = chrono::Utc.with_ymd_and_hms(2026, 10, 15, 10, 11, 12).unwrap() + chrono::Duration::milliseconds(123);
        assert_eq!(download_name("nginx-abc", t), "nginx-abc-2026-10-15T10-11-12-123Z.log");
    }

    #[test]
    fn containers() {
        let v = serde_json::json!({"spec": {"initContainers": [{"name": "init1"}], "containers": [{"name": "a"}, {"name": "b"}]}});
        assert_eq!(container_names(&v), vec!["init:init1", "a", "b"]);
        assert!(container_names(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn nav() {
        assert_eq!(nav_index(0, 3, true), 1);
        assert_eq!(nav_index(2, 3, true), 0);
        assert_eq!(nav_index(0, 3, false), 2);
        assert_eq!(nav_index(5, 3, true), 0);
        assert_eq!(nav_index(0, 0, true), 0);
    }
}
