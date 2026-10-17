//! DeploymentRollback (spec 02 §6): revision timeline (ReplicaSets by
//! ownerReference), expandable details, inline ReplicaSet YAML viewer and
//! rollback with confirmation. Fix 16: kore-core's rollback/restart now patch
//! correctly, so the confirm actually rolls back.

use crate::automation::Automation;
use crate::backend;
use crate::state::app_state::{ConfirmRequest, ConfirmVariant};
use crate::state::AppState;
use crate::theme::{self, amber, emerald, red, slate, KoreStyled};
use crate::ui::scrollbar::{self, Gutter};
use crate::ui::text::caps;
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::{skeleton_bar, spinning_icon};
use crate::views::detail_ui::{btn_icon, button, time_ago, BtnStyle};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use crate::views::yaml_text_area::LINE_H as WEB_RELAXED_12;
use kore_core::state::rollback::DeploymentRevision;
use regex::Regex;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

/// Token classes of the rollback YAML viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YTok {
    Plain,
    Key,
    Comment,
    Str,
    Num,
    Bool,
    Null,
    Dash,
}

/// YAML syntax highlighting of the rollback diff, as (text, class) segments.
pub fn highlight_yaml(yaml: &str) -> Vec<(String, YTok)> {
    static KV: OnceLock<Regex> = OnceLock::new();
    static LIST: OnceLock<Regex> = OnceLock::new();
    static COMMENT: OnceLock<Regex> = OnceLock::new();
    static QUOTED: OnceLock<Regex> = OnceLock::new();
    static NUM: OnceLock<Regex> = OnceLock::new();
    let kv = KV.get_or_init(|| Regex::new(r"^(\s*)([\w.-]+)(:)(.*)").unwrap());
    let list = LIST.get_or_init(|| Regex::new(r"^(\s*)(- )(.*)").unwrap());
    let comment = COMMENT.get_or_init(|| Regex::new(r"^\s*#").unwrap());
    let quoted = QUOTED.get_or_init(|| Regex::new(r#"^["'].*["']$"#).unwrap());
    let num = NUM.get_or_init(|| Regex::new(r"^\d+(\.\d+)?$").unwrap());
    let mut out: Vec<(String, YTok)> = Vec::new();
    let push = |s: &str, t: YTok, out: &mut Vec<(String, YTok)>| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((p, pt)) if *pt == t => p.push_str(s),
            _ => out.push((s.to_string(), t)),
        }
    };
    for (i, line) in yaml.split('\n').enumerate() {
        if i > 0 {
            push("\n", YTok::Plain, &mut out);
        }
        if comment.is_match(line) {
            push(line, YTok::Comment, &mut out);
        } else if let Some(c) = kv.captures(line) {
            push(&c[1], YTok::Plain, &mut out);
            push(&c[2], YTok::Key, &mut out);
            push(&c[3], YTok::Plain, &mut out);
            let value = &c[4];
            let trimmed = value.trim();
            let tok = if quoted.is_match(trimmed) {
                Some(YTok::Str)
            } else if num.is_match(trimmed) {
                Some(YTok::Num)
            } else if trimmed == "true" || trimmed == "false" {
                Some(YTok::Bool)
            } else if trimmed == "null" || trimmed == "~" {
                Some(YTok::Null)
            } else if !trimmed.is_empty() {
                Some(YTok::Str)
            } else {
                None
            };
            match tok {
                Some(t) => {
                    push(" ", YTok::Plain, &mut out);
                    push(trimmed, t, &mut out);
                }
                None => push(value, YTok::Plain, &mut out),
            }
        } else if let Some(c) = list.captures(line) {
            push(&c[1], YTok::Plain, &mut out);
            push(&c[2], YTok::Dash, &mut out);
            push(&c[3], YTok::Str, &mut out);
        } else {
            push(line, YTok::Plain, &mut out);
        }
    }
    out
}

fn ytok_color(t: YTok, accent: Rgba) -> Rgba {
    match t {
        YTok::Plain => slate::S100,
        YTok::Key => accent,
        YTok::Comment | YTok::Null | YTok::Dash => slate::S500,
        YTok::Str => emerald::E400,
        YTok::Num => amber::A400,
        YTok::Bool => theme::hex(0x818cf8),
    }
}

/// `new Date(ts).toLocaleString()` (en-GB style as rendered: `15/10/2026, 13:41:36`).
pub fn locale_string(ts: &str) -> String {
    match crate::state::resource_item::parse_ts(ts) {
        Some(d) => d.with_timezone(&chrono::Local).format("%d/%m/%Y, %H:%M:%S").to_string(),
        None => "Invalid Date".into(),
    }
}

pub struct DeploymentRollback {
    app: Entity<AppState>,
    namespace: String,
    name: String,
    revisions: Vec<DeploymentRevision>,
    loading: bool,
    error: Option<String>,
    expanded: Option<i64>,
    yaml_target: Option<String>,
    yaml_content: String,
    yaml_loading: bool,
    /// Bumped on every fetch so the entry animation replays.
    generation: u64,
    scroll: ScrollHandle,
    gutter: Gutter,
    yaml_scroll: ScrollHandle,
    yaml_gutter: Gutter,
    _load: Option<Task<()>>,
    _yaml: Option<Task<()>>,
    _poll: Option<Task<()>>,
}

impl DeploymentRollback {
    pub fn new(app: Entity<AppState>, namespace: String, name: String, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = DeploymentRollback {
            app,
            namespace,
            name,
            revisions: Vec::new(),
            loading: true,
            error: None,
            expanded: None,
            yaml_target: None,
            yaml_content: String::new(),
            yaml_loading: false,
            generation: 0,
            scroll: ScrollHandle::new(),
            gutter: Gutter::default(),
            yaml_scroll: ScrollHandle::new(),
            yaml_gutter: Gutter::default(),
            _load: None,
            _yaml: None,
            _poll: None,
        };
        this.fetch(cx);
        this
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        self.fetch_expecting(None, 0, cx);
    }

    /// Fetch revisions. After a rollback, `expect` is the restored ReplicaSet:
    /// the deployment controller renumbers it to the top revision a moment
    /// after the patch, so poll (every 500ms, up to ~10s) until the list
    /// reflects that instead of showing the pre-rollback order.
    fn fetch_expecting(&mut self, expect: Option<String>, tries: u32, cx: &mut Context<Self>) {
        if tries == 0 {
            self.loading = true;
        }
        self.error = None;
        cx.notify();
        let (ns, name) = (self.namespace.clone(), self.name.clone());
        self._load = Some(backend::run(
            cx,
            move |core| async move { core.list_deployment_revisions(ns, name).await },
            move |this: &mut Self, res, cx| {
                this.loading = false;
                if tries == 0 {
                    this.generation += 1;
                }
                let mut settled = true;
                match res {
                    Ok(mut v) => {
                        v.sort_by_key(|r| std::cmp::Reverse(r.revision));
                        if let Some(rs) = &expect {
                            settled = v.first().map(|r| &r.name == rs).unwrap_or(true);
                        }
                        this.revisions = v;
                    }
                    Err(e) => this.error = Some(backend::format_error(&e)),
                }
                if !settled && tries < 20 {
                    let t = backend::sleep(cx, std::time::Duration::from_millis(500));
                    this._poll = Some(cx.spawn(async move |this, cx| {
                        t.await;
                        this.update(cx, |r, cx| r.fetch_expecting(expect, tries + 1, cx)).ok();
                    }));
                }
                cx.notify();
            },
        ));
    }

    fn view_yaml(&mut self, rs: String, cx: &mut Context<Self>) {
        if self.yaml_target.as_deref() == Some(rs.as_str()) {
            self.yaml_target = None;
            self.yaml_content.clear();
            cx.notify();
            return;
        }
        self.yaml_target = Some(rs.clone());
        self.yaml_loading = true;
        cx.notify();
        let ns = self.namespace.clone();
        self._yaml = Some(backend::run(
            cx,
            move |core| async move { core.get_revision_yaml(ns, rs).await },
            |this: &mut Self, res, cx| {
                this.yaml_loading = false;
                this.yaml_content = match res {
                    Ok(y) => y,
                    Err(e) => format!("Error fetching YAML: {}", backend::format_error(&e)),
                };
                cx.notify();
            },
        ));
    }

    fn request_rollback(&mut self, rev: DeploymentRevision, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let app = self.app.clone();
        let req = rollback_request(app.clone(), this, self.namespace.clone(), self.name.clone(), rev, false);
        app.update(cx, |s, cx| s.confirm(req, cx));
    }

    fn row(&self, i: usize, rev: &DeploymentRevision, mono_size: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let current = i == 0;
        let expanded = self.expanded == Some(rev.revision);
        let revision = rev.revision;
        let gid: SharedString = format!("rev-{revision}").into();
        let main = div()
            .id(ElementId::Name(format!("rev-row-{revision}").into()))
            .w_full()
            .px(px(12.))
            .py(px(10.))
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .child(icon_c(if expanded { "chevron-down" } else { "chevron-right" }, 14., slate::S500))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .flex_none()
                    .child(icon_c("hash", 12., slate::S500))
                    .child(
                        div()
                            .font_mono()
                            .text_xs_tw()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if current { accent } else { slate::S300 })
                            .child(revision.to_string()),
                    )
                    .when(current, |d| {
                        d.child(
                            div()
                                .text_9()
                                .line_height(px(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(accent)
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(4.))
                                .child(caps("current", 9.)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .min_w_0()
                    .flex_1()
                    .child(icon_c("container", 12., slate::S500))
                    .child(
                        div()
                            .text_xs_tw()
                            .text_color(slate::S400)
                            .font_mono()
                            .truncate()
                            .child(if rev.image.is_empty() { "-".to_string() } else { rev.image.clone() }),
                    ),
            )
            .child(div().text_10().line_height(px(15.)).text_color(slate::S500).flex_none().child(time_ago(Some(&rev.created))))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.expanded = if this.expanded == Some(revision) { None } else { Some(revision) };
                cx.notify();
            }))
            .auto_id(format!("rollback.rev.{revision}"), "button", rev.image.clone())
            .selected(current);

        let card = div()
            .id(ElementId::Name(gid.clone()))
            .border_1()
            .rounded(px(8.))
            .map(|d| {
                if current {
                    d.border_color(theme::PREFLIGHT_BORDER)
                } else {
                    d.border_color(slate::S800)
                        .bg(theme::hexa(0x101828, 0.3))
                        .hover(|s| s.border_color(slate::S700).bg(theme::hexa(0x101828, 0.5)))
                }
            })
            .child(main)
            .when(expanded, |d| d.child(self.expanded_section(rev, current, mono_size, cx)));

        let delay = i as f32 * 0.03;
        let total = 0.2 + delay;
        div()
            .relative()
            .pl(px(36.))
            .child(
                div()
                    .absolute()
                    .left(px(11.))
                    .top(px(14.))
                    .size(px(9.))
                    .rounded_full()
                    .border_2()
                    .map(|d| if current { d.border_color(accent) } else { d.border_color(slate::S600).bg(theme::SURFACE) }),
            )
            .child(card)
            .with_animation(
                ElementId::Name(format!("rev-anim-{}-{revision}", self.generation).into()),
                Animation::new(Duration::from_secs_f32(total)),
                move |d, t| {
                    let local = ((t * total - delay) / 0.2).clamp(0., 1.);
                    d.opacity(local).top(px(12. * (1. - local)))
                },
            )
    }

    fn expanded_section(&self, rev: &DeploymentRevision, current: bool, mono_size: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let cell = |label: &str, ic: &str, value: (Div, String)| {
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(div().text_10().line_height(px(15.)).text_color(slate::S500).mb(px(2.)).child(caps(label, 10.)))
                .child(div().flex().flex_row().items_center().gap(px(6.)).child(icon_c(ic, 12., slate::S500)).child(value.0))
                .auto_id(format!("rollback.cell.{}", label.to_lowercase().replace(' ', "-")), "text", value.1)
        };
        let mono = |s: String| (div().text_xs_tw().text_color(slate::S200).font_mono().min_w_0().child(s.clone()), s);
        let showing = self.yaml_target.as_deref() == Some(rev.name.as_str());
        let rs = rev.name.clone();
        let rev2 = rev.clone();
        div()
            .px(px(12.))
            .pb(px(12.))
            .pt(px(4.))
            .border_t_1()
            .border_color(theme::hexa(0x1e293b, 0.5))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_x(px(16.))
                    .gap_y(px(8.))
                    .mb(px(12.))
                    .child(cell("Image", "container", mono(if rev.image.is_empty() { "-".into() } else { rev.image.clone() })))
                    .child(cell("Created", "clock", mono(locale_string(&rev.created))))
                    .child(cell("Replicas", "users", mono(rev.replicas.to_string())))
                    .child(cell("Change Cause", "message-square", {
                        let cause = if rev.change_cause.is_empty() { "No annotation".to_string() } else { rev.change_cause.clone() };
                        (div().text_xs_tw().text_color(slate::S300).min_w_0().child(cause.clone()), cause)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        button(
                            format!("rev-yaml-{}", rev.revision),
                            if showing { BtnStyle::Active } else { BtnStyle::Standard },
                            cx,
                        )
                        .px(px(10.))
                        .py(px(6.))
                        .text_xs_tw()
                        .child(btn_icon("file-code-2", 14., if showing { accent } else { slate::S300 }))
                        .child(if showing { "Hide YAML" } else { "View YAML" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.view_yaml(rs.clone(), cx)
                        }))
                        .auto_id(format!("rollback.yaml.{}", rev.revision), "button", if showing { "Hide YAML" } else { "View YAML" }),
                    )
                    .when(!current, |d| {
                        d.child(
                            div()
                                .id(ElementId::Name(format!("rev-rollback-{}", rev.revision).into()))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .px(px(10.))
                                .py(px(6.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(theme::hexa(0x92400e, 0.5))
                                .text_color(amber::A400)
                                .text_xs_tw()
                                .cursor_pointer()
                                .hover(|s| s.border_color(theme::hex(0xd97706)).bg(theme::hexa(0xf59e0b, 0.1)))
                                .child(btn_icon("rotate-ccw", 14., amber::A400))
                                .child("Rollback")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.request_rollback(rev2.clone(), cx)
                                }))
                                .auto_id(format!("rollback.rollback.{}", rev.revision), "button", "Rollback"),
                        )
                    }),
            )
            .when(showing, |d| d.child(self.yaml_viewer(mono_size, cx)))
    }

    fn yaml_viewer(&self, mono_size: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let icon_btn = |id: &'static str, ic: &'static str| {
            div()
                .id(id)
                .group(id)
                .p(px(4.))
                .cursor_pointer()
                .child(icon(ic, 12.).text_color(slate::S500).group_hover(id, |s| s.text_color(slate::S300)))
        };
        let body: AnyElement = if self.yaml_loading {
            div()
                .flex()
                .items_center()
                .justify_center()
                .py(px(32.))
                .child(spinning_icon("rev-yaml-spin", "loader-2", 16., accent))
                .into_any_element()
        } else {
            let mono = font(SharedString::from(theme::MONO));
            let segs = highlight_yaml(&self.yaml_content);
            let mut text = String::new();
            let runs: Vec<TextRun> = segs
                .iter()
                .map(|(seg, t)| {
                    text.push_str(seg);
                    TextRun {
                        len: seg.len(),
                        font: mono.clone(),
                        color: ytok_color(*t, accent).into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }
                })
                .collect();
            let reserve = self.yaml_gutter.overflows();
            div()
                .relative()
                .max_h(px(256.))
                .child(
                    div()
                        .id("rev-yaml-scroll")
                        .max_h(px(256.))
                        .overflow_y_scroll()
                        .track_scroll(&self.yaml_scroll)
                        .p(px(12.))
                        .when(reserve, |d| d.pr(px(12. + scrollbar::WIDTH)))
                        .font_mono()
                        .text_size(px(mono_size))
                        .line_height(px(WEB_RELAXED_12))
                        .text_color(slate::S100)
                        .child(StyledText::new(text).with_runs(runs)),
                )
                .child(div().absolute().inset_0().child(scrollbar::vertical(self.yaml_scroll.clone(), self.yaml_gutter.clone())))
                .into_any_element()
        };
        div()
            .mt(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0, 0.4))
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(slate::S800)
                    .bg(theme::hexa(0x101828, 0.3))
                    .child(
                        div()
                            .text_10()
                            .line_height(px(15.))
                            .text_color(slate::S500)
                            .child(caps("ReplicaSet YAML", 10.))
                            .auto_id("rollback.yaml-viewer", "document", self.yaml_content.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .child(icon_btn("rev-yaml-copy", "copy").on_click(cx.listener(|this, _, _, cx| {
                                let y = this.yaml_content.clone();
                                cx.write_to_clipboard(ClipboardItem::new_string(y));
                                toast(cx, "YAML copied to clipboard", ToastKind::Success);
                            })))
                            .child(icon_btn("rev-yaml-close", "x").on_click(cx.listener(|this, _, _, cx| {
                                this.yaml_target = None;
                                this.yaml_content.clear();
                                cx.notify();
                            }))),
                    ),
            )
            .child(body)
    }
}

fn icon_c(name: &str, size: f32, color: Rgba) -> Svg {
    crate::ui::icon::icon(name, size).text_color(color)
}

use crate::ui::icon::icon;

/// The rollback ConfirmDialog. `busy` shows `Rolling back...`.
fn rollback_request(
    app: Entity<AppState>,
    this: WeakEntity<DeploymentRollback>,
    ns: String,
    name: String,
    rev: DeploymentRevision,
    busy: bool,
) -> ConfirmRequest {
    let msg = format!(
        "Roll back \"{name}\" to revision {}? This will replace the current pod template with the one from this revision.",
        rev.revision
    );
    let (app2, this2, ns2, name2, rev2) = (app.clone(), this.clone(), ns.clone(), name.clone(), rev.clone());
    ConfirmRequest {
        title: "Rollback Deployment".into(),
        message: msg,
        confirm_label: if busy { "Rolling back...".into() } else { "Rollback".into() },
        cancel_label: "Cancel".into(),
        variant: ConfirmVariant::Warning,
        on_confirm: Rc::new(move |_, cx| {
            if busy {
                return;
            }
            let busy_req = rollback_request(app2.clone(), this2.clone(), ns2.clone(), name2.clone(), rev2.clone(), true);
            app2.update(cx, |s, cx| s.confirm(busy_req, cx));
            let core = backend::core(cx);
            let (app, this, ns, name, rev) = (app2.clone(), this2.clone(), ns2.clone(), name2.clone(), rev2.clone());
            cx.spawn(async move |cx| {
                let res = core.rollback_deployment(ns.clone(), name.clone(), rev.name.clone()).await;
                cx.update(|cx| match res {
                    Ok(()) => {
                        toast(cx, format!("Rolled back to revision {}", rev.revision), ToastKind::Success);
                        app.update(cx, |s, cx| s.close_confirm(cx));
                        this.update(cx, |r, cx| r.fetch_expecting(Some(rev.name.clone()), 0, cx)).ok();
                    }
                    Err(e) => {
                        toast(cx, backend::format_error(&e), ToastKind::Error);
                        let req = rollback_request(app.clone(), this.clone(), ns, name, rev, false);
                        app.update(cx, |s, cx| {
                            if s.confirm.is_some() {
                                s.confirm(req, cx)
                            }
                        });
                    }
                });
            })
            .detach();
        }),
    }
}

impl Render for DeploymentRollback {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let mono_size = crate::views::describe_content::pitched_mono_size(window, 12.);
        if self.loading {
            let mut d = div().p(px(16.)).flex().flex_col().gap(px(12.));
            for i in 0..3 {
                d = d.child(
                    div().opacity(1. - i as f32 * 0.2).child(
                        div().rounded(px(8.)).overflow_hidden().child(skeleton_bar(
                            ElementId::Name(format!("rev-skel-{i}").into()),
                            relative(1.),
                            80.,
                        )),
                    ),
                );
            }
            return d.into_any_element();
        }
        if let Some(err) = &self.error {
            return div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .p(px(16.))
                .text_sm_tw()
                .text_color(red::R400)
                .child(icon_c("alert-circle", 16., red::R400))
                .child(format!("Failed to load revisions: {err}"))
                .into_any_element();
        }
        if self.revisions.is_empty() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .py(px(48.))
                .text_color(slate::S400)
                .child(icon_c("history", 32., slate::S600))
                .child(div().text_sm_tw().child("No revisions found"))
                .into_any_element();
        }
        let revisions = self.revisions.clone();
        let mut list = div().flex().flex_col().gap(px(8.));
        for (i, r) in revisions.iter().enumerate() {
            list = list.child(self.row(i, r, mono_size, cx));
        }
        let reserve = self.gutter.overflows();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(16.))
                    .pt(px(16.))
                    .pb(px(8.))
                    .flex_none()
                    .child(icon_c("history", 16., accent))
                    .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).text_color(slate::S200).child("Revision History"))
                    .child(
                        div()
                            .text_10()
                            .line_height(px(15.))
                            .bg(slate::S800)
                            .text_color(slate::S400)
                            .px(px(6.))
                            .py(px(2.))
                            .rounded_full()
                            .child(revisions.len().to_string()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("rollback-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .px(px(16.))
                            .pb(px(16.))
                            .when(reserve, |d| d.pr(px(16. + scrollbar::WIDTH)))
                            .child(
                                div()
                                    .relative()
                                    .child(
                                        div().absolute().left(px(15.)).top(px(8.)).bottom(px(8.)).w(px(1.)).bg(slate::S800),
                                    )
                                    .child(list),
                            ),
                    )
                    .child(div().absolute().inset_0().child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone()))),
            )
            .auto_id("view.rollback", "region", "Rollback")
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{highlight_yaml, YTok};

    #[test]
    fn yaml_tokens() {
        let segs = highlight_yaml("# c\nkind: ReplicaSet\n  n: 3\nmetadata:\n- name: x\nok: true\nz: ~");
        assert_eq!(segs[0], ("# c".to_string(), YTok::Comment));
        assert!(segs.contains(&("kind".to_string(), YTok::Key)));
        assert!(segs.contains(&("ReplicaSet".to_string(), YTok::Str)));
        assert!(segs.contains(&("3".to_string(), YTok::Num)));
        assert!(segs.contains(&("- ".to_string(), YTok::Dash)));
        assert!(segs.contains(&("name: x".to_string(), YTok::Str)));
        assert!(segs.contains(&("true".to_string(), YTok::Bool)));
        assert!(segs.contains(&("~".to_string(), YTok::Null)));
        let text: String = segs.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(text, "# c\nkind: ReplicaSet\n  n: 3\nmetadata:\n- name: x\nok: true\nz: ~");
    }
}
