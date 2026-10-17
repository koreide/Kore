//! PodDetailsView (spec 02 §2): header (back, name, status badge, details
//! toggle, Debug B / Delete D / Ports), collapsible info row, debug banner,
//! tabs Logs/Describe/YAML/Metrics/Events/Shell (keys 1–6), deleted state and
//! the port-forward sidebar host. Single integration point for the Task 5
//! components (logs, metrics, exec, port-forward, debug modal).

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::app_state::{ConfirmRequest, ConfirmVariant};
use crate::state::resource_item::{status_variant, ResourceItem, StatusVariant};
use crate::state::types::ResourceKind;
use crate::state::{AppEvent, AppState};
use crate::theme::{self, emerald, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::status_colors;
use crate::views::debug_container::{DebugContainerModal, DebugEvent};
use crate::views::describe_content::DescribeContent;
use crate::views::detail_ui::{
    btn_icon, button, describe_skeleton, enter_anim, fade_in, kbd_chip, ping_dot, tabs_bar, watch_item, BtnStyle, TabSpec,
};
use crate::views::events_timeline::EventsTimeline;
use crate::views::exec_terminal::ExecTerminal;
use crate::views::pod_logs::{strip_init, LogOptions, PodLogs};
use crate::views::pod_metrics::PodMetrics;
use crate::views::port_forward::PortForwardPanel;
use crate::views::resource_details::pretty;
use crate::views::yaml_editor::YamlEditor;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::Value;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Logs,
    Describe,
    Yaml,
    Metrics,
    Events,
    Shell,
}

const TABS: [(Tab, &str, &str); 6] = [
    (Tab::Logs, "logs", "Logs"),
    (Tab::Describe, "describe", "Describe"),
    (Tab::Yaml, "yaml", "YAML"),
    (Tab::Metrics, "metrics", "Metrics"),
    (Tab::Events, "events", "Events"),
    (Tab::Shell, "shell", "Shell"),
];

/// Container names from the pod JSON: init containers prefixed `init:`, then
/// regular containers.
pub fn pod_containers(pod: &Value) -> Vec<String> {
    let mut names = Vec::new();
    for (key, prefix) in [("initContainers", "init:"), ("containers", "")] {
        if let Some(cs) = pod["spec"][key].as_array() {
            for c in cs {
                if let Some(n) = c["name"].as_str() {
                    names.push(format!("{prefix}{n}"));
                }
            }
        }
    }
    names
}

/// The node a pod is scheduled on (`spec.nodeName`).
pub fn pod_node_name(pod: &Value) -> Option<String> {
    pod["spec"]["nodeName"].as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// Last running debug container from `list_debug_containers`.
pub fn last_running_debug(list: &[Value]) -> Option<(String, String)> {
    list.iter()
        .rfind(|d| d["running"].as_bool().unwrap_or(false))
        .map(|d| (d["name"].as_str().unwrap_or_default().to_string(), d["image"].as_str().unwrap_or_default().to_string()))
}

/// Shell container priority (§2.10): debug container, selected log container,
/// first non-init container.
pub fn shell_container(debug: Option<&str>, log_container: &str, containers: &[String]) -> Option<String> {
    debug
        .map(str::to_string)
        .or_else(|| strip_init(log_container))
        .or_else(|| containers.iter().find(|c| !c.starts_with("init:")).cloned())
}

struct Debug {
    name: String,
    image: String,
}

pub struct PodDetailsView {
    app: Entity<AppState>,
    item: ResourceItem,
    namespace: String,
    tab: Tab,
    prev_tab: usize,
    details_open: bool,
    deleted: bool,
    seen_in_store: bool,
    deleting: bool,
    delete_error: Option<String>,
    static_pod: bool,
    /// `spec.nodeName` (the table's `node` column holds the host IP).
    node_name: Option<String>,
    containers: Vec<String>,
    describe: Option<Entity<DescribeContent>>,
    describe_text: Option<String>,
    debug: Option<Debug>,
    stopping_debug: bool,
    debug_modal: Option<Entity<DebugContainerModal>>,
    log_opts: LogOptions,
    logs: Option<Entity<PodLogs>>,
    yaml: Option<Entity<YamlEditor>>,
    metrics: Option<Entity<PodMetrics>>,
    events: Option<Entity<EventsTimeline>>,
    shell: Option<(Option<String>, Entity<ExecTerminal>)>,
    ports: Option<Entity<PortForwardPanel>>,
    ports_open: bool,
    ports_toggles: u32,
    active_forwards: usize,
    _load: Option<Task<()>>,
    _task: Option<Task<()>>,
    /// Watch keeping `item` (status badge, node, IP) live.
    _item_watch: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _modal_subs: Vec<Subscription>,
    _ports_sub: Option<Subscription>,
}

impl PodDetailsView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let item = app.read(cx).selected.clone().unwrap_or_default();
        let namespace = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let store = app.read(cx).resources.clone();
        let subs = vec![
            cx.subscribe_in(&app, window, |this, app, ev: &AppEvent, _, cx| {
                if matches!(ev, AppEvent::Escape) {
                    // Esc closes the log search first, else goes back.
                    let closed = this.logs.as_ref().map(|l| l.update(cx, |l, cx| l.close_search(cx))).unwrap_or(false);
                    if !closed {
                        app.update(cx, |s, cx| s.back(cx));
                    }
                }
            }),
            cx.observe(&store, |this, store, cx| this.check_deleted(&store, cx)),
            keys::on_window_key(cx, Self::on_key),
        ];
        let mut this = PodDetailsView {
            app,
            item,
            namespace,
            tab: Tab::Logs,
            prev_tab: 0,
            details_open: false,
            deleted: false,
            seen_in_store: false,
            deleting: false,
            delete_error: None,
            static_pod: false,
            node_name: None,
            containers: Vec::new(),
            describe: None,
            describe_text: None,
            debug: None,
            stopping_debug: false,
            debug_modal: None,
            log_opts: LogOptions::default(),
            logs: None,
            yaml: None,
            metrics: None,
            events: None,
            shell: None,
            ports: None,
            ports_open: false,
            ports_toggles: 0,
            active_forwards: 0,
            _load: None,
            _task: None,
            _item_watch: None,
            _subs: subs,
            _modal_subs: Vec::new(),
            _ports_sub: None,
        };
        let store = this.app.read(cx).resources.clone();
        this.check_deleted(&store, cx);
        this._item_watch = Some(watch_item(cx, ResourceKind::Pods, this.namespace.clone(), this.name(), Self::apply_live_item));
        this.mount_ports(window, cx);
        this.load_describe(cx);
        this.enter_tab(Tab::Logs, window, cx);
        this
    }

    fn name(&self) -> String {
        self.item.name.clone()
    }

    /// Deleted detection (§2.12): the pods watch reported the pod gone. We
    /// watch the shared store: once the pod was seen in a settled pods list
    /// for its namespace, its disappearance means a `deleted` event.
    fn check_deleted(&mut self, store: &Entity<crate::state::resources::ResourceStore>, cx: &mut Context<Self>) {
        if self.deleted {
            return;
        }
        let s = store.read(cx);
        let Some(q) = &s.query else { return };
        if q.kind != ResourceKind::Pods || q.multi_cluster || !(q.namespace == "*" || q.namespace == self.namespace) {
            return;
        }
        if s.loading {
            return;
        }
        let present = s.rows.iter().any(|r| r.name == self.item.name && r.namespace == self.namespace);
        if present {
            self.seen_in_store = true;
        } else if self.seen_in_store {
            self.mark_deleted(cx);
        }
    }

    /// Live header data (status badge, node, IP) from the pod watch.
    fn apply_live_item(&mut self, item: ResourceItem, obj: &Value, cx: &mut Context<Self>) {
        if self.deleted {
            return;
        }
        let node = pod_node_name(obj);
        if node.is_some() && node != self.node_name {
            self.node_name = node;
            cx.notify();
        }
        if item == self.item {
            return;
        }
        self.item = ResourceItem { name: self.item.name.clone(), namespace: self.item.namespace.clone(), ..item };
        cx.notify();
    }

    fn mark_deleted(&mut self, cx: &mut Context<Self>) {
        self.deleted = true;
        // Streams of a deleted pod stop with their views.
        self.logs = None;
        self.yaml = None;
        self.metrics = None;
        self.events = None;
        self.shell = None;
        self.ports = None;
        self._ports_sub = None;
        self._item_watch = None;
        self.debug_modal = None;
        self.app.update(cx, |s, cx| s.set_modal("debug-container", false, cx));
        cx.notify();
    }

    fn load_describe(&mut self, cx: &mut Context<Self>) {
        let (ns, name) = (self.namespace.clone(), self.name());
        self._load = Some(cx.spawn(async move |this, cx| {
            let core = cx.update(|cx| backend::core(cx));
            let res = core.describe_pod(ns.clone(), name.clone()).await;
            let ok = res.is_ok();
            this.update(cx, |this, cx| {
                let text = match res {
                    Ok(v) => {
                        this.containers = pod_containers(&v);
                        this.node_name = pod_node_name(&v).or(this.node_name.take());
                        this.static_pod = v["metadata"]["annotations"].get("kubernetes.io/config.mirror").is_some();
                        pretty(&v)
                    }
                    Err(e) => format!("Error fetching pod details: {}", backend::format_error(&e)),
                };
                this.describe_text = Some(text);
                this.sync_describe(cx);
                cx.notify();
            })
            .ok();
            if !ok {
                return;
            }
            if let Ok(list) = core.list_debug_containers(ns, name).await {
                this.update(cx, |this, cx| {
                    if let Some((name, image)) = last_running_debug(&list) {
                        this.debug = Some(Debug { name, image });
                        cx.notify();
                    }
                })
                .ok();
            }
        }));
    }

    /// DescribeContent handles ⌘F at window level, so it only exists while the
    /// Describe tab is shown (inactive panes are dropped).
    fn sync_describe(&mut self, cx: &mut Context<Self>) {
        if self.tab != Tab::Describe || self.deleted {
            self.describe = None;
            return;
        }
        if let (None, Some(text)) = (&self.describe, &self.describe_text) {
            let text = text.clone();
            self.describe = Some(cx.new(|cx| DescribeContent::new(text, cx)));
        }
    }

    fn mount_ports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (app, item) = (self.app.clone(), self.item.clone());
        let panel = cx.new(|cx| PortForwardPanel::new(app, item, window, cx));
        self._ports_sub = Some(cx.observe(&panel, |this, p, cx| {
            let n = p.read(cx).active_count();
            if n != this.active_forwards {
                this.active_forwards = n;
                cx.notify();
            }
        }));
        self.ports = Some(panel);
        self.active_forwards = 0;
    }

    fn tab_index(t: Tab) -> usize {
        TABS.iter().position(|(x, _, _)| *x == t).unwrap_or(0)
    }

    fn enter_tab(&mut self, t: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.deleted {
            return;
        }
        let was = self.tab;
        if t != was {
            self.prev_tab = Self::tab_index(was);
        }
        if let Some(l) = &self.logs {
            self.log_opts = l.read(cx).options();
        }
        self.tab = t;
        // Inactive panes are dropped.
        if t != Tab::Logs {
            self.logs = None;
        }
        if t != Tab::Yaml {
            self.yaml = None;
        }
        if t != Tab::Metrics {
            self.metrics = None;
        }
        if t != Tab::Events {
            self.events = None;
        }
        if t != Tab::Shell {
            self.shell = None;
        }
        self.sync_describe(cx);
        let (app, item, ns, name) = (self.app.clone(), self.item.clone(), self.namespace.clone(), self.name());
        match t {
            Tab::Logs if self.logs.is_none() => {
                let opts = self.log_opts.clone();
                let logs = cx.new(|cx| {
                    let mut l = PodLogs::new(app, item, window, cx);
                    l.set_options(opts, cx);
                    l
                });
                self.logs = Some(logs);
            }
            Tab::Yaml if self.yaml.is_none() => {
                self.yaml = Some(cx.new(|cx| YamlEditor::new(app, ResourceKind::Pods, ns, name, window, cx)));
            }
            Tab::Metrics if self.metrics.is_none() => {
                self.metrics = Some(cx.new(|cx| PodMetrics::new(app, item, window, cx)));
            }
            Tab::Events if self.events.is_none() => {
                self.events = Some(cx.new(|cx| EventsTimeline::new(app, "pods".into(), ns, name, window, cx)));
            }
            _ => {}
        }
        // Port-forward sidebar: unmounted on Shell, remounted (empty) after.
        if t == Tab::Shell {
            self.ports = None;
            self._ports_sub = None;
        } else if self.ports.is_none() {
            self.mount_ports(window, cx);
        }
        self.sync_shell(window, cx);
        cx.notify();
    }

    /// (Re)create the terminal when on the Shell tab and its container changed.
    fn sync_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab != Tab::Shell || self.deleted {
            return;
        }
        let want = shell_container(self.debug.as_ref().map(|d| d.name.as_str()), &self.log_opts.container, &self.containers);
        if self.shell.as_ref().map(|(c, _)| c == &want).unwrap_or(false) {
            return;
        }
        let (app, item, c) = (self.app.clone(), self.item.clone(), want.clone());
        let term = cx.new(|cx| ExecTerminal::new(app, item, c, window, cx));
        self.shell = Some((want, term));
    }

    fn on_key(&mut self, k: &keys::Key, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keys::text_entry_focused(window) {
            return false;
        }
        let (confirm_open, modal) = {
            let a = self.app.read(cx);
            (a.confirm.is_some(), a.modal_open())
        };
        let blocked = self.deleted || confirm_open || modal || self.debug_modal.is_some();
        if k.key == "d" && !blocked {
            self.request_delete(cx);
            return true;
        }
        if k.key == "b" && !blocked {
            self.debug_action(window, cx);
            return true;
        }
        if k.plain() && !k.shift {
            if let Some(i) = ["1", "2", "3", "4", "5", "6"].iter().position(|d| *d == k.key) {
                self.enter_tab(TABS[i].0, window, cx);
                return true;
            }
        }
        false
    }

    fn debug_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.debug.is_some() {
            self.stop_debug(cx);
        } else if !self.static_pod {
            self.open_debug_modal(window, cx);
        }
    }

    fn open_debug_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.debug_modal.is_some() {
            return;
        }
        let (app, item) = (self.app.clone(), self.item.clone());
        let modal = cx.new(|cx| DebugContainerModal::new(app, item, window, cx));
        self._modal_subs = vec![cx.subscribe_in(&modal, window, |this, _, ev: &DebugEvent, window, cx| {
            match ev {
                DebugEvent::Created { container, image } => {
                    this.debug = Some(Debug { name: container.clone(), image: image.clone() });
                    this.debug_modal = None;
                    toast(cx, format!("Debug container \"{container}\" is ready"), ToastKind::Success);
                    this.enter_tab(Tab::Shell, window, cx);
                }
                DebugEvent::Cancelled => {
                    this.debug_modal = None;
                }
            }
            this._modal_subs.clear();
            cx.notify();
        })];
        self.debug_modal = Some(modal);
        cx.notify();
    }

    fn stop_debug(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.debug else { return };
        if self.stopping_debug {
            return;
        }
        self.stopping_debug = true;
        cx.notify();
        let (ns, pod, c) = (self.namespace.clone(), self.name(), d.name.clone());
        self._task = Some(cx.spawn(async move |this, cx| {
            let core = cx.update(|cx| backend::core(cx));
            let res = core.stop_debug_container(ns, pod, c).await;
            cx.update(|cx| {
                this.update(cx, |this, cx| {
                    this.stopping_debug = false;
                    match res {
                        Ok(()) => {
                            this.debug = None;
                            toast(cx, "Debug container stopped", ToastKind::Success);
                        }
                        Err(e) => toast(
                            cx,
                            format!("Failed to stop debug container: {}", backend::format_error(&e)),
                            ToastKind::Error,
                        ),
                    }
                    cx.notify();
                })
                .ok();
            });
        }));
    }

    fn request_delete(&mut self, cx: &mut Context<Self>) {
        if self.deleted {
            return;
        }
        let (ns, name) = (self.namespace.clone(), self.name());
        let app = self.app.clone();
        let this = cx.entity().downgrade();
        let message =
            format!("Are you sure you want to delete pod \"{name}\" in namespace \"{ns}\"? This action cannot be undone.");
        self.app.update(cx, |s, cx| {
            s.confirm(
                ConfirmRequest {
                    title: "Delete Pod".into(),
                    message,
                    confirm_label: "Delete".into(),
                    cancel_label: "Cancel".into(),
                    variant: ConfirmVariant::Danger,
                    on_confirm: Rc::new(move |_, cx| {
                        let core = backend::core(cx);
                        let (app, this, ns, name) = (app.clone(), this.clone(), ns.clone(), name.clone());
                        this.update(cx, |v, cx| {
                            v.deleting = true;
                            v.delete_error = None;
                            cx.notify();
                        })
                        .ok();
                        cx.spawn(async move |cx| {
                            let res = core.delete_resource(ResourceKind::Pods, ns, name).await;
                            cx.update(|cx| {
                                let err = res.err().map(|e| backend::format_error(&e));
                                this.update(cx, |v, cx| {
                                    v.deleting = false;
                                    v.delete_error = err.clone();
                                    cx.notify();
                                })
                                .ok();
                                match err {
                                    None => app.update(cx, |s, cx| s.close_confirm(cx)),
                                    Some(msg) => toast(cx, msg, ToastKind::Error),
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

    // ── Render pieces ───────────────────────────────────────────────────

    fn badge(&self) -> impl IntoElement {
        let label = if self.deleted {
            "Deleted".to_string()
        } else {
            self.item.status.clone().unwrap_or_else(|| "Unknown".into())
        };
        let (dot, text, bg, pulse) = if self.deleted {
            (red::R400, red::R400, theme::hexa(0xef4444, 0.1), false)
        } else {
            status_colors(self.item.status.as_deref())
        };
        let pulse = pulse && status_variant(self.item.status.as_deref()) == StatusVariant::Running;
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .gap(px(6.))
            .px(px(8.))
            .py(px(2.))
            .rounded_full()
            .bg(bg)
            .text_xs_tw()
            .font_weight(FontWeight::MEDIUM)
            .text_color(text)
            .child(if pulse {
                ping_dot("pod-status-ping", dot, 6.).into_any_element()
            } else {
                div().size(px(6.)).rounded_full().bg(dot).flex_none().into_any_element()
            })
            .child(label.clone())
            .auto_id("pod.status", "status", label)
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let small = |b: Stateful<Div>| b.px(px(10.)).py(px(4.)).text_sm_tw();
        let back = small(button("pod-back", BtnStyle::Standard, cx))
            .child(btn_icon("arrow-left", 14., slate::S300))
            .child(kbd_chip("Esc"))
            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |s, cx| s.back(cx))))
            .auto_id("details.back", "button", "Go back to resource list");
        let chevron = icon("chevron-right", 14.)
            .text_color(slate::S500)
            .group_hover("pod-details-toggle", |s| s.text_color(slate::S300))
            .with_transformation(Transformation::rotate(radians(if self.details_open {
                std::f32::consts::FRAC_PI_2
            } else {
                0.
            })));
        let center = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .min_w_0()
            .flex_1()
            .child(div().font_mono().text_sm_tw().text_color(slate::S100).overflow_hidden().min_w_0().child(crate::ui::mono_text::mono_text(self.name())))
            .child(self.badge())
            .child(
                div()
                    .id("pod-details-toggle")
                    .group("pod-details-toggle")
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_none()
                    .gap(px(4.))
                    .text_xs_tw()
                    .text_color(slate::S500)
                    .hover(|s| s.text_color(slate::S300))
                    .cursor_pointer()
                    .child(chevron)
                    .child("details")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.details_open = !this.details_open;
                        cx.notify();
                    }))
                    .auto_id("pod.details-toggle", "button", "details"),
            );

        let actions = (!self.deleted).then(|| {
            let debug_btn = match (&self.debug, self.static_pod) {
                (Some(_), _) => small(button("pod-debug", BtnStyle::Plain, cx))
                    .border_color(theme::hexa(0x10b981, 0.5))
                    .bg(theme::hexa(0x10b981, 0.15))
                    .text_color(emerald::E400)
                    .hover(|s| s.bg(theme::hexa(0x10b981, 0.15)))
                    .when(self.stopping_debug, |b| b.opacity(0.5))
                    .child(btn_icon("bug", 14., emerald::E400))
                    .child(if self.stopping_debug { "Stopping…" } else { "Stop Debug" })
                    .child(ping_dot("pod-debug-ping", emerald::E400, 8.)),
                (None, true) => small(button("pod-debug", BtnStyle::Plain, cx))
                    .text_color(slate::S600)
                    .cursor_not_allowed()
                    .hover(|s| s.bg(transparent_black()))
                    .child(btn_icon("bug", 14., slate::S600))
                    .child("Debug")
                    .child(kbd_chip("B")),
                (None, false) => small(button("pod-debug", BtnStyle::Active, cx))
                    .child(btn_icon("bug", 14., accent))
                    .child("Debug")
                    .child(kbd_chip("B")),
            };
            let debug_label = match (&self.debug, self.static_pod) {
                (Some(d), _) => format!("Stop debug container \"{}\"", d.name),
                (None, true) => "Static pods do not support ephemeral debug containers".to_string(),
                (None, false) => "Debug container".to_string(),
            };
            let debug_disabled = self.debug.is_none() && self.static_pod;
            let delete_btn = small(button("pod-delete", BtnStyle::Danger, cx))
                .when(self.deleting, |b| b.opacity(0.5))
                .child(btn_icon("trash-2", 14., red::R400))
                .child("Delete")
                .child(kbd_chip("D"))
                .on_click(cx.listener(|this, _, _, cx| {
                    if !this.deleting {
                        this.request_delete(cx)
                    }
                }))
                .auto_id("pod.delete", "button", "Delete pod");
            let ports_btn = if self.active_forwards > 0 {
                small(button("pod-ports", BtnStyle::Standard, cx))
                    .border_color(theme::hexa(0x10b981, 0.5))
                    .bg(theme::hexa(0x10b981, 0.15))
                    .text_color(emerald::E400)
                    .child(btn_icon("plug", 14., emerald::E400))
                    .child("Ports")
                    .child(ping_dot("pod-ports-ping", emerald::E400, 8.))
            } else if self.ports_open {
                small(button("pod-ports", BtnStyle::Active, cx)).child(btn_icon("plug", 14., accent)).child("Ports")
            } else {
                small(button("pod-ports", BtnStyle::Standard, cx)).child(btn_icon("plug", 14., slate::S300)).child("Ports")
            };
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .flex_none()
                .child(
                    debug_btn
                        .on_click(cx.listener(|this, _, window, cx| this.debug_action(window, cx)))
                        .auto_id("pod.debug", "button", debug_label)
                        .disabled(debug_disabled),
                )
                .child(delete_btn)
                .child(
                    ports_btn
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ports_open = !this.ports_open;
                            this.ports_toggles += 1;
                            cx.notify();
                        }))
                        .auto_id("pod.ports", "button", "Ports")
                        .selected(self.ports_open),
                )
        });

        let pair = |id: &'static str, label: &'static str, value: String| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(div().text_color(slate::S500).child(label))
                .child(div().text_color(slate::S200).font_mono().child(value.clone()))
                .auto_id(id, "text", value)
        };
        let sep = || div().h(px(12.)).w(px(1.)).bg(slate::S800);
        let unknown = |v: &Option<String>| v.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Unknown".into());

        div()
            .flex_none()
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(10.))
                    .child(back)
                    .child(center)
                    .children(actions),
            )
            .when(self.details_open, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(24.))
                        .px(px(16.))
                        .py(px(8.))
                        .border_t_1()
                        .border_color(theme::hexa(0x1e293b, 0.5))
                        .text_xs_tw()
                        .child(pair("pod.info.namespace", "Namespace:", self.namespace.clone()))
                        .child(sep())
                        .child(pair("pod.info.node", "Node:", unknown(&self.node_name)))
                        .child(sep())
                        .child(pair("pod.info.ip", "IP:", unknown(&self.item.ip)))
                        .with_animation(
                            "pod-info-row",
                            Animation::new(Duration::from_millis(150)),
                            |d, t| d.opacity(t),
                        ),
                )
            })
            .when_some(self.debug.as_ref(), |d, dbg| {
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .px(px(16.))
                        .py(px(8.))
                        .border_t_1()
                        .border_color(theme::hexa(0x1e293b, 0.5))
                        .bg(theme::hexa(0x10b981, 0.05))
                        .child(icon("bug", 14.).text_color(emerald::E400))
                        .child(div().text_xs_tw().font_weight(FontWeight::MEDIUM).text_color(emerald::E400).child("Debug:"))
                        .child(div().text_xs_tw().text_color(slate::S200).child(dbg.name.clone()))
                        .child(
                            div()
                                .text_10()
                                .line_height(px(15.))
                                .font_mono()
                                .text_color(theme::hexa(0x34d399, 0.7))
                                .bg(theme::hexa(0x10b981, 0.1))
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(4.))
                                .child(crate::ui::mono_text::mono_text(dbg.image.clone())),
                        )
                        .auto_id("pod.debug-banner", "status", format!("Debug: {}", dbg.name)),
                )
            })
    }

    fn deleted_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .m(px(16.))
                    .max_w(px(448.))
                    .p(px(32.))
                    .bg(theme::hexa(0x101828, 0.3))
                    .border_1()
                    .border_color(theme::hexa(0xef4444, 0.5))
                    .rounded(px(8.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(icon("alert-circle", 48.).text_color(red::R400).mb(px(16.)))
                    .child(
                        div()
                            .text_xl_tw()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(red::R400)
                            .mb(px(8.))
                            .child("Pod No Longer Exists")
                            .auto_id("pod.deleted", "alert", "Pod No Longer Exists"),
                    )
                    .child(
                        div().text_color(slate::S300).mb(px(16.)).text_center().child(
                            {
                                let pre = "The pod ";
                                let mid = " in namespace ";
                                let post = " has been deleted.";
                                let text = format!("{pre}{}{mid}{}{post}", self.item.name, self.namespace);
                                let sans = font(SharedString::from(theme::SANS));
                                let mono = font(SharedString::from(theme::MONO));
                                let run = |len: usize, f: &Font, c: Rgba| TextRun {
                                    len,
                                    font: f.clone(),
                                    color: c.into(),
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                };
                                StyledText::new(text).with_runs(vec![
                                    run(pre.len(), &sans, slate::S300),
                                    run(self.item.name.len(), &mono, accent),
                                    run(mid.len(), &sans, slate::S300),
                                    run(self.namespace.len(), &mono, accent),
                                    run(post.len(), &sans, slate::S300),
                                ])
                            },
                        ),
                    )
                    .child(
                        button("pod-go-back", BtnStyle::Standard, cx)
                            .text_color(slate::S100)
                            .px(px(16.))
                            .py(px(8.))
                            .text_sm_tw()
                            .child("Go Back")
                            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |s, cx| s.back(cx))))
                            .auto_id("pod.go-back", "button", "Go Back"),
                    ),
            )
            .with_animation("pod-deleted-card", Animation::new(Duration::from_millis(300)), |d, t| {
                d.opacity(t).top(px(20. * (1. - t)))
            })
    }

    fn content(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.deleted {
            return self.deleted_card(cx).into_any_element();
        }
        let el: Option<AnyElement> = match self.tab {
            Tab::Logs => self.logs.clone().map(|v| v.into_any_element()),
            Tab::Describe => {
                let body: AnyElement = match &self.describe {
                    Some(d) => d.clone().into_any_element(),
                    None => describe_skeleton("pod-describe", &[128., 112.], &[&[192., 224., 160.], &[256., 208., 176., 240.]])
                        .into_any_element(),
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
            Tab::Yaml => self.yaml.clone().map(|v| v.into_any_element()),
            Tab::Metrics => self.metrics.clone().map(|v| v.into_any_element()),
            Tab::Events => self.events.clone().map(|v| v.into_any_element()),
            Tab::Shell => self.shell.as_ref().map(|(_, t)| t.clone().into_any_element()),
        };
        fade_in(format!("pod-tab-{:?}", self.tab), el.unwrap_or_else(|| div().into_any_element())).into_any_element()
    }
}

impl Render for PodDetailsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_shell(window, cx);
        let shell_dot = self.debug.is_some();
        let tabs: Vec<TabSpec> =
            TABS.iter().map(|(t, id, label)| TabSpec { id, label, dot: *t == Tab::Shell && shell_dot }).collect();
        let this = cx.entity().downgrade();
        let bar = (!self.deleted).then(|| {
            tabs_bar(&tabs, Self::tab_index(self.tab), self.prev_tab, window, cx, move |i, window, cx| {
                this.update(cx, |v, cx| v.enter_tab(TABS[i].0, window, cx)).ok();
            })
        });
        let header = self.header(cx);
        let content = self.content(cx);
        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(header)
            .children(bar)
            .child(div().flex_1().min_h_0().relative().overflow_hidden().child(content))
            .when_some(self.delete_error.clone(), |d, e| {
                d.child(
                    div()
                        .mx(px(16.))
                        .mb(px(8.))
                        .p(px(8.))
                        .bg(theme::hexa(0xef4444, 0.1))
                        .border_1()
                        .border_color(theme::hexa(0xef4444, 0.5))
                        .rounded(px(4.))
                        .text_xs_tw()
                        .text_color(red::R400)
                        .child(format!("Failed to delete pod: {e}")),
                )
            });
        // Port-forward sidebar: 0 ↔ 320px (0.2s), kept mounted while collapsed.
        let sidebar = (!self.deleted && self.tab != Tab::Shell).then(|| self.ports.clone()).flatten().map(|panel| {
            let open = self.ports_open;
            let target = if open { 320. } else { 0. };
            let wrapper = div()
                .flex_none()
                .h_full()
                .overflow_hidden()
                .w(px(target))
                .opacity(if open { 1. } else { 0. })
                .child(div().w(px(320.)).h_full().child(panel));
            if self.ports_toggles == 0 {
                wrapper.into_any_element()
            } else {
                wrapper
                    .with_animation(
                        ElementId::Name(format!("ports-anim-{}", self.ports_toggles).into()),
                        Animation::new(Duration::from_millis(200)).with_easing(ease_in_out),
                        move |d, t| {
                            let (from, to) = if open { (0., 320.) } else { (320., 0.) };
                            let w = from + (to - from) * t;
                            d.w(px(w)).opacity(w / 320.)
                        },
                    )
                    .into_any_element()
            }
        });
        let modal = self.debug_modal.clone().map(|m| {
            let vp = window.viewport_size();
            deferred(
                anchored()
                    .position(point(px(0.), px(0.)))
                    .child(div().relative().w(vp.width).h(vp.height).child(m)),
            )
            .with_priority(1)
        });
        let body = enter_anim(
            "pod-details-enter",
            div().size_full().flex().flex_row().bg(theme::BACKGROUND).child(left).children(sidebar),
        )
        .auto_id("view.details", "region", self.item.name.clone());
        div().size_full().child(body).children(modal)
    }
}

#[cfg(test)]
mod tests {
    use super::{last_running_debug, pod_containers, pod_node_name, shell_container};

    #[test]
    fn node_name_from_spec() {
        assert_eq!(pod_node_name(&json!({"spec": {"nodeName": "kind-worker"}, "status": {"hostIP": "172.18.0.2"}})).as_deref(), Some("kind-worker"));
        assert_eq!(pod_node_name(&json!({"spec": {}})), None);
    }
    use serde_json::json;

    #[test]
    fn containers_and_shell_priority() {
        let pod = json!({"spec": {"initContainers": [{"name": "i"}], "containers": [{"name": "a"}, {"name": "b"}]}});
        let cs = pod_containers(&pod);
        assert_eq!(cs, vec!["init:i", "a", "b"]);
        assert_eq!(shell_container(Some("dbg"), "b", &cs).as_deref(), Some("dbg"));
        assert_eq!(shell_container(None, "init:i", &cs).as_deref(), Some("i"));
        assert_eq!(shell_container(None, "", &cs).as_deref(), Some("a"));
        assert_eq!(shell_container(None, "", &[]), None);
    }

    #[test]
    fn restores_last_running_debug() {
        let l = vec![
            json!({"name": "d1", "image": "x", "running": true}),
            json!({"name": "d2", "image": "y", "running": false}),
            json!({"name": "d3", "image": "z", "running": true}),
        ];
        assert_eq!(last_running_debug(&l), Some(("d3".into(), "z".into())));
    }
}
