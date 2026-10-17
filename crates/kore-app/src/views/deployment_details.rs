//! DeploymentDetailsView (spec 02 §3): header with Back, replica stepper,
//! Scale, Restart (Fix 16: works), Delete; info grid; tabs Describe / YAML /
//! Rollback / Logs / Events (keys 1–5).

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::app_state::{ConfirmRequest, ConfirmVariant};
use crate::state::resource_item::ResourceItem;
use crate::state::types::ResourceKind;
use crate::state::{AppEvent, AppState};
use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::text::measure;
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::spinning_icon;
use crate::views::describe_content::DescribeContent;
use crate::views::deployment_rollback::DeploymentRollback;
use crate::views::detail_ui::{btn_icon, button, describe_skeleton, enter_anim, fade_in, info_cell, tabs_bar, watch_item, BtnStyle, TabSpec};
use crate::views::events_timeline::EventsTimeline;
use crate::views::multi_pod_logs::MultiPodLogs;
use crate::views::resource_details::{back_button, pretty};
use crate::views::yaml_editor::YamlEditor;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::rc::Rc;

const TABS: [(&str, &str); 5] =
    [("describe", "Describe"), ("yaml", "YAML"), ("rollback", "Rollback"), ("logs", "Logs"), ("events", "Events")];

/// Desired replicas from `ready` (`"x/y"` → y).
pub fn replicas_from_ready(ready: Option<&str>) -> Option<i64> {
    let r = ready?;
    let (_, b) = r.split_once('/')?;
    let digits: String = b.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Number-input parse: `parseInt`, NaN → 0, min 0.
pub fn parse_replicas(s: &str) -> i64 {
    let t = s.trim_start();
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: i64 = digits.parse().unwrap_or(0);
    if neg {
        0
    } else {
        n.max(0)
    }
}

pub struct DeploymentDetailsView {
    app: Entity<AppState>,
    item: ResourceItem,
    namespace: String,
    tab: usize,
    prev_tab: usize,
    replicas: i64,
    replica_input: Entity<TextInput>,
    scaling: bool,
    restarting: bool,
    describe: Option<Entity<DescribeContent>>,
    loading: bool,
    yaml: Option<Entity<YamlEditor>>,
    rollback: Option<Entity<DeploymentRollback>>,
    logs: Option<Entity<MultiPodLogs>>,
    events: Option<Entity<EventsTimeline>>,
    _load: Option<Task<()>>,
    _task: Option<Task<()>>,
    /// Watch keeping `item` (Ready / Up-to-date / Available) live.
    _item_watch: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl DeploymentDetailsView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let item = app.read(cx).selected.clone().unwrap_or_default();
        let namespace = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let replicas = replicas_from_ready(item.ready.as_deref()).unwrap_or(0);
        let replica_input = cx.new(|cx| {
            let mut i = TextInput::new(
                "",
                InputStyle {
                    text_color: slate::S100,
                    font_family: theme::MONO.into(),
                    font_size: 14.,
                    line_height: 20.,
                    padding: (0., 0., 6., 6.),
                    ..Default::default()
                },
                window,
                cx,
            );
            i.set_text(replicas.to_string(), cx);
            i
        });
        let subs = vec![
            cx.subscribe(&app, |_, app, ev: &AppEvent, cx| {
                if matches!(ev, AppEvent::Escape) {
                    app.update(cx, |s, cx| s.back(cx));
                }
            }),
            cx.subscribe(&replica_input, |this, input, ev: &InputEvent, cx| match ev {
                InputEvent::Change(s) => {
                    this.replicas = parse_replicas(s);
                    cx.notify();
                }
                InputEvent::Up => this.set_replicas(this.replicas + 1, cx),
                InputEvent::Down => this.set_replicas((this.replicas - 1).max(0), cx),
                InputEvent::Blur => {
                    // A controlled number input re-renders the parsed value.
                    let v = this.replicas.to_string();
                    if input.read(cx).text() != v {
                        input.update(cx, |i, cx| i.set_text(v, cx));
                    }
                }
                _ => {}
            }),
            keys::on_window_key(cx, Self::on_key),
        ];
        let mut this = DeploymentDetailsView {
            app,
            item,
            namespace,
            tab: 0,
            prev_tab: 0,
            replicas,
            replica_input,
            scaling: false,
            restarting: false,
            describe: None,
            loading: true,
            yaml: None,
            rollback: None,
            logs: None,
            events: None,
            _load: None,
            _task: None,
            _item_watch: None,
            _subs: subs,
        };
        this._item_watch =
            Some(watch_item(cx, ResourceKind::Deployments, this.namespace.clone(), this.item.name.clone(), |this, item, _, cx| {
                if item != this.item {
                    this.item = ResourceItem { name: this.item.name.clone(), namespace: this.item.namespace.clone(), ..item };
                    cx.notify();
                }
            }));
        this.enter_tab(0, window, cx);
        this
    }

    fn on_key(&mut self, k: &keys::Key, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keys::text_entry_focused(window) || !k.plain() || k.shift {
            return false;
        }
        if let Some(i) = ["1", "2", "3", "4", "5"].iter().position(|d| *d == k.key) {
            self.enter_tab(i, window, cx);
            return true;
        }
        false
    }

    fn set_replicas(&mut self, n: i64, cx: &mut Context<Self>) {
        self.replicas = n.max(0);
        let v = self.replicas.to_string();
        self.replica_input.update(cx, |i, cx| i.set_text(v, cx));
        cx.notify();
    }

    fn enter_tab(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if i != self.tab {
            self.prev_tab = self.tab;
        }
        self.tab = i;
        self.yaml = None;
        self.rollback = None;
        self.logs = None;
        self.events = None;
        // DescribeContent listens for ⌘F at window level: only keep it while shown.
        self.describe = None;
        self._load = None;
        let (ns, name) = (self.namespace.clone(), self.item.name.clone());
        let app = self.app.clone();
        match TABS[i].0 {
            "describe" => {
                self.loading = true;
                self.describe = None;
                self._load = Some(backend::run(
                    cx,
                    move |core| async move { core.describe_resource(ResourceKind::Deployments, ns, name).await },
                    |this: &mut Self, res, cx| {
                        let text = match res {
                            Ok(v) => {
                                if let Some(n) = v["spec"]["replicas"].as_i64() {
                                    this.set_replicas(n, cx);
                                }
                                pretty(&v)
                            }
                            Err(e) => format!("Error: {}", backend::format_error(&e)),
                        };
                        this.loading = false;
                        this.describe = Some(cx.new(|cx| DescribeContent::new(text, cx)));
                        cx.notify();
                    },
                ));
            }
            "yaml" => self.yaml = Some(cx.new(|cx| YamlEditor::new(app, ResourceKind::Deployments, ns, name, window, cx))),
            "rollback" => self.rollback = Some(cx.new(|cx| DeploymentRollback::new(app, ns, name, window, cx))),
            "logs" => {
                // The Deployment's real `spec.selector` (resolved by MultiPodLogs).
                self.logs = Some(cx.new(|cx| MultiPodLogs::for_deployment(app, ns, name, window, cx)));
            }
            _ => {
                self.events = Some(cx.new(|cx| EventsTimeline::new(app, "deployments".into(), ns, name, window, cx)));
            }
        }
        cx.notify();
    }

    fn scale(&mut self, cx: &mut Context<Self>) {
        if self.scaling {
            return;
        }
        self.scaling = true;
        cx.notify();
        let (ns, name, n) = (self.namespace.clone(), self.item.name.clone(), self.replicas);
        self._task = Some(backend::run(
            cx,
            move |core| async move { core.scale_deployment(ns, name, n as i32).await },
            move |this: &mut Self, res, cx| {
                this.scaling = false;
                match res {
                    Ok(()) => toast(cx, format!("Scaled to {n} replicas"), ToastKind::Success),
                    Err(e) => toast(cx, backend::format_error(&e), ToastKind::Error),
                }
                cx.notify();
            },
        ));
    }

    fn request_restart(&mut self, cx: &mut Context<Self>) {
        let (ns, name) = (self.namespace.clone(), self.item.name.clone());
        let app = self.app.clone();
        let this = cx.entity().downgrade();
        let message = format!("This will initiate a rolling restart of deployment \"{name}\". Pods will be recreated.");
        self.app.update(cx, |s, cx| {
            s.confirm(
                ConfirmRequest {
                    title: "Restart Deployment".into(),
                    message,
                    confirm_label: "Restart".into(),
                    cancel_label: "Cancel".into(),
                    variant: ConfirmVariant::Warning,
                    on_confirm: Rc::new(move |_, cx| {
                        let core = backend::core(cx);
                        let (app, this, ns, name) = (app.clone(), this.clone(), ns.clone(), name.clone());
                        this.update(cx, |v, cx| {
                            v.restarting = true;
                            cx.notify();
                        })
                        .ok();
                        cx.spawn(async move |cx| {
                            let res = core.restart_deployment(ns, name).await;
                            cx.update(|cx| {
                                this.update(cx, |v, cx| {
                                    v.restarting = false;
                                    cx.notify();
                                })
                                .ok();
                                match res {
                                    Ok(()) => {
                                        app.update(cx, |s, cx| s.close_confirm(cx));
                                        toast(cx, "Rolling restart initiated", ToastKind::Success);
                                    }
                                    Err(e) => toast(cx, backend::format_error(&e), ToastKind::Error),
                                }
                            });
                        })
                        .detach();
                    }),
                },
                cx,
            )
        });
    }

    fn request_delete(&mut self, cx: &mut Context<Self>) {
        let (ns, name) = (self.namespace.clone(), self.item.name.clone());
        let app = self.app.clone();
        let message =
            format!("Are you sure you want to delete deployment \"{name}\" in namespace \"{ns}\"? This action cannot be undone.");
        self.app.update(cx, |s, cx| {
            s.confirm(
                ConfirmRequest {
                    title: "Delete Deployment".into(),
                    message,
                    confirm_label: "Delete".into(),
                    cancel_label: "Cancel".into(),
                    variant: ConfirmVariant::Danger,
                    on_confirm: Rc::new(move |_, cx| {
                        let core = backend::core(cx);
                        let (app, ns, name) = (app.clone(), ns.clone(), name.clone());
                        cx.spawn(async move |cx| {
                            let res = core.delete_resource(ResourceKind::Deployments, ns, name).await;
                            cx.update(|cx| match res {
                                Ok(()) => {
                                    app.update(cx, |s, cx| s.close_confirm(cx));
                                    toast(cx, "Deployment deleted", ToastKind::Success);
                                    app.update(cx, |s, cx| s.back(cx));
                                }
                                Err(e) => toast(cx, backend::format_error(&e), ToastKind::Error),
                            });
                        })
                        .detach();
                    }),
                },
                cx,
            )
        });
    }

    fn header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        // Center the number in the input's text box (the spin button takes the right 15px).
        let text_w = measure(window, self.replica_input.read(cx).text(), theme::MONO, 14., FontWeight::NORMAL);
        let pad_l = ((31. - text_w) / 2.).max(0.);
        self.replica_input.update(cx, |i, _| i.style.padding = (pad_l, 15., 6., 6.));
        let step_btn = |id: &'static str, ic: &'static str, disabled: bool| {
            div()
                .id(id)
                .px(px(8.))
                .py(px(6.))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3)))
                .when(disabled, |d| d.opacity(0.5))
                .child(btn_icon(ic, 14., slate::S300))
        };
        let stepper = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .border_1()
            .border_color(slate::S800)
            .rounded(px(6.))
            .overflow_hidden()
            .child(
                step_btn("replicas-minus", "minus", self.scaling)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.scaling {
                            this.set_replicas(this.replicas - 1, cx)
                        }
                    }))
                    .auto_id("deployment.replicas.minus", "button", "Decrease replicas"),
            )
            .child(
                div()
                    .relative()
                    .w(px(48.))
                    .h(px(32.))
                    .border_l_1()
                    .border_r_1()
                    .border_color(slate::S800)
                    .child(self.replica_input.clone())
                    .child(
                        div()
                            .absolute()
                            .right(px(3.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(btn_icon("chevrons-up-down", 12., slate::S400)),
                    )
                    .auto_id("deployment.replicas", "spinbutton", self.replicas.to_string()),
            )
            .child(
                step_btn("replicas-plus", "plus", self.scaling)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.scaling {
                            this.set_replicas(this.replicas + 1, cx)
                        }
                    }))
                    .auto_id("deployment.replicas.plus", "button", "Increase replicas"),
            );
        let scale = button("deployment-scale", BtnStyle::Active, cx)
            .px(px(12.))
            .py(px(6.))
            .text_sm_tw()
            .when(self.scaling, |d| d.opacity(0.5))
            .child(if self.scaling { "Scaling..." } else { "Scale" })
            .on_click(cx.listener(|this, _, _, cx| this.scale(cx)))
            .auto_id("deployment.scale", "button", "Scale");
        let restart = button("deployment-restart", BtnStyle::Plain, cx)
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .text_sm_tw()
            .hover(|s| s.border_color(theme::hexa(0xf59e0b, 0.5)).bg(theme::hexa(0xf59e0b, 0.1)))
            .when(self.restarting, |d| d.opacity(0.5))
            .map(|d| {
                if self.restarting {
                    d.child(spinning_icon("restart-spin", "refresh-cw", 16., slate::S300))
                } else {
                    d.child(btn_icon("refresh-cw", 16., slate::S300))
                }
            })
            .child("Restart")
            .on_click(cx.listener(|this, _, _, cx| this.request_restart(cx)))
            .auto_id("deployment.restart", "button", "Restart");
        let delete = button("deployment-delete", BtnStyle::Danger, cx)
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .text_sm_tw()
            .child(btn_icon("trash-2", 16., red::R400))
            .child("Delete")
            .on_click(cx.listener(|this, _, _, cx| this.request_delete(cx)))
            .auto_id("deployment.delete", "button", "Delete");
        let _ = accent;
        let wide = window.viewport_size().width >= px(768.);
        let it = &self.item;
        div()
            .flex_none()
            .p(px(16.))
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .mb(px(16.))
                    .child(
                        back_button("deployment-back", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |s, cx| s.back(cx))))
                            .auto_id("details.back", "button", "Back"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(stepper)
                            .child(scale)
                            .child(restart)
                            .child(delete),
                    ),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(if wide { 5 } else { 2 })
                    .gap(px(16.))
                    .text_sm_tw()
                    .child(info_cell("Name", it.name.clone()))
                    .child(info_cell("Namespace", self.namespace.clone()))
                    .child(info_cell("Ready", it.ready.clone().unwrap_or_else(|| "-".into())))
                    .child(info_cell("Up-to-date", it.up_to_date.map(|n| n.to_string()).unwrap_or_else(|| "-".into())))
                    .child(info_cell("Available", it.available.map(|n| n.to_string()).unwrap_or_else(|| "-".into()))),
            )
    }
}

impl Render for DeploymentDetailsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs: Vec<TabSpec> = TABS.iter().map(|(id, label)| TabSpec { id, label, dot: false }).collect();
        let this = cx.entity().downgrade();
        let bar = tabs_bar(&tabs, self.tab, self.prev_tab, window, cx, move |i, window, cx| {
            this.update(cx, |v, cx| v.enter_tab(i, window, cx)).ok();
        });
        let content: AnyElement = match TABS[self.tab].0 {
            "describe" => {
                let body: AnyElement = match (&self.describe, self.loading) {
                    (Some(d), false) => d.clone().into_any_element(),
                    _ => describe_skeleton("dep-describe", &[128.], &[&[192., 224.]]).into_any_element(),
                };
                Some(
                    div()
                        .absolute()
                        .top(px(16.))
                        .left(px(16.))
                        .right(px(16.))
                        .bottom(px(16.))
                        .bg(theme::hexa(0x101828, 0.3))
                        .border_1()
                        .border_color(slate::S800)
                        .rounded(px(8.))
                        .overflow_hidden()
                        .child(body)
                        .into_any_element(),
                )
            }
            "yaml" => self.yaml.clone().map(|v| v.into_any_element()),
            "rollback" => self.rollback.clone().map(|v| v.into_any_element()),
            "logs" => self.logs.clone().map(|v| v.into_any_element()),
            _ => self.events.clone().map(|v| v.into_any_element()),
        }
        .unwrap_or_else(|| div().into_any_element());
        let header = self.header(window, cx);
        enter_anim(
            "deployment-details-enter",
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(theme::BACKGROUND)
                .child(header)
                .child(bar)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .overflow_hidden()
                        .child(fade_in(format!("dep-tab-{}", self.tab), content)),
                ),
        )
        .auto_id("view.details", "region", self.item.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_replicas, replicas_from_ready};

    #[test]
    fn replica_parsing() {
        assert_eq!(replicas_from_ready(Some("2/3")), Some(3));
        assert_eq!(replicas_from_ready(Some("3")), None);
        assert_eq!(parse_replicas("12abc"), 12);
        assert_eq!(parse_replicas("abc"), 0);
        assert_eq!(parse_replicas("-4"), 0);
    }
}
