//! Query tab (spec 03 §7.6): Check Permission / Who Can? with verb and
//! resource selects.

use super::picker::identity_label;
use super::rule_chain::rule_chain;
use super::{QueryMode, RbacView, SelectId, RESOURCES, VERBS};
use crate::automation::Automation;
use crate::backend;
use crate::theme::{self, slate, KoreStyled, SANS};
use crate::ui::icon::icon;
use crate::ui::text::measure;
use crate::ui::widgets::spinning_icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;

fn fade_up8(id: &'static str, el: Div) -> AnyElement {
    el.with_animation(id, Animation::new(Duration::from_millis(300)).with_easing(ease_out_quint()), |el, t| {
        el.opacity(t).mt(px(8. * (1. - t)))
    })
    .into_any_element()
}

impl RbacView {
    fn set_mode(&mut self, mode: QueryMode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.query_error = None;
        self.query_result = None;
        self.who_can.clear();
        self.query_ran = false;
        cx.notify();
    }

    fn query_failed(&mut self, msg: String, cx: &mut Context<Self>) {
        self.forbidden.update(cx, |b, cx| b.set_error(Some(&msg), cx));
        self.query_error = Some(msg);
    }

    fn submit_query(&mut self, cx: &mut Context<Self>) {
        let disabled = self.query_loading || (self.mode == QueryMode::Check && self.query_identity.is_none());
        if disabled {
            return;
        }
        self.query_loading = true;
        self.query_error = None;
        self.query_result = None;
        self.who_can.clear();
        let (verb, res, ns) = (self.verb.clone(), self.resource.clone(), self.namespace.clone());
        let t = match self.mode {
            QueryMode::Check => {
                let ident = self.query_identity.clone().unwrap_or(kore_core::state::rbac::RbacIdentity::User { name: String::new() });
                backend::run(cx, move |c| async move { c.rbac_check_permission(ident, verb, res, ns).await }, |this: &mut Self, r, cx| {
                    match r {
                        Ok(r) => {
                            this.query_result = Some(r);
                            this.query_ran = true;
                        }
                        Err(e) => this.query_failed(backend::format_error(&e), cx),
                    }
                    this.query_loading = false;
                    cx.notify();
                })
            }
            QueryMode::WhoCan => backend::run(cx, move |c| async move { c.rbac_who_can(verb, res, ns).await }, |this: &mut Self, r, cx| {
                match r {
                    Ok(r) => {
                        this.who_can = r;
                        this.query_ran = true;
                    }
                    Err(e) => this.query_failed(backend::format_error(&e), cx),
                }
                this.query_loading = false;
                cx.notify();
            }),
        };
        self.tasks.insert("query", t);
        cx.notify();
    }

    /// Styled native popup select: 20px tall, width of the
    /// longest option + padding and the up/down arrows.
    fn render_select(&self, which: SelectId, value: &str, options: &'static [&'static str], window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let longest = options.iter().map(|o| measure(window, o, SANS, 12., FontWeight::NORMAL)).fold(0., f32::max);
        let width = longest + 32.5;
        let open = self.select_open == Some(which);
        let id = if which == SelectId::Verb { "verb" } else { "resource" };
        let menu = open.then(|| {
            let current = value.to_string();
            deferred(
                div()
                    .id(SharedString::from(format!("rbac-select-menu-{id}")))
                    .absolute()
                    .top(px(22.))
                    .left_0()
                    .min_w(px(width))
                    .max_h(px(400.))
                    .overflow_y_scroll()
                    .py(px(4.))
                    .rounded(px(6.))
                    .bg(theme::hex(0x1f2633))
                    .border_1()
                    .border_color(theme::hexa(0xffffff, 0.12))
                    .shadow_lg()
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.select_open = None;
                        cx.notify();
                    }))
                    .children(options.iter().map(|o| {
                        let o: &'static str = o;
                        let sel = *o == current;
                        div()
                            .id(SharedString::from(format!("rbac-opt-{id}-{o}")))
                            .px(px(10.))
                            .py(px(2.))
                            .mx(px(4.))
                            .rounded(px(4.))
                            .text_size(px(13.))
                            .text_color(slate::S200)
                            .hover(|s| s.bg(theme::hex(0x2563eb)).text_color(theme::hex(0xffffff)))
                            .cursor_pointer()
                            .child(format!("{}{o}", if sel { "✓ " } else { "   " }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                match which {
                                    SelectId::Verb => this.verb = o.to_string(),
                                    SelectId::Resource => this.resource = o.to_string(),
                                }
                                this.select_open = None;
                                cx.notify();
                            }))
                            .auto_id(format!("rbac.select.{id}.{o}"), "option", o)
                            .selected(sel)
                    })),
            )
            .with_priority(2)
        });
        div()
            .relative()
            .flex_none()
            .child(
                div()
                    .id(SharedString::from(format!("rbac-select-{id}")))
                    .w(px(width))
                    .h(px(20.))
                    .pl(px(8.))
                    .pr(px(6.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .rounded(px(5.))
                    .bg(theme::SURFACE)
                    .border_1()
                    .border_color(theme::hexa(0x334155, 0.5))
                    .text_xs_tw()
                    .text_color(slate::S200)
                    .cursor_pointer()
                    .child(value.to_string())
                    .child(icon("chevrons-up-down", 12.).text_color(slate::S200))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_open = if this.select_open == Some(which) { None } else { Some(which) };
                        cx.notify();
                    }))
                    .auto_id(format!("rbac.select.{id}"), "combobox", value.to_string()),
            )
            .children(menu)
    }

    pub(super) fn render_query_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let mode_btn = |label: &'static str, mode: QueryMode, active: bool, cx: &mut Context<Self>| {
            div()
                .id(label)
                .px(px(12.))
                .py(px(6.))
                .text_xs_tw()
                .cursor_pointer()
                .when(mode == QueryMode::WhoCan, |d| d.border_l_1().border_color(theme::hexa(0x334155, 0.5)))
                .map(|d| {
                    if active {
                        d.bg(theme::hexa(0x334155, 0.6)).text_color(slate::S200)
                    } else {
                        d.text_color(slate::S500).hover(|s| s.text_color(slate::S300).bg(theme::hexa(0x1e293b, 0.5)))
                    }
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)))
                .auto_id(format!("rbac.query.mode.{}", if mode == QueryMode::Check { "check" } else { "whocan" }), "button", label)
                .selected(active)
        };
        let disabled = self.query_loading || (self.mode == QueryMode::Check && self.query_identity.is_none());
        let (verb, resource) = (self.verb.clone(), self.resource.clone());
        let controls = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme::hexa(0x334155, 0.5))
                    .overflow_hidden()
                    .child(mode_btn("Check Permission", QueryMode::Check, self.mode == QueryMode::Check, cx))
                    .child(mode_btn("Who Can?", QueryMode::WhoCan, self.mode == QueryMode::WhoCan, cx)),
            )
            .when(self.mode == QueryMode::Check, |d| d.child(self.query_picker.clone()))
            .child(self.render_select(SelectId::Verb, &verb, &VERBS, window, cx))
            .child(self.render_select(SelectId::Resource, &resource, &RESOURCES, window, cx))
            .when_some(self.namespace.clone(), |d, ns| {
                d.child(
                    div()
                        .flex_none()
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .bg(theme::hexa(0x1e293b, 0.5))
                        .text_10()
                        .font_mono()
                        .text_color(slate::S500)
                        .child(format!("ns: {ns}"))
                        .auto_id("rbac.query.namespace", "text", format!("ns: {ns}")),
                )
            })
            .child(
                div()
                    .id("rbac-check")
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .text_xs_tw()
                    .font_weight(FontWeight::MEDIUM)
                    .map(|d| {
                        if disabled {
                            d.bg(theme::hexa(0x334155, 0.3)).text_color(slate::S600)
                        } else {
                            d.border_1().border_color(theme::PREFLIGHT_BORDER).text_color(accent).cursor_pointer()
                        }
                    })
                    .child(if self.query_loading {
                        spinning_icon("rbac-check-spin", "loader-2", 14., slate::S600).into_any_element()
                    } else {
                        icon("play", 14.).text_color(if disabled { slate::S600 } else { accent }).into_any_element()
                    })
                    .child("Check")
                    .on_click(cx.listener(|this, _, _, cx| this.submit_query(cx)))
                    .auto_id("rbac.query.check", "button", "Check")
                    .disabled(disabled),
            );

        let ns_suffix = self.namespace.as_ref().map(|n| format!(" in {n}")).unwrap_or_default();
        let mut col = div().flex().flex_col().gap(px(16.)).child(controls);
        if self.query_loading {
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .text_sm_tw()
                    .text_color(slate::S500)
                    .child(spinning_icon("rbac-q-loading", "loader-2", 16., slate::S500))
                    .child("Checking permissions..."),
            );
        }
        if let Some(err) = self.query_error.clone() {
            col = col.child(fade_up8(
                "rbac-q-error",
                div()
                    .max_w(px(672.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme::hexa(0xef4444, 0.2))
                    .bg(theme::hexa(0xef4444, 0.05))
                    .p(px(16.))
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(8.))
                    .child(div().flex_none().mt(px(2.)).child(icon("alert-circle", 14.).text_color(theme::red::R400)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).text_color(theme::red::R300).child("Query failed"))
                            .child(div().mt(px(4.)).text_xs_tw().text_color(slate::S400).child(err.clone()))
                            .auto_id("rbac.query.error", "alert", err),
                    ),
            ));
        }
        if let Some(r) = &self.query_result {
            let (border, bg, title_c) = if r.allowed {
                (theme::hexa(0x10b981, 0.2), theme::hexa(0x10b981, 0.05), theme::hex(0x6ee7b7))
            } else {
                (theme::hexa(0xef4444, 0.2), theme::hexa(0xef4444, 0.05), theme::hex(0xfca5a5))
            };
            col = col.child(fade_up8(
                "rbac-check-result",
                div()
                    .max_w(px(672.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .p(px(16.))
                    .child(
                        div()
                            .text_sm_tw()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(title_c)
                            .mb(px(8.))
                            .child(if r.allowed { "Allowed" } else { "Denied" }),
                    )
                    .child(div().text_xs_tw().text_color(slate::S400).mb(px(12.)).child(r.summary.clone()).auto_id("rbac.query.summary", "text", r.summary.clone()))
                    .child(rule_chain("rbac-q-chain", "rbac.query.chain", &r.rule_chain, r.allowed))
                    .child(div().auto_id("rbac.query.result", "status", if r.allowed { "Allowed" } else { "Denied" })),
            ));
        }
        if !self.who_can.is_empty() {
            let max_h = f32::from(window.viewport_size().height) - 320.;
            let whocan_gutter = self.whocan_gutter.overflows();
            col = col.child(fade_up8(
                "rbac-whocan",
                div()
                    .max_w(px(672.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_xs_tw()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(slate::S500)
                            .child(format!("{} identities can {} {}{}:", self.who_can.len(), self.verb, self.resource, ns_suffix))
                            .auto_id(
                                "rbac.whocan.title",
                                "heading",
                                format!("{} identities can {} {}{}:", self.who_can.len(), self.verb, self.resource, ns_suffix),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_row()
                            .when(whocan_gutter, |d| d.child(div().absolute().top_0().bottom_0().right_0().w(px(crate::ui::scrollbar::WIDTH))))
                            .child(crate::ui::scrollbar::vertical(self.whocan_scroll.clone(), self.whocan_gutter.clone()))
                            .child(
                        div()
                            .id("rbac-whocan-list")
                            .flex_1()
                            .min_w_0()
                            .max_h(px(max_h))
                            .overflow_y_scroll()
                            .track_scroll(&self.whocan_scroll)
                            .when(whocan_gutter, |d| d.mr(px(crate::ui::scrollbar::WIDTH)))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .pr(px(4.))
                            .children(self.who_can.iter().enumerate().map(|(i, r)| {
                                let name = identity_label(&r.identity);
                                let ident = r.identity.clone();
                                div()
                                    .id(("rbac-whocan-row", i))
                                    .w_full()
                                    .flex_none()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .px(px(12.))
                                    .py(px(8.))
                                    .rounded(px(4.))
                                    .bg(theme::hexa(0x10b981, 0.05))
                                    .border_1()
                                    .border_color(theme::hexa(0x10b981, 0.15))
                                    .hover(|s| s.bg(theme::hexa(0x10b981, 0.1)))
                                    .cursor_pointer()
                                    .child(div().size(px(8.)).flex_none().rounded_full().bg(theme::hex(0x34d399)))
                                    .child(div().text_xs_tw().font_mono().text_color(theme::hex(0xa7f3d0)).child(name.clone()))
                                    .child(
                                        div()
                                            .ml_auto()
                                            .text_10()
                                            .text_color(slate::S500)
                                            .child(format!("via {}", r.rule_chain.first().map(|e| e.role_name.clone()).unwrap_or_default()))
                                            .auto_id(
                                                format!("rbac.whocan.{name}.via"),
                                                "text",
                                                format!("via {}", r.rule_chain.first().map(|e| e.role_name.clone()).unwrap_or_default()),
                                            ),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| this.select_identity(ident.clone(), cx)))
                                    .auto_id(format!("rbac.whocan.{name}"), "button", name)
                            })),
                            ),
                    ),
            ));
        }
        if self.query_ran && !self.query_loading && self.mode == QueryMode::WhoCan && self.who_can.is_empty() {
            col = col.child(
                div()
                    .text_sm_tw()
                    .text_color(slate::S500)
                    .italic()
                    .child(format!("No identities can {} {}{}.", self.verb, self.resource, ns_suffix)),
            );
        }
        col.into_any_element()
    }
}
