//! Role Browser (RbacReverseLookup, spec 03 §7.5).

use super::picker::{focus_ring, identity_icon, identity_label};
use super::rule_chain::{rule_text, PURPLE300, PURPLE400, PURPLE500};
use super::RbacView;
use crate::automation::Automation;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::text::caps;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::rbac::RoleSummary;

fn label(text: &str) -> Div {
    div().mb(px(8.)).text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S500).child(caps(text, 10.))
}

impl RbacView {
    fn role_item(&self, role: &RoleSummary, cx: &mut Context<Self>) -> impl IntoElement {
        let key = format!("{}/{}/{}", role.kind, role.namespace.clone().unwrap_or_default(), role.name);
        let selected = self.selected_role.as_deref() == Some(key.as_str());
        let r = role.clone();
        let name: AnyElement = if role.kind == "Role" {
            div()
                .flex()
                .flex_row()
                .min_w_0()
                .overflow_hidden()
                .child(div().flex_none().text_color(slate::S600).child(format!("{}/", role.namespace.clone().unwrap_or_default())))
                .child(div().truncate().child(role.name.clone()))
                .into_any_element()
        } else {
            div().truncate().child(role.name.clone()).into_any_element()
        };
        div()
            .id(SharedString::from(format!("rbac-role-{key}")))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(8.))
            .py(px(6.))
            .rounded(px(4.))
            .text_sm_tw()
            .cursor_pointer()
            .map(|d| if selected { d.bg(theme::hexa(0x334155, 0.6)) } else { d.hover(|s| s.bg(theme::hexa(0x334155, 0.4))) })
            .child(div().min_w_0().font_mono().text_xs_tw().text_color(slate::S300).child(name))
            .child(div().flex_none().ml(px(8.)).text_10().text_color(slate::S600).child(role.rule_count.to_string()))
            .on_click(cx.listener(move |this, _, _, cx| this.reverse_lookup(&r, cx)))
            .auto_id(format!("rbac.role.{key}"), "option", role.name.clone())
            .selected(selected)
    }

    pub(super) fn render_roles_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let h = f32::from(window.viewport_size().height) - 200.;
        let q = self.roles_query.to_lowercase();
        let filtered: Vec<RoleSummary> = self.roles.iter().filter(|r| r.name.to_lowercase().contains(&q)).cloned().collect();
        let cluster: Vec<&RoleSummary> = filtered.iter().filter(|r| r.kind == "ClusterRole").collect();
        let ns_roles: Vec<&RoleSummary> = filtered.iter().filter(|r| r.kind == "Role").collect();
        let focused = self.roles_filter.read(cx).is_focused(window);
        let group = |this: &Self, title: String, ic: &str, items: &[&RoleSummary], cx: &mut Context<Self>| {
            div()
                .p(px(4.))
                .child(
                    div()
                        .px(px(8.))
                        .py(px(4.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .text_10()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(slate::S500)
                        .child(icon(ic, 12.).text_color(slate::S500))
                        .child(caps(title, 10.)),
                )
                .children(items.iter().map(|r| this.role_item(r, cx)).collect::<Vec<_>>())
        };
        let left = div()
            .w(px(280.))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(slate::S800)
            .flex()
            .flex_col()
            .child(
                div().p(px(8.)).border_b_1().border_color(slate::S800).child(
                    div()
                        .relative()
                        .h(px(32.))
                        .rounded(px(4.))
                        .when(focused, |d| d.child(focus_ring(cx)))
                        .child(self.roles_filter.clone())
                        .child(div().absolute().left(px(8.)).top_0().bottom_0().flex().items_center().child(icon("search", 14.).text_color(slate::S500)))
                        .auto_id("rbac.roles.filter", "textbox", "Filter roles..."),
                ),
            )
            .child(
                div()
                    .id("rbac-roles-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(!cluster.is_empty(), |d| d.child(group(self, format!("ClusterRoles ({})", cluster.len()), "shield-check", &cluster, cx)))
                    .when(!ns_roles.is_empty(), |d| d.child(group(self, format!("Roles ({})", ns_roles.len()), "shield", &ns_roles, cx)))
                    .when(cluster.is_empty() && ns_roles.is_empty(), |d| {
                        d.child(
                            div()
                                .p(px(24.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(8.))
                                .text_sm_tw()
                                .text_color(slate::S500)
                                .child(icon("inbox", 20.).text_color(slate::S500))
                                .child("No roles found"),
                        )
                    }),
            );

        let right: AnyElement = if self.loading {
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .children((0..3).map(|_| div().h(px(48.)).rounded(px(4.)).bg(theme::hexa(0x1e293b, 0.4))))
                .into_any_element()
        } else if let Some(r) = &self.reverse {
            let rules = (!r.rules.is_empty()).then(|| {
                div().child(label(&format!("Rules ({})", r.rules.len()))).child(div().flex().flex_col().gap(px(4.)).children(r.rules.iter().enumerate().map(|(i, rule)| {
                    let text = format!("{} [{}]", rule.resources.join(", "), rule.verbs.join(", "));
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .bg(theme::hexa(PURPLE500, 0.05))
                        .border_1()
                        .border_color(theme::hexa(PURPLE500, 0.1))
                        .flex()
                        .child(rule_text(rule, theme::hex(PURPLE300), false))
                        .auto_id(format!("rbac.roles.rule.{i}"), "listitem", text)
                })))
            });
            let subjects = div().child(label(&format!("Subjects ({})", r.subjects.len()))).child(if r.subjects.is_empty() {
                div().text_sm_tw().text_color(slate::S500).italic().child("No subjects bound to this role").into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(r.subjects.iter().enumerate().map(|(i, s)| {
                        let (ic, c) = identity_icon(&s.identity);
                        let name = identity_label(&s.identity);
                        let ident = s.identity.clone();
                        div()
                            .id(("rbac-subject", i))
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(4.))
                            .bg(theme::hexa(0x1e293b, 0.3))
                            .border_1()
                            .border_color(slate::S800)
                            .hover(|st| st.bg(theme::hexa(0x334155, 0.4)))
                            .cursor_pointer()
                            .child(icon(ic, 14.).text_color(c))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().text_xs_tw().font_mono().text_color(slate::S200).truncate().child(name.clone()))
                                    .child(
                                        div()
                                            .text_10()
                                            .text_color(slate::S500)
                                            .child(format!("via {} \"{}\" ({})", s.binding_kind, s.binding_name, s.scope))
                                            .auto_id(
                                                format!("rbac.subject.{name}.via"),
                                                "text",
                                                format!("via {} \"{}\" ({})", s.binding_kind, s.binding_name, s.scope),
                                            ),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.select_identity(ident.clone(), cx)))
                            .auto_id(format!("rbac.subject.{name}"), "button", name)
                    }))
                    .into_any_element()
            });
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(icon("shield-check", 16.).text_color(theme::hex(PURPLE400)))
                        .child(div().text_sm_tw().font_mono().text_color(slate::S200).child(format!("{}/{}", r.role_kind, r.role_name)))
                        .when_some(r.role_namespace.clone(), |d, ns| d.child(div().text_xs_tw().text_color(slate::S500).child(format!("(ns: {ns})"))))
                        .auto_id(
                            "rbac.roles.detail",
                            "heading",
                            format!(
                                "{}/{}{}",
                                r.role_kind,
                                r.role_name,
                                r.role_namespace.as_ref().map(|ns| format!(" (ns: {ns})")).unwrap_or_default()
                            ),
                        ),
                )
                .children(rules)
                .child(subjects)
                .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm_tw()
                .text_color(slate::S500)
                .child("Select a role to see who holds it")
                .into_any_element()
        };
        div()
            .h(px(h))
            .flex()
            .flex_row()
            .border_1()
            .border_color(slate::S800)
            .rounded(px(8.))
            .overflow_hidden()
            .child(left)
            .child(div().id("rbac-roles-detail").flex_1().min_w_0().h_full().overflow_y_scroll().p(px(16.)).child(right))
            .into_any_element()
    }
}
