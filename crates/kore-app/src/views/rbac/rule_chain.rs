//! RbacRuleChain (spec 03 §7.4): binding → role → rule boxes per entry.

use crate::automation::Automation;
use crate::theme::{self, amber, emerald, red, slate, KoreStyled};
use crate::ui::icon::icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::rbac::{PolicyRuleSummary, RuleChainEntry};
use std::time::Duration;

pub const INDIGO200: u32 = 0xc7d2fe;
pub const INDIGO300: u32 = 0xa5b4fc;
pub const INDIGO400: u32 = 0x818cf8;
pub const INDIGO500: u32 = 0x6366f1;
pub const PURPLE200: u32 = 0xe9d5ff;
pub const PURPLE300: u32 = 0xd8b4fe;
pub const PURPLE400: u32 = 0xc084fc;
pub const PURPLE500: u32 = 0xa855f7;

/// One wrapping text made of colored parts (inline `<span>`s), broken after
/// spaces and inner hyphens. `ml-1` margins are approximated
/// by a space.
pub fn spans(parts: Vec<(String, Rgba)>) -> crate::ui::text::ProseText {
    crate::ui::text::prose_text_parts(parts.into_iter().map(|(t, c)| (t, Some(c))).collect())
}

/// `{kind} / {name}` + optional `(ns: x)` box.
#[allow(clippy::too_many_arguments)]
fn ref_box(icon_name: &str, kind: &str, name: &str, ns: Option<&str>, base: u32, c300: u32, c400: u32, c200: u32) -> Div {
    let mut parts = vec![(kind.to_string(), theme::hex(c400)), (" / ".to_string(), slate::S500), (name.to_string(), theme::hex(c200))];
    if let Some(ns) = ns {
        parts.push((format!(" (ns: {ns})"), slate::S600));
    }
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .py(px(6.))
        .rounded(px(4.))
        .bg(theme::hexa(base, 0.1))
        .border_1()
        .border_color(theme::hexa(base, 0.2))
        .child(icon(icon_name, 14.).text_color(theme::hex(c300)))
        .child(div().flex_1().min_w_0().text_xs_tw().font_mono().child(spans(parts)))
}

/// Rule text: resources, ` [verbs]`, api groups, resource names.
pub fn rule_text(rule: &PolicyRuleSummary, res_color: Rgba, show_names: bool) -> Div {
    let mut parts = vec![
        (rule.resources.join(", "), res_color),
        (" [".to_string(), slate::S500),
        (rule.verbs.join(", "), slate::S300),
        ("]".to_string(), slate::S500),
    ];
    if !rule.api_groups.is_empty() && !rule.api_groups[0].is_empty() {
        parts.push((format!(" in \"{}\"", rule.api_groups.join(", ")), slate::S600));
    }
    if show_names && !rule.resource_names.is_empty() {
        parts.push((format!(" (only: {})", rule.resource_names.join(", ")), amber::A400));
    }
    div().flex_1().min_w_0().text_xs_tw().font_mono().child(spans(parts))
}

fn connector() -> Div {
    div().flex().justify_center().child(div().w(px(1.)).h(px(12.)).bg(slate::S700))
}

/// One entry as text, for its automation label:
/// `RoleBinding viewer-bot-pod-reader (ns: kore-demo) -> Role pod-reader (ns: kore-demo) -> pods, pods/log [get, list, watch]`.
pub fn entry_label(e: &RuleChainEntry) -> String {
    let ns = |n: &Option<String>| n.as_ref().map(|n| format!(" (ns: {n})")).unwrap_or_default();
    let r = &e.matching_rule;
    format!(
        "{} {}{} -> {} {}{} -> {} [{}]",
        e.binding_kind,
        e.binding_name,
        ns(&e.binding_namespace),
        e.role_kind,
        e.role_name,
        ns(&e.role_namespace),
        r.resources.join(", "),
        r.verbs.join(", ")
    )
}

/// `auto` names the automation ids: `<auto>.<i>` per entry (label
/// [`entry_label`]), `<auto>.none` for an empty chain.
pub fn rule_chain(id: impl Into<SharedString>, auto: &str, chain: &[RuleChainEntry], allowed: bool) -> AnyElement {
    if chain.is_empty() {
        return div()
            .text_sm_tw()
            .text_color(slate::S500)
            .italic()
            .child("No matching rule found")
            .auto_id(format!("{auto}.none"), "status", "No matching rule found")
            .into_any_element();
    }
    let id: SharedString = id.into();
    let n = chain.len();
    let (rbg, rborder, ric) = if allowed {
        (theme::hexa(0x10b981, 0.1), theme::hexa(0x10b981, 0.2), emerald::E300)
    } else {
        (theme::hexa(0xef4444, 0.1), theme::hexa(0xef4444, 0.2), red::R300)
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .children(chain.iter().enumerate().map(|(i, e)| {
            let block = div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(ref_box(
                    "link-2",
                    &e.binding_kind,
                    &e.binding_name,
                    e.binding_namespace.as_deref(),
                    INDIGO500,
                    INDIGO300,
                    INDIGO400,
                    INDIGO200,
                ))
                .child(connector())
                .child(ref_box(
                    "shield",
                    &e.role_kind,
                    &e.role_name,
                    e.role_namespace.as_deref(),
                    PURPLE500,
                    PURPLE300,
                    PURPLE400,
                    PURPLE200,
                ))
                .child(connector())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .bg(rbg)
                        .border_1()
                        .border_color(rborder)
                        .child(div().mt(px(2.)).flex_none().child(icon("file-text", 14.).text_color(ric)))
                        .child(rule_text(&e.matching_rule, ric, true)),
                )
                .when(i + 1 < n, |d| d.child(div().my(px(8.)).border_b_1().border_color(slate::S800)));
            let delay = i as f32 * 0.05;
            block.with_animation(
                SharedString::from(format!("{id}-{i}")),
                Animation::new(Duration::from_millis(150 + 50 * i as u64)),
                move |el, p| {
                    let total = 0.15 + delay;
                    let q = ((p * total - delay) / 0.15).clamp(0., 1.);
                    el.opacity(q).mt(px(4. * (1. - q)))
                },
            )
            .auto_id(format!("{auto}.{i}"), "group", entry_label(e))
        }))
        .into_any_element()
}
