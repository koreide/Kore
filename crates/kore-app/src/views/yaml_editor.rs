//! YamlEditor (spec 02 §5): the YAML tab of every detail view. Toolbar
//! (Read-only/Editing toggle, Diff, Reset, Copy, Apply), the text area with a
//! line-number gutter that stays aligned (Fix 15), the inline diff pane, the
//! "Unsaved changes" pill and ⌘F search.

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::app_state::{ConfirmRequest, ConfirmVariant};
use crate::state::types::ResourceKind;
use crate::state::AppState;
use crate::theme::{self, amber, emerald, red, slate, KoreStyled};
use crate::ui::mono_text::{mono_paragraph, mono_text};
use crate::ui::scrollbar::{self, Gutter};
use crate::ui::text::caps;
use crate::ui::text_search_bar::{SearchBarEvent, TextSearchBar};
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::spinning_icon;
use crate::views::detail_ui::{bars_skeleton, btn_icon, button, BtnStyle};
use crate::views::yaml_text_area::{SearchMarks, TextAreaEvent, YamlTextArea};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use crate::views::yaml_text_area::LINE_H as WEB_RELAXED_12;
use kore_core::state::yaml::DiffLine;
use std::rc::Rc;
use std::time::Duration;

const SKELETON: [f32; 12] = [85., 60., 75., 40., 90., 50., 70., 55., 80., 45., 65., 35.];

/// Case-insensitive, overlapping substring positions (byte offsets).
pub fn find_positions(text: &str, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let t = text.to_lowercase();
    let q = query.to_lowercase();
    // Lower-casing can change byte lengths for non-ASCII; only trust offsets when it did not.
    if t.len() != text.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = t[pos..].find(&q) {
        out.push(pos + i);
        pos += i + t[pos + i..].chars().next().map(char::len_utf8).unwrap_or(1);
        if pos > t.len() {
            break;
        }
    }
    out
}

pub struct YamlEditor {
    app: Entity<AppState>,
    kind: ResourceKind,
    namespace: String,
    name: String,
    original: String,
    loading: bool,
    read_only: bool,
    applying: bool,
    diff: Option<Vec<DiffLine>>,
    diff_loading: bool,
    area: Entity<YamlTextArea>,
    search: Option<Entity<TextSearchBar>>,
    search_input: String,
    search_query: String,
    search_index: usize,
    matches: Vec<usize>,
    diff_scroll: ScrollHandle,
    diff_gutter: Gutter,
    _debounce: Option<Task<()>>,
    _load: Option<Task<()>>,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl YamlEditor {
    pub fn new(
        app: Entity<AppState>,
        kind: ResourceKind,
        namespace: String,
        name: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let area = cx.new(YamlTextArea::new);
        let subs = vec![
            cx.subscribe(&area, |this, _, ev: &TextAreaEvent, cx| match ev {
                TextAreaEvent::Changed => {
                    this.recompute_matches(cx);
                    cx.notify();
                }
            }),
            keys::on_window_key(cx, |this: &mut Self, k, window, cx| {
                if k.mod_key("f") && !k.shift {
                    this.open_search(window, cx);
                    return true;
                }
                false
            }),
        ];
        let mut this = YamlEditor {
            app,
            kind,
            namespace,
            name,
            original: String::new(),
            loading: true,
            read_only: true,
            applying: false,
            diff: None,
            diff_loading: false,
            area,
            search: None,
            search_input: String::new(),
            search_query: String::new(),
            search_index: 0,
            matches: Vec::new(),
            diff_scroll: ScrollHandle::new(),
            diff_gutter: Gutter::default(),
            _debounce: None,
            _load: None,
            _task: None,
            _subs: subs,
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.diff = None;
        self.read_only = true;
        let (kind, ns, name) = (self.kind, self.namespace.clone(), self.name.clone());
        self._load = Some(backend::run(
            cx,
            move |core| async move { core.get_resource_yaml(kind, ns, name).await },
            |this: &mut Self, res, cx| {
                let text = match res {
                    Ok(y) => {
                        this.original = y.clone();
                        y
                    }
                    Err(e) => {
                        let msg = backend::format_error(&e);
                        toast(cx, msg.clone(), ToastKind::Error);
                        format!("# Error fetching YAML: {msg}")
                    }
                };
                this.area.update(cx, |a, cx| {
                    a.set_text(text, cx);
                    a.set_read_only(true, cx);
                });
                this.loading = false;
                this.recompute_matches(cx);
                cx.notify();
            },
        ));
    }

    fn yaml(&self, cx: &App) -> String {
        self.area.read(cx).text().to_string()
    }

    fn has_changes(&self, cx: &App) -> bool {
        self.area.read(cx).text() != self.original
    }

    fn set_text(&mut self, t: String, cx: &mut Context<Self>) {
        self.area.update(cx, |a, cx| a.set_text(t, cx));
        self.recompute_matches(cx);
    }

    fn toggle_edit(&mut self, cx: &mut Context<Self>) {
        if !self.read_only {
            let o = self.original.clone();
            self.set_text(o, cx);
            self.diff = None;
        }
        self.read_only = !self.read_only;
        let ro = self.read_only;
        self.area.update(cx, |a, cx| a.set_read_only(ro, cx));
        cx.notify();
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        let o = self.original.clone();
        self.set_text(o, cx);
        self.diff = None;
        cx.notify();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let y = self.yaml(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(y));
        toast(cx, "YAML copied to clipboard", ToastKind::Success);
    }

    fn show_diff(&mut self, cx: &mut Context<Self>) {
        if !self.has_changes(cx) {
            return;
        }
        self.diff_loading = true;
        cx.notify();
        let (kind, ns, name, y) = (self.kind, self.namespace.clone(), self.name.clone(), self.yaml(cx));
        self._task = Some(backend::run(
            cx,
            move |core| async move { core.diff_resource_yaml(kind, ns, name, y).await },
            |this: &mut Self, res, cx| {
                this.diff_loading = false;
                match res {
                    Ok(d) => this.diff = Some(d),
                    Err(e) => toast(cx, backend::format_error(&e), ToastKind::Error),
                }
                cx.notify();
            },
        ));
    }

    fn request_apply(&mut self, cx: &mut Context<Self>) {
        let singular = self.kind.as_str().strip_suffix('s').unwrap_or(self.kind.as_str()).to_string();
        let this = cx.entity().downgrade();
        let app = self.app.clone();
        let msg = format!(
            "Are you sure you want to apply the modified YAML to {singular} \"{}\" in namespace \"{}\"? This will update the live resource.",
            self.name, self.namespace
        );
        self.app.update(cx, |s, cx| {
            s.confirm(
                ConfirmRequest {
                    title: "Apply YAML Changes".into(),
                    message: msg,
                    confirm_label: "Apply".into(),
                    cancel_label: "Cancel".into(),
                    variant: ConfirmVariant::Warning,
                    on_confirm: Rc::new(move |_, cx| {
                        app.update(cx, |s, cx| s.close_confirm(cx));
                        this.update(cx, |e, cx| e.apply(cx)).ok();
                    }),
                },
                cx,
            )
        });
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        self.applying = true;
        cx.notify();
        let (kind, ns, name, y) = (self.kind, self.namespace.clone(), self.name.clone(), self.yaml(cx));
        let applied = y.clone();
        self._task = Some(backend::run(
            cx,
            move |core| async move { core.apply_resource_yaml(kind, ns, name, y).await },
            move |this: &mut Self, res, cx| {
                this.applying = false;
                match res {
                    Ok(r) => {
                        let msg = if r.is_empty() { "YAML applied successfully".to_string() } else { r };
                        toast(cx, msg, ToastKind::Success);
                        this.original = applied;
                        this.diff = None;
                        this.read_only = true;
                        this.area.update(cx, |a, cx| a.set_read_only(true, cx));
                    }
                    Err(e) => toast(cx, backend::format_error(&e), ToastKind::Error),
                }
                cx.notify();
            },
        ));
    }

    // ── Search ──────────────────────────────────────────────────────────

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = &self.search {
            s.update(cx, |s, cx| s.focus(window, cx));
            return;
        }
        let bar = cx.new(|cx| TextSearchBar::new(window, cx));
        self._subs.push(cx.subscribe(&bar, |this, _, ev: &SearchBarEvent, cx| match ev {
            SearchBarEvent::Query(q) => {
                this.search_input = q.clone();
                this.search_index = 0;
                let q = q.clone();
                let t = backend::sleep(cx, Duration::from_millis(200));
                this._debounce = Some(cx.spawn(async move |this, cx| {
                    t.await;
                    this.update(cx, |this, cx| {
                        this.search_query = q;
                        this.recompute_matches(cx);
                        this.scroll_to_current(cx);
                    })
                    .ok();
                }));
            }
            SearchBarEvent::Next => {
                if !this.matches.is_empty() {
                    this.search_index = (this.search_index + 1) % this.matches.len();
                    this.diff = None;
                    this.recompute_matches(cx);
                    this.scroll_to_current(cx);
                }
            }
            SearchBarEvent::Prev => {
                if !this.matches.is_empty() {
                    let n = this.matches.len();
                    this.search_index = (this.search_index + n - 1) % n;
                    this.diff = None;
                    this.recompute_matches(cx);
                    this.scroll_to_current(cx);
                }
            }
            SearchBarEvent::Close => {
                this.search = None;
                this.search_input.clear();
                this.search_query.clear();
                this.search_index = 0;
                this.recompute_matches(cx);
            }
        }));
        self.search = Some(bar);
        cx.notify();
    }

    fn recompute_matches(&mut self, cx: &mut Context<Self>) {
        let text = self.yaml(cx);
        self.matches = find_positions(&text, &self.search_query);
        if self.search_index >= self.matches.len() {
            self.search_index = 0;
        }
        let marks = if self.search.is_some() && !self.matches.is_empty() {
            SearchMarks { starts: self.matches.clone(), len: self.search_query.len(), current: self.search_index }
        } else {
            SearchMarks::default()
        };
        self.area.update(cx, |a, cx| a.set_marks(marks, cx));
        if let Some(bar) = &self.search {
            let (c, n) = (self.search_index, self.matches.len());
            bar.update(cx, |b, cx| b.set_matches(c, n, cx));
        }
        cx.notify();
    }

    fn scroll_to_current(&mut self, cx: &mut Context<Self>) {
        if let Some(&pos) = self.matches.get(self.search_index) {
            self.area.update(cx, |a, cx| a.scroll_to_pos(pos, cx));
        }
    }

    // ── Render ──────────────────────────────────────────────────────────

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let editing = !self.read_only;
        let changes = editing && self.has_changes(cx);
        let small = |b: Stateful<Div>| b.px(px(10.)).py(px(6.)).text_xs_tw();
        let toggle = if self.read_only {
            small(button("yaml-toggle", BtnStyle::Plain, cx))
                .text_color(slate::S400)
                .hover(|s| s.bg(gpui_kit::transparent_black()))
                .child(btn_icon("lock", 14., slate::S400))
                .child("Read-only")
        } else {
            small(button("yaml-toggle", BtnStyle::Active, cx)).child(btn_icon("pencil", 14., accent)).child("Editing")
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .flex_none()
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
                    .gap(px(6.))
                    .child(div().text_10().text_color(slate::S500).mr(px(8.)).child(caps("YAML", 10.)))
                    .child(
                        toggle
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_edit(cx)))
                            .auto_id("yaml.toggle", "button", if self.read_only { "Read-only" } else { "Editing" }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .when(changes, |d| {
                        d.child(
                            small(button("yaml-diff", BtnStyle::Standard, cx))
                                .when(self.diff_loading, |b| b.opacity(0.5))
                                .map(|b| {
                                    if self.diff_loading {
                                        b.child(spinning_icon("yaml-diff-spin", "loader-2", 14., slate::S300))
                                    } else {
                                        b.child(btn_icon("git-compare-arrows", 14., slate::S300))
                                    }
                                })
                                .child("Diff")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.diff_loading {
                                        this.show_diff(cx)
                                    }
                                }))
                                .auto_id("yaml.diff", "button", "Diff"),
                        )
                        .child(
                            small(button("yaml-reset", BtnStyle::Plain, cx))
                                .hover(|s| s.border_color(theme::hexa(0xf59e0b, 0.5)).bg(theme::hexa(0xf59e0b, 0.1)))
                                .child(btn_icon("rotate-ccw", 14., slate::S300))
                                .child("Reset")
                                .on_click(cx.listener(|this, _, _, cx| this.reset(cx)))
                                .auto_id("yaml.reset", "button", "Reset"),
                        )
                    })
                    .child(
                        small(button("yaml-copy", BtnStyle::Standard, cx))
                            .child(btn_icon("copy", 14., slate::S300))
                            .child("Copy")
                            .on_click(cx.listener(|this, _, _, cx| this.copy(cx)))
                            .auto_id("yaml.copy", "button", "Copy"),
                    )
                    .when(changes, |d| {
                        d.child(
                            small(button("yaml-apply", BtnStyle::Plain, cx))
                                .border_color(theme::hexa(0x047857, 0.5))
                                .bg(theme::hexa(0x10b981, 0.1))
                                .text_color(emerald::E400)
                                .hover(|s| s.bg(theme::hexa(0x10b981, 0.2)).border_color(emerald::E500))
                                .when(self.applying, |b| b.opacity(0.5))
                                .map(|b| {
                                    if self.applying {
                                        b.child(spinning_icon("yaml-apply-spin", "loader-2", 14., emerald::E400))
                                    } else {
                                        b.child(btn_icon("save", 14., emerald::E400))
                                    }
                                })
                                .child("Apply")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.applying {
                                        this.request_apply(cx)
                                    }
                                }))
                                .auto_id("yaml.apply", "button", "Apply"),
                        )
                    }),
            )
    }

    fn diff_pane(&self, diff: &[DiffLine], cx: &mut Context<Self>) -> impl IntoElement {
        let adds = diff.iter().filter(|l| l.tag == "insert").count();
        let dels = diff.iter().filter(|l| l.tag == "delete").count();
        let mut rows = div().flex().flex_col().font_mono().text_size(px(12.)).line_height(px(WEB_RELAXED_12));
        for (i, l) in diff.iter().enumerate() {
            let (bg, gut, sign_c, text_c, sign) = match l.tag.as_str() {
                "insert" => (Some(theme::hexa(0x10b981, 0.1)), theme::hex(0x059669), emerald::E400, emerald::E300, "+"),
                "delete" => (Some(theme::hexa(0xef4444, 0.1)), red::R600, red::R400, red::R300, "-"),
                _ => (None, slate::S600, slate::S700, slate::S400, " "),
            };
            let value = l.value.strip_suffix('\n').unwrap_or(&l.value).to_string();
            rows = rows.child(
                div()
                    .flex()
                    .flex_row()
                    .when_some(bg, |d, bg| d.bg(bg))
                    .child(
                        div()
                            .w(px(48.))
                            .flex_none()
                            .pr(px(12.))
                            .py(px(1.))
                            .flex()
                            .justify_end()
                            .border_r_1()
                            .border_color(theme::hexa(0x1e293b, 0.5))
                            .text_color(gut)
                            .child(mono_text((i + 1).to_string())),
                    )
                    .child(div().w(px(20.)).flex_none().py(px(1.)).flex().justify_center().text_color(sign_c).child(mono_text(sign)))
                    .child(div().pl(px(8.)).py(px(1.)).flex_1().min_w_0().text_color(text_c).child(mono_paragraph(value).line_height(px(WEB_RELAXED_12)))),
            );
        }
        let reserve = self.diff_gutter.overflows();
        div()
            .absolute()
            .top(px(16.))
            .left(px(16.))
            .right(px(16.))
            .bottom(px(16.))
            .bg(theme::hexa(0, 0.4))
            .border_1()
            .border_color(slate::S800)
            .rounded(px(8.))
            .overflow_hidden()
            .child(
                div()
                    .id("yaml-diff-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.diff_scroll)
                    .when(reserve, |d| d.pr(px(scrollbar::WIDTH)))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(16.))
                            .py(px(8.))
                            .border_b_1()
                            .border_color(theme::hexa(0x1e293b, 0.5))
                            .child(
                                div()
                                    .text_xs_tw()
                                    .text_color(slate::S400)
                                    .child(format!("Diff: {adds} additions, {dels} deletions"))
                                    .auto_id("yaml.diff-summary", "status", format!("Diff: {adds} additions, {dels} deletions")),
                            )
                            .child(
                                div()
                                    .id("yaml-close-diff")
                                    .group("yaml-close-diff")
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_xs_tw()
                                    .text_color(slate::S500)
                                    .hover(|s| s.text_color(slate::S300))
                                    .cursor_pointer()
                                    .child(
                                        btn_icon("x", 14., slate::S500)
                                            .group_hover("yaml-close-diff", |s| s.text_color(slate::S300)),
                                    )
                                    .child("Close diff")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.diff = None;
                                        cx.notify();
                                    }))
                                    .auto_id("yaml.close-diff", "button", "Close diff"),
                            ),
                    )
                    .child(rows),
            )
            .child(div().absolute().inset_0().child(scrollbar::vertical(self.diff_scroll.clone(), self.diff_gutter.clone())))
    }
}

impl Render for YamlEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = !self.read_only;
        let changes = editing && self.has_changes(cx);
        // The editor/diff card: inset 16px on every side (fully visible).
        let card = |d: Div| {
            d.absolute()
                .top(px(16.))
                .left(px(16.))
                .right(px(16.))
                .bottom(px(16.))
                .bg(theme::hexa(0, 0.4))
                .border_1()
                .border_color(slate::S800)
                .rounded(px(8.))
                .overflow_hidden()
        };
        let body: AnyElement = if self.loading {
            card(div()).child(bars_skeleton("yaml-skel", &SKELETON)).into_any_element()
        } else if let Some(diff) = self.diff.clone() {
            self.diff_pane(&diff, cx).into_any_element()
        } else {
            card(div())
                .child(self.area.clone().auto_id("yaml.text", "text", self.yaml(cx)))
                .auto_id("yaml.editor", "textbox", "YAML")
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .child(body)
                    .when(changes && self.diff.is_none(), |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom(px(24.))
                                .right(px(24.))
                                .px(px(10.))
                                .py(px(4.))
                                .rounded_full()
                                .bg(theme::hexa(0xf59e0b, 0.1))
                                .border_1()
                                .border_color(theme::hexa(0xf59e0b, 0.3))
                                .text_10()
                                .line_height(px(15.))
                                .text_color(amber::A400)
                                .child("Unsaved changes")
                                .auto_id("yaml.unsaved", "status", "Unsaved changes"),
                        )
                    })
                    .children(self.search.clone()),
            )
            .auto_id("view.yaml", "region", "YAML")
    }
}

#[cfg(test)]
mod tests {
    use super::find_positions;

    #[test]
    fn overlapping_case_insensitive() {
        assert_eq!(find_positions("aaa", "aa"), vec![0, 1]);
        assert_eq!(find_positions("Image: x\nimage", "IMAGE"), vec![0, 9]);
        assert!(find_positions("abc", "").is_empty());
    }
}
