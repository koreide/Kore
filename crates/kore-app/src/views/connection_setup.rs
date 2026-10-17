//! ConnectionSetup (spec 01 §12): shown instead of the app when not connected.

use crate::automation::Automation;
use crate::state::AppState;
use crate::theme::{self, amber, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::scrollbar;
use crate::ui::text::caps;
use crate::ui::widgets::spinning_icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;

pub struct ConnectionSetup {
    app: Entity<AppState>,
    selected: Option<String>,
    details_open: bool,
    list_scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    _sub: Subscription,
}

impl ConnectionSetup {
    pub fn new(app: Entity<AppState>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&app, |_, _, cx| cx.notify());
        ConnectionSetup { app, selected: None, details_open: false, list_scroll: ScrollHandle::new(), gutter: Default::default(), _sub: sub }
    }
}

struct ErrorInfo {
    icon: &'static str,
    title: &'static str,
    description: String,
    steps: &'static [&'static str],
}

fn error_info(kind: Option<&str>, path: Option<&str>, error: Option<&str>) -> ErrorInfo {
    match kind.unwrap_or("unknown") {
        "no_kubeconfig" => ErrorInfo {
            icon: "file-question",
            title: "No Kubeconfig Found",
            description: format!("Kore couldn't find a kubeconfig file{}.", path.map(|p| format!(" at {p}")).unwrap_or_default()),
            steps: &[
                "Ensure you have a Kubernetes cluster configured",
                "Check that ~/.kube/config exists and is readable",
                "Or set the KUBECONFIG environment variable to your config path",
                "If using a cloud provider, run the appropriate auth command (e.g. aws eks update-kubeconfig, gcloud container clusters get-credentials, az aks get-credentials)",
            ],
        },
        "auth_failed" => ErrorInfo {
            icon: "shield-x",
            title: "Authentication Failed",
            description: "Kore connected to the cluster but your credentials were rejected.".into(),
            steps: &[
                "Your token or certificate may have expired",
                "Run kubectl get pods to verify your credentials work",
                "If using a cloud provider, re-authenticate (e.g. aws sso login, gcloud auth login)",
                "Check that your kubeconfig user entry has valid credentials",
            ],
        },
        "cluster_unreachable" => ErrorInfo {
            icon: "wifi-off",
            title: "Cluster Unreachable",
            description: "Kore found your kubeconfig but couldn't connect to the cluster.".into(),
            steps: &[
                "Check that your cluster is running and accessible",
                "If using a VPN, ensure it's connected",
                "Verify the cluster endpoint in your kubeconfig is correct",
                "Try running kubectl cluster-info to test connectivity",
            ],
        },
        _ => ErrorInfo {
            icon: "alert-triangle",
            title: "Connection Error",
            description: error.unwrap_or("An unexpected error occurred while connecting.").to_string(),
            steps: &[
                "Check your kubeconfig file for syntax errors",
                "Try running kubectl get pods to test your configuration",
                "Restart Kore after fixing the issue",
            ],
        },
    }
}

impl Render for ConnectionSetup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let app = self.app.read(cx);
        let retrying = app.retrying;
        let Some(status) = app.connection.clone() else { return div().into_any_element() };
        let info = error_info(status.error_kind.as_deref(), status.kubeconfig_path.as_deref(), status.error.as_deref());
        let selected = self.selected.clone();

        let logo = div()
            .flex()
            .flex_col()
            .items_center()
            .mb(px(32.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .mb(px(8.))
                    .child(
                        div()
                            .size(px(40.))
                            .rounded(px(12.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("wifi", 20.).text_color(accent)),
                    )
                    .child(
                        div()
                            .text_2xl_tw()
                            .font_weight(FontWeight::BOLD)
                            .text_color(slate::S100)
                            .child(crate::ui::text::tracked("Kore", 24., -0.025)),
                    ),
            )
            .child(div().text_sm_tw().text_color(slate::S500).child("Kubernetes IDE"));

        let details = status.error.clone().map(|err| {
            div()
                .mb(px(20.))
                .child(
                    div()
                        .id("tech-details")
                        .group("tech-details")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .text_xs_tw()
                        .text_color(slate::S600)
                        .hover(|s| s.text_color(slate::S400))
                        .cursor_pointer()
                        .child(
                            div()
                                .text_size(px(9.))
                                .child(if self.details_open { "▼" } else { "▶" }),
                        )
                        .child("Show technical details")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.details_open = !this.details_open;
                            cx.notify();
                        }))
                        .auto_id("connection.details", "button", "Show technical details"),
                )
                .when(self.details_open, |d| {
                    d.child(
                        div()
                            .mt(px(8.))
                            .text_xs_tw()
                            .text_color(slate::S500)
                            .bg(theme::hexa(0x0b1221, 0.6))
                            .rounded(px(8.))
                            .p(px(12.))
                            .font_mono()
                            .child(crate::ui::mono_text::mono_paragraph(err.clone()))
                            .auto_id("connection.error", "text", err),
                    )
                })
        });

        let card = div()
            .glass()
            .rounded(px(12.))
            .p(px(24.))
            .mb(px(24.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(16.))
                    .mb(px(20.))
                    .child(div().mt(px(2.)).flex_none().child(icon(info.icon, 32.).text_color(amber::A400)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_lg_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).mb(px(4.)).child(info.title))
                            .child(
                                div()
                                    .text_sm_tw()
                                    .line_height(px(22.))
                                    .text_color(slate::S400)
                                    .child(crate::ui::text::hyphen_wrap(&info.description))
                                    .auto_id("connection.description", "text", info.description.clone()),
                            ),
                    ),
            )
            .children(details)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_xs_tw().font_weight(FontWeight::MEDIUM).text_color(slate::S500).child(caps("How to fix", 12.)))
                    .children(info.steps.iter().enumerate().map(|(i, step)| {
                        div()
                            .mt(px(10.))
                            .flex()
                            .flex_row()
                            .items_start()
                            .gap(px(10.))
                            .child(
                                div()
                                    .size(px(20.))
                                    .mt(px(2.))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(slate::S800)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_10()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(slate::S400)
                                    .child((i + 1).to_string()),
                            )
                            .child(div().flex_1().min_w_0().text_sm_tw().line_height(px(22.)).text_color(slate::S300).child(crate::ui::text::hyphen_wrap(step)))
                            .auto_id(format!("connection.step.{}", i + 1), "listitem", step.to_string())
                    })),
            );

        let picker = (!status.contexts_available.is_empty()).then(|| {
            let gutter = self.gutter.overflows();
            div()
                .glass()
                .rounded(px(12.))
                .p(px(20.))
                .mb(px(24.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .mb(px(12.))
                        .text_sm_tw()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(slate::S300)
                        .child(icon("terminal", 16.).text_color(slate::S500))
                        .child("Try a different context")
                        .auto_id("connection.picker", "heading", "Try a different context"),
                )
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_row()
                        .max_h(px(192.))
                        .child(
                            div()
                                .id("ctx-list")
                                .flex_1()
                                .overflow_y_scroll()
                                .track_scroll(&self.list_scroll)
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .children(status.contexts_available.iter().enumerate().map(|(i, ctx)| {
                                    let is_current = status.current_context.as_deref() == Some(ctx.as_str());
                                    let is_sel = selected.as_deref() == Some(ctx.as_str());
                                    let c = ctx.clone();
                                    let group: SharedString = format!("ctx-row-{i}").into();
                                    div()
                                        .id(("ctx-row", i))
                                        .group(group.clone())
                                        .w_full()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(10.))
                                        .px(px(12.))
                                        .py(px(8.))
                                        .rounded(px(8.))
                                        .text_sm_tw()
                                        .border_1()
                                        .map(|d| {
                                            if is_sel {
                                                d.border_color(theme::PREFLIGHT_BORDER).text_color(accent)
                                            } else if is_current {
                                                d.bg(theme::hexa(0x1e293b, 0.6)).border_color(slate::S700).text_color(slate::S400)
                                            } else {
                                                d.bg(theme::hexa(0x101828, 0.4))
                                                    .border_color(theme::hexa(0, 0.))
                                                    .text_color(slate::S300)
                                                    .hover(|s| s.border_color(slate::S700).text_color(slate::S100))
                                            }
                                        })
                                        .when(!retrying, |d| d.cursor_pointer())
                                        .child(div().size(px(6.)).flex_none().rounded_full().bg(if is_sel {
                                            accent
                                        } else if is_current {
                                            amber::A400
                                        } else {
                                            slate::S600
                                        }))
                                        .child(div().font_mono().truncate().child(ctx.clone()))
                                        .when(is_current, |d| {
                                            d.child(
                                                div()
                                                    .ml_auto()
                                                    .flex_none()
                                                    .text_10()
                                                    .text_color(slate::S600)
                                                    .child("current")
                                                    .auto_id(format!("connection.context.{ctx}.current"), "text", "current"),
                                            )
                                        })
                                        .when(is_sel, |d| d.child(div().ml_auto().flex_none().child(icon("chevron-right", 14.).text_color(accent))))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if this.app.read(cx).retrying {
                                                return;
                                            }
                                            this.selected = if this.selected.as_deref() == Some(c.as_str()) { None } else { Some(c.clone()) };
                                            cx.notify();
                                        }))
                                        .auto_id(format!("connection.context.{ctx}"), "option", ctx.clone())
                                        .selected(is_sel)
                                })),
                        )
                        .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
                        .child(scrollbar::vertical(self.list_scroll.clone(), self.gutter.clone())),
                )
        });

        let label: SharedString = if retrying {
            "Connecting...".into()
        } else if let Some(s) = &selected {
            format!("Connect to {s}").into()
        } else {
            "Retry Connection".into()
        };
        let retry = div()
            .id("retry")
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(10.))
            .rounded(px(8.))
            .text_sm_tw()
            .font_weight(FontWeight::MEDIUM)
            .text_color(accent)
            .border_1()
            .border_color(theme::PREFLIGHT_BORDER)
            .when(retrying, |d| d.opacity(0.5))
            .when(!retrying, |d| d.cursor_pointer())
            .child(if retrying {
                spinning_icon("retry-spin", "refresh-cw", 16., accent).into_any_element()
            } else {
                icon("refresh-cw", 16.).text_color(accent).into_any_element()
            })
            .child(label.clone())
            .on_click(cx.listener(|this, _, _, cx| {
                if this.app.read(cx).retrying {
                    return;
                }
                let ctx = this.selected.clone();
                this.app.update(cx, |a, cx| a.retry_connection(ctx, cx));
            }))
            .auto_id("connection.retry", "button", label)
            .disabled(retrying);

        let footer = status.kubeconfig_path.clone().map(|p| {
            div()
                .mt(px(16.))
                .w_full()
                .text_center()
                .text_11()
                .font_mono()
                .text_color(slate::S600)
                .child(crate::ui::text::hyphen_wrap(&format!("Checking: {p}")))
                .auto_id("connection.kubeconfig", "text", format!("Checking: {p}"))
        });

        div()
            .id("connection-setup")
            .size_full()
            .bg(theme::BACKGROUND)
            .flex()
            .items_center()
            .justify_center()
            .p(px(32.))
            .overflow_y_scroll()
            .child(
                div()
                    .max_w(px(512.))
                    .w_full()
                    .child(logo)
                    .child(card)
                    .children(picker)
                    .child(div().flex().flex_row().gap(px(12.)).child(retry))
                    .children(footer)
                    .with_animation(
                        "setup-in",
                        Animation::new(Duration::from_millis(400)).with_easing(ease_out_quint()),
                        |el, t| el.opacity(t).mt(px(20. * (1. - t))),
                    ),
            )
            .auto_id("connection", "region", info.title)
            .into_any_element()
    }
}
