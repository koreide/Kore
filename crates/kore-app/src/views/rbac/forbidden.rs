//! RBAC "Permission Denied" banner (spec 03 §7.8). Views that can hit a
//! Kubernetes 403 hold an `Entity<ForbiddenBanner>` and report their current
//! error with [`ForbiddenBanner::set_error`]; when the message is a 403 the
//! banner asks `Core::rbac_analyze_forbidden` for the identity, the missing
//! permission, the closest matching rules and a suggestion, and renders them.
//! It renders nothing for other errors, before the analysis arrives, after
//! "Dismiss", or if the analysis fails.

use super::picker::identity_label;
use super::rule_chain::rule_chain;
use crate::automation::Automation;
use crate::backend;
use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::mono_text::mono_paragraph;
use crate::ui::text::caps;
use gpui_kit::*;
use kore_core::state::rbac::{is_forbidden_message, ForbiddenAnalysis};
use std::rc::Rc;
use std::time::Duration;

pub struct ForbiddenBanner {
    id: SharedString,
    error: Option<String>,
    analysis: Option<Rc<ForbiddenAnalysis>>,
    dismissed: bool,
    task: Option<Task<()>>,
}

/// `{identity} cannot {verb} {resource}[ in namespace "{ns}"]`.
pub fn denied_line(a: &ForbiddenAnalysis) -> String {
    let mut s = format!("{} cannot {} {}", identity_label(&a.identity), a.verb, a.resource);
    if let Some(ns) = &a.namespace {
        s.push_str(&format!(" in namespace \"{ns}\""));
    }
    s
}

/// Creates a banner owned by view `V`; keep the subscription (it re-renders
/// the owner when the banner's visibility changes).
pub fn forbidden_banner<V: 'static>(id: impl Into<SharedString>, cx: &mut Context<V>) -> (Entity<ForbiddenBanner>, Subscription) {
    let id = id.into();
    let banner = cx.new(|_| ForbiddenBanner::new(id));
    let sub = cx.observe(&banner, |_, _, cx| cx.notify());
    (banner, sub)
}

impl ForbiddenBanner {
    /// `id` names the automation ids (`<id>`, `<id>.dismiss`).
    pub fn new(id: impl Into<SharedString>) -> Self {
        ForbiddenBanner { id: id.into(), error: None, analysis: None, dismissed: false, task: None }
    }

    /// Reports the owner's current error (`None` when it cleared). A new 403
    /// message starts an analysis and un-dismisses the banner; the same
    /// message again is a no-op (so periodic refreshes keep a dismissal).
    pub fn set_error(&mut self, error: Option<&str>, cx: &mut Context<Self>) {
        if self.error.as_deref() == error {
            return;
        }
        self.error = error.map(String::from);
        self.analysis = None;
        self.dismissed = false;
        self.task = None;
        if let Some(msg) = error.filter(|m| is_forbidden_message(m)) {
            let msg = msg.to_string();
            self.task = Some(backend::run(cx, move |c| async move { c.rbac_analyze_forbidden(msg).await }, |this: &mut Self, res, cx| {
                this.analysis = res.ok().map(Rc::new);
                cx.notify();
            }));
        }
        cx.notify();
    }

    /// Whether the banner currently renders anything (owners use it to skip
    /// the slot, so it adds no gap).
    pub fn visible(&self) -> bool {
        self.analysis.is_some() && !self.dismissed
    }
}

impl Render for ForbiddenBanner {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(a) = self.analysis.clone().filter(|_| !self.dismissed) else {
            return div().into_any_element();
        };
        let id = self.id.clone();
        let line = denied_line(&a);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme::hexa(0xef4444, 0.2))
            .bg(theme::hexa(0xef4444, 0.05))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(12.))
                    .child(div().flex_none().mt(px(2.)).child(icon("shield-x", 20.).text_color(red::R400)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).text_color(red::R300).child("Permission Denied"))
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_xs_tw()
                                    .font_mono()
                                    .text_color(slate::S400)
                                    .child(mono_paragraph(line.clone()).line_height(px(16.))),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("{id}-dismiss")))
                            .flex_none()
                            .text_xs_tw()
                            .text_color(slate::S500)
                            .cursor_pointer()
                            .hover(|s| s.text_color(slate::S300))
                            .child("Dismiss")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismissed = true;
                                cx.notify();
                            }))
                            .auto_id(format!("{id}.dismiss"), "button", "Dismiss"),
                    ),
            )
            .when_closest_rules(&a, &id)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(4.))
                    .bg(theme::hexa(0xf59e0b, 0.05))
                    .border_1()
                    .border_color(theme::hexa(0xf59e0b, 0.15))
                    .child(div().flex_none().mt(px(2.)).child(icon("lightbulb", 14.).text_color(theme::amber::A400)))
                    .child(div().flex_1().min_w_0().text_xs_tw().text_color(theme::hexa(0xfde68a, 0.8)).child(a.suggestion.clone())),
            )
            .with_animation(SharedString::from(format!("{id}-in")), Animation::new(Duration::from_millis(300)), |el, t| {
                el.opacity(t).mt(px(12. * (1. - t)))
            })
            .auto_id(id.to_string(), "alert", line)
            .into_any_element()
    }
}

trait ClosestRules {
    fn when_closest_rules(self, a: &ForbiddenAnalysis, id: &SharedString) -> Self;
}

impl ClosestRules for Div {
    /// "Closest Matching Rules" over a denied rule chain, when there are any.
    fn when_closest_rules(self, a: &ForbiddenAnalysis, id: &SharedString) -> Self {
        if a.closest_rules.is_empty() {
            return self;
        }
        self.child(
            div()
                .child(div().mb(px(6.)).text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S500).child(caps("Closest Matching Rules", 10.)))
                .child(rule_chain(format!("{id}-chain"), &format!("{id}.chain"), &a.closest_rules, false)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::denied_line;
    use kore_core::state::rbac::{ForbiddenAnalysis, RbacIdentity};

    #[test]
    fn denied_line_names_identity_and_scope() {
        let mut a = ForbiddenAnalysis {
            verb: "list".into(),
            resource: "pods".into(),
            namespace: Some("kore-demo".into()),
            identity: RbacIdentity::ServiceAccount { name: "bot".into(), namespace: "kore-rbac".into() },
            missing_permission: String::new(),
            closest_rules: vec![],
            suggestion: String::new(),
        };
        assert_eq!(denied_line(&a), "kore-rbac/bot cannot list pods in namespace \"kore-demo\"");
        a.namespace = None;
        a.identity = RbacIdentity::User { name: "alice".into() };
        assert_eq!(denied_line(&a), "alice cannot list pods");
    }
}
