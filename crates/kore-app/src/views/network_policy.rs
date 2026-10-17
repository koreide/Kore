//! Network Policies view (spec 03 §8): stats bar, AI chat with locally
//! computed traffic simulations, and the collapsible policy panel.

use crate::automation::Automation;
use crate::backend;
use crate::state::ai_config::{self, AiConfigModel};
use crate::state::ai_stream::{self, AiUpdate};
use crate::state::types::ResourceKind;
use crate::state::AppState;
use crate::theme::{self, amber, emerald, misc, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::scrollbar;
use crate::ui::widgets::spinning_icon;
use crate::views::ai_chat_bubble::{chat_bubble_sized, BubbleVariant, DisplayMessage};
use crate::views::rbac::{forbidden_banner, ForbiddenBanner};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::state::network_policy::{NetworkPolicyGraph, PodGroup, PolicySummary, TrafficSimulationResult};
use kore_core::{AIChatRequest, AIStreamEvent, ChatMessage};
use std::rc::Rc;
use std::time::Duration;

// ── Pure helpers ────────────────────────────────────────────────────────

fn selector_pairs(sel: &std::collections::HashMap<String, String>) -> Vec<(String, String)> {
    let mut v: Vec<_> = sel.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    v.sort();
    v
}

/// `buildGraphContext(graph)`: markdown summary for the system prompt.
pub fn build_graph_context(g: &NetworkPolicyGraph) -> String {
    let mut lines: Vec<String> = vec!["# Network Policy Graph Context\n".into()];
    if !g.policies.is_empty() {
        lines.push(format!("## Policies ({})", g.policies.len()));
        for p in &g.policies {
            let sel = if p.pod_selector.is_empty() {
                "all pods".to_string()
            } else {
                selector_pairs(&p.pod_selector).iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ")
            };
            lines.push(format!(
                "- **{}** (ns: {}) — types: [{}], selector: {{{}}}, affects {} pods, {} ingress rules, {} egress rules",
                p.name,
                p.namespace,
                p.policy_types.join(", "),
                sel,
                p.affected_pod_count,
                p.ingress_rule_count,
                p.egress_rule_count
            ));
        }
        lines.push(String::new());
    }
    if !g.groups.is_empty() {
        lines.push(format!("## Workload Groups ({})", g.groups.len()));
        for gr in &g.groups {
            let mut iso = Vec::new();
            if gr.is_isolated_ingress {
                iso.push("ingress-isolated");
            }
            if gr.is_isolated_egress {
                iso.push("egress-isolated");
            }
            let iso = if iso.is_empty() { String::new() } else { format!(" [{}]", iso.join(", ")) };
            let pol = if gr.matching_policies.is_empty() { String::new() } else { format!(" policies: [{}]", gr.matching_policies.join(", ")) };
            lines.push(format!("- **{}** (ns: {}, kind: {}, {} pods){}{}", gr.name, gr.namespace, gr.kind, gr.pod_count, iso, pol));
        }
        lines.push(String::new());
    }
    if !g.external_cidrs.is_empty() {
        lines.push(format!("## External CIDRs ({})", g.external_cidrs.len()));
        for c in &g.external_cidrs {
            let ex = if c.except.is_empty() { String::new() } else { format!(" except [{}]", c.except.join(", ")) };
            lines.push(format!("- {}{} (from policy: {})", c.cidr, ex, c.from_policy));
        }
        lines.push(String::new());
    }
    if !g.edges.is_empty() {
        lines.push(format!("## Traffic Edges ({})", g.edges.len()));
        for e in &g.edges {
            let ports = if e.ports.is_empty() {
                "all ports".to_string()
            } else {
                e.ports
                    .iter()
                    .map(|p| match p.port {
                        Some(n) if n != 0 => format!("{n}/{}", p.protocol),
                        _ => p.protocol.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            lines.push(format!("- {} → {} [{}] via {} ({})", e.source, e.target, e.direction, e.policy_name, ports));
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

/// `detectSimulation`: "can X talk|reach|… to|with Y" → (source, dest) group indices.
pub fn detect_simulation(text: &str, groups: &[PodGroup]) -> Option<(usize, usize)> {
    let re = regex::Regex::new(r"(?i)can\s+(.+?)\s+(?:talk|reach|connect|access|communicate|send)\s+(?:to|with)\s+(.+?)[\s?]*$").ok()?;
    let caps = re.captures(text)?;
    let src = caps.get(1)?.as_str().trim().to_lowercase();
    let dst = caps.get(2)?.as_str().trim().to_lowercase();
    let find = |q: &str| -> Option<usize> {
        groups
            .iter()
            .position(|g| g.name.to_lowercase() == q)
            .or_else(|| groups.iter().position(|g| g.name.to_lowercase().contains(q)))
            .or_else(|| groups.iter().position(|g| g.pods.iter().any(|p| p.name.to_lowercase().contains(q))))
    };
    let (s, d) = (find(&src)?, find(&dst)?);
    (groups[s].id != groups[d].id).then_some((s, d))
}

/// Suggested questions (spec 03 §8.4), at most 4.
pub fn suggested_questions(g: &NetworkPolicyGraph) -> Vec<String> {
    let mut q = Vec::new();
    if g.groups.len() >= 2 {
        q.push(format!("Can {} talk to {}?", g.groups[0].name, g.groups[1].name));
    }
    if g.groups.iter().any(|g| g.is_isolated_ingress || g.is_isolated_egress) {
        q.push("Which pods are isolated and why?".into());
    }
    if !g.external_cidrs.is_empty() {
        q.push("What external traffic is allowed?".into());
    }
    if let Some(p) = g.policies.first() {
        q.push(format!("What does {} do?", p.name));
    }
    q.push("Summarize the network security posture".into());
    q.truncate(4);
    q
}

/// Custom 6px horizontal scrollbar (in flow, below the strip) shown
/// when `handle` overflows horizontally.
fn hscrollbar(handle: ScrollHandle) -> impl IntoElement {
    let h = handle.clone();
    let overflow = f32::from(handle.max_offset().x) > 0.5;
    div().w_full().when(overflow, |d| d.h(px(scrollbar::WIDTH))).child(
        canvas(|_, _, _| {}, move |bounds, _, window, _| {
            let max = h.max_offset().x;
            let vw = h.bounds().size.width;
            if max <= px(0.5) || vw <= px(0.) {
                return;
            }
            let content = vw + max;
            let thumb_w = (vw * (vw / content)).max(px(20.));
            let off = -h.offset().x;
            let x = (vw - thumb_w) * (off / max);
            let r = Bounds::new(point(bounds.left() + x, bounds.top()), size(thumb_w, px(scrollbar::WIDTH)));
            window.paint_quad(fill(r, theme::hex(0x334155)).corner_radii(px(3.)));
        })
        .size_full(),
    )
}

const SYSTEM_PROMPT_HEAD: &str = "You are a Kubernetes network policy expert assistant. You analyze NetworkPolicies and help users understand traffic flow between workloads.

Answer questions clearly and concisely. Reference specific policy names when relevant. Use the graph context below to answer accurately.

";

// ── View ────────────────────────────────────────────────────────────────

struct Simulation {
    result: TrafficSimulationResult,
    source: String,
    dest: String,
}

struct NpMessage {
    msg: DisplayMessage,
    sim: Option<Simulation>,
    expanded: bool,
}

pub struct NetworkPolicyView {
    app: Entity<AppState>,
    namespace: Option<String>,
    context: Option<String>,
    graph: Option<Rc<NetworkPolicyGraph>>,
    loading: bool,
    /// The last refresh error. With a graph loaded it is shown as a small
    /// indicator in the stats bar and the last good graph stays (quirk 10.16,
    /// fixed); without one it is the full error state.
    error: Option<String>,
    messages: Vec<NpMessage>,
    streaming: bool,
    /// RBAC "Permission Denied" analysis of a 403 refresh error (spec 03 §7.8).
    forbidden: Entity<ForbiddenBanner>,
    /// The latest AI tool-status event ("Querying ..."), until the next chunk.
    tool_status: Option<String>,
    ai: Entity<AiConfigModel>,
    input: Entity<TextInput>,
    input_text: String,
    filter: Entity<TextInput>,
    policy_search: String,
    selected_policy: Option<String>,
    panel_open: bool,
    scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    /// The input is disabled while streaming, which drops its focus.
    blur_input: bool,
    strip_scroll: ScrollHandle,
    pods_version: u64,
    fetch_task: Option<Task<()>>,
    debounce_task: Option<Task<()>>,
    chat_task: Option<Task<()>>,
    sim_task: Option<Task<()>>,
    _timers: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

impl NetworkPolicyView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ai = ai_config::model(cx);
        let input = cx.new(|cx| {
            TextInput::new(
                "Ask about network policies...",
                InputStyle {
                    text_color: slate::S100,
                    placeholder_color: slate::S600,
                    font_size: 14.,
                    line_height: 20.,
                    padding: (12., 12., 8., 8.),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let filter = cx.new(|cx| {
            TextInput::new(
                "Filter policies...",
                InputStyle {
                    text_color: slate::S300,
                    placeholder_color: slate::S600,
                    font_family: theme::MONO.into(),
                    font_size: 11.,
                    line_height: 16.5,
                    padding: (24., 24., 4., 4.),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let a = app.read(cx);
        let namespace = (a.namespace != "*").then(|| a.namespace.clone());
        let context = a.context.clone();
        let pods_version = a.resources.read(cx).version;
        let resources = a.resources.clone();
        let subs = vec![
            cx.observe(&app, |this: &mut Self, app, cx| {
                let a = app.read(cx);
                let ns = (a.namespace != "*").then(|| a.namespace.clone());
                let ctx = a.context.clone();
                if ns != this.namespace || ctx != this.context {
                    this.namespace = ns;
                    this.context = ctx;
                    // Another scope's graph must not be shown as stale data.
                    this.graph = None;
                    this.fetch(cx);
                }
                cx.notify();
            }),
            // `resource://event` with kind == pods → refresh after a 2s debounce.
            cx.observe(&resources, |this: &mut Self, res, cx| {
                let v = res.read(cx).version;
                if v != this.pods_version {
                    this.pods_version = v;
                    if this.app.read(cx).kind == ResourceKind::Pods {
                        let t = backend::sleep(cx, Duration::from_secs(2));
                        this.debounce_task = Some(cx.spawn(async move |this, cx| {
                            t.await;
                            this.update(cx, |this, cx| this.fetch(cx)).ok();
                        }));
                    }
                }
            }),
            cx.observe(&ai, |this: &mut Self, _, cx| {
                this.update_placeholder(cx);
                cx.notify();
            }),
            cx.subscribe(&input, |this: &mut Self, _, ev: &InputEvent, cx| match ev {
                InputEvent::Change(t) => {
                    this.input_text = t.clone();
                    cx.notify();
                }
                // The input is disabled while unconfigured or streaming.
                InputEvent::Enter { shift: false } if this.configured(cx) => {
                    let t = this.input_text.clone();
                    this.send(t, cx);
                }
                _ => {}
            }),
            cx.subscribe(&filter, |this: &mut Self, _, ev: &InputEvent, cx| {
                if let InputEvent::Change(t) = ev {
                    this.policy_search = t.clone();
                    cx.notify();
                }
            }),
        ];
        // 30s polling (no NetworkPolicy watcher).
        let poll = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(30)).await;
            if this.update(cx, |this, cx| this.fetch(cx)).is_err() {
                return;
            }
        });
        // Focus the input 300ms after mount, unless the user already put focus in
        // a text field (e.g. the policy filter) in the meantime.
        let focus_t = backend::sleep(cx, Duration::from_millis(300));
        let input_f = input.clone();
        let focus = cx.spawn_in(window, async move |_, cx| {
            focus_t.await;
            cx.update(|window, cx| {
                if !crate::keys::text_entry_focused(window) {
                    input_f.update(cx, |i, cx| i.focus(window, cx));
                }
            })
            .ok();
        });
        let (forbidden, forbidden_sub) = forbidden_banner("np.forbidden", cx);
        let mut subs = subs;
        subs.push(forbidden_sub);
        let mut this = NetworkPolicyView {
            app,
            namespace,
            context,
            graph: None,
            loading: true,
            error: None,
            messages: Vec::new(),
            streaming: false,
            tool_status: None,
            forbidden,
            ai,
            input,
            input_text: String::new(),
            filter,
            policy_search: String::new(),
            selected_policy: None,
            panel_open: true,
            scroll: ScrollHandle::new(),
            gutter: Default::default(),
            blur_input: false,
            strip_scroll: ScrollHandle::new(),
            pods_version,
            fetch_task: None,
            debounce_task: None,
            chat_task: None,
            sim_task: None,
            _timers: vec![poll, focus],
            _subs: subs,
        };
        this.update_placeholder(cx);
        this.fetch(cx);
        this
    }

    fn configured(&self, cx: &App) -> bool {
        self.ai.read(cx).is_configured()
    }

    fn update_placeholder(&mut self, cx: &mut Context<Self>) {
        let p = if !self.configured(cx) {
            "Configure an AI provider to get started..."
        } else if self.streaming {
            "Waiting for response..."
        } else {
            "Ask about network policies..."
        };
        self.input.update(cx, |i, cx| i.set_placeholder(p, cx));
    }

    /// Refreshes the graph. A failure keeps the last good graph (the error is
    /// kept until the next successful refresh).
    fn fetch(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let ns = self.namespace.clone();
        self.fetch_task = Some(backend::run(cx, move |c| async move { c.build_network_policy_graph(ns).await }, |this: &mut Self, res, cx| {
            match res {
                Ok(g) => {
                    this.graph = Some(Rc::new(g));
                    this.error = None;
                }
                Err(e) => this.error = Some(backend::format_error(&e)),
            }
            let err = this.error.clone();
            this.forbidden.update(cx, |b, cx| b.set_error(err.as_deref(), cx));
            this.loading = false;
            cx.notify();
        }));
    }

    fn near_bottom(&self) -> bool {
        let max = self.scroll.max_offset().y;
        let off = -self.scroll.offset().y;
        f32::from(max - off) < 150.
    }

    fn push_message(&mut self, m: NpMessage, cx: &mut Context<Self>) {
        let stick = self.near_bottom();
        self.messages.push(m);
        if stick {
            self.scroll_to_bottom_soon(cx);
        }
        cx.notify();
    }

    fn scroll_to_bottom_soon(&mut self, cx: &mut Context<Self>) {
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// `sendMessage(text)`.
    fn send(&mut self, text: String, cx: &mut Context<Self>) {
        let trimmed = text.trim().to_string();
        let Some(graph) = self.graph.clone() else { return };
        if trimmed.is_empty() || self.streaming {
            return;
        }
        // Simulation first (async), then the chat request.
        let sim = detect_simulation(&trimmed, &graph.groups).and_then(|(s, d)| {
            let (sg, dg) = (&graph.groups[s], &graph.groups[d]);
            let (sp, dp) = (sg.pods.first()?, dg.pods.first()?);
            Some((sp.namespace.clone(), sp.name.clone(), dp.namespace.clone(), dp.name.clone(), sg.name.clone(), dg.name.clone()))
        });
        match sim {
            Some((sns, sp, dns, dp, sl, dl)) => {
                self.sim_task = Some(backend::run(
                    cx,
                    move |c| async move { c.simulate_network_traffic(sns, sp, dns, dp, None, None).await },
                    move |this: &mut Self, res, cx| {
                        let sim = res.ok().map(|result| Simulation { result, source: sl.clone(), dest: dl.clone() });
                        this.finish_send(trimmed.clone(), sim, cx);
                    },
                ));
            }
            None => self.finish_send(trimmed, None, cx),
        }
    }

    fn finish_send(&mut self, text: String, sim: Option<Simulation>, cx: &mut Context<Self>) {
        let Some(graph) = self.graph.clone() else { return };
        self.push_message(NpMessage { msg: DisplayMessage::user(text), sim, expanded: false }, cx);
        self.input.update(cx, |i, cx| i.set_text("", cx));
        self.input_text.clear();
        self.streaming = true;
        self.tool_status = None;
        self.blur_input = true;
        self.update_placeholder(cx);

        let total_pods: usize = graph.groups.iter().map(|g| g.pods.len()).sum();
        let system = format!(
            "{SYSTEM_PROMPT_HEAD}{}\n\nWhen a simulation result is provided with the user's question, explain WHY the traffic is allowed or denied based on the policies. Reference the specific policies and rules.\n\nAvailable workloads: {}\nTotal pods: {}",
            build_graph_context(&graph),
            graph.groups.iter().map(|g| g.name.clone()).collect::<Vec<_>>().join(", "),
            total_pods
        );
        let mut messages = vec![ChatMessage { role: "system".into(), content: system }];
        let start = self.messages.len().saturating_sub(10);
        for m in &self.messages[start..] {
            let mut content = m.msg.content.clone();
            if let Some(s) = &m.sim {
                content.push_str(&format!(
                    "\n\n[Simulation Result: {} → {}: {}. {}]",
                    s.source,
                    s.dest,
                    if s.result.allowed { "ALLOWED" } else { "DENIED" },
                    s.result.summary
                ));
            }
            messages.push(ChatMessage { role: m.msg.role.clone(), content });
        }
        let req = AIChatRequest { messages, session_id: ai_stream::session_id("np-ai"), namespace: self.namespace.clone() };
        let config = ai_config::current(cx);
        self.chat_task = Some(ai_stream::start_chat(cx, &config, req, |this: &mut Self, upd, cx| {
            match upd {
                AiUpdate::Events(evs) => {
                    for ev in evs {
                        match ev {
                            AIStreamEvent::Chunk { content } if !content.is_empty() => {
                                this.tool_status = None;
                                this.append_assistant(&content, cx)
                            }
                            // Tool calls ("Querying get cluster health...") replace
                            // the "Thinking..." text until the answer arrives
                            // (quirk 10.16, fixed: status events were ignored).
                            AIStreamEvent::Status { message } if !message.is_empty() => this.tool_status = Some(message),
                            AIStreamEvent::Done => {
                                this.streaming = false;
                                this.tool_status = None;
                            }
                            AIStreamEvent::Error { message } => {
                                this.streaming = false;
                                this.tool_status = None;
                                let msg = if message.is_empty() { "An unknown error occurred.".to_string() } else { message };
                                if this.messages.last().map(|m| !m.msg.is_user()).unwrap_or(false) {
                                    this.append_assistant(&format!("\n\n[Error: {msg}]"), cx);
                                } else {
                                    this.append_assistant(&format!("[Error: {msg}]"), cx);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                AiUpdate::Failed(e) => {
                    this.streaming = false;
                    this.tool_status = None;
                    this.push_message(NpMessage { msg: DisplayMessage::assistant(format!("[Error: {e}]")), sim: None, expanded: false }, cx);
                }
            }
            this.update_placeholder(cx);
            cx.notify();
        }));
        cx.notify();
    }

    fn append_assistant(&mut self, chunk: &str, cx: &mut Context<Self>) {
        let stick = self.near_bottom();
        match self.messages.last_mut() {
            Some(m) if !m.msg.is_user() => m.msg.content.push_str(chunk),
            _ => self.messages.push(NpMessage { msg: DisplayMessage::assistant(chunk), sim: None, expanded: false }),
        }
        if stick {
            self.scroll_to_bottom_soon(cx);
        }
    }

    // ── Rendering ───────────────────────────────────────────────────────

    fn render_state(icon_el: AnyElement, title: &str, title_color: Rgba, sub: Option<String>, sub_max: f32) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .text_color(slate::S400)
            .child(icon_el)
            .child(div().text_sm_tw().font_weight(FontWeight::MEDIUM).text_color(title_color).child(title.to_string()))
            .when_some(sub, |d, s| d.child(div().max_w(px(sub_max)).text_center().text_xs_tw().text_color(slate::S500).child(s)))
            .auto_id("view.network-policies", "region", title.to_string())
            .into_any_element()
    }

    fn render_stats(&self, g: &NetworkPolicyGraph, cx: &mut Context<Self>) -> impl IntoElement {
        let stat = |t: String| div().text_10().font_mono().text_color(slate::S500).child(t);
        div()
            .flex_none()
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
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(icon("shield", 12.).text_color(slate::S500))
                            .child(stat(format!("{} policies", g.policies.len())).auto_id("np.stats.policies", "text", format!("{} policies", g.policies.len()))),
                    )
                    .child(stat(format!("{} workloads", g.groups.len())).auto_id("np.stats.workloads", "text", format!("{} workloads", g.groups.len())))
                    .child(stat(format!("{} edges", g.edges.len())).auto_id("np.stats.edges", "text", format!("{} edges", g.edges.len()))),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .children(self.error.clone().map(|e| {
                        div()
                            .id("np-refresh-error")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .max_w(px(360.))
                            .text_10()
                            .text_color(red::R400)
                            .child(icon("alert-circle", 12.).text_color(red::R400))
                            .child(div().min_w_0().truncate().child(format!("Refresh failed: {e}")))
                            .auto_id("np.error", "status", e)
                    }))
                    .child(self.refresh_button(cx)),
            )
    }

    fn refresh_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
                    .id("np-refresh")
                    .group("np-refresh")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .bg(theme::hexa(0x101828, 0.8))
                    .border_1()
                    .border_color(slate::S800)
                    .text_11()
                    .line_height(px(16.5))
                    .cursor_pointer()
                    .child(icon("refresh-cw", 12.).text_color(slate::S400).group_hover("np-refresh", |s| s.text_color(slate::S300)))
                    .on_click(cx.listener(|this, _, _, cx| this.fetch(cx)))
                    .auto_id("np.refresh", "button", "Refresh")
    }

    fn render_sim_card(&self, ix: usize, s: &Simulation, expanded: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let r = &s.result;
        let (border, bg) = if r.allowed {
            (theme::hexa(0x10b981, 0.3), theme::hexa(0x10b981, 0.05))
        } else {
            (theme::hexa(0xef4444, 0.3), theme::hexa(0xef4444, 0.05))
        };
        let egress: Vec<_> = r.egress_evaluation.policy_results.iter().filter(|p| p.selects_pod).collect();
        let ingress: Vec<_> = r.ingress_evaluation.policy_results.iter().filter(|p| p.selects_pod).collect();
        let dir = |label: &str, isolated: bool, res: &[&kore_core::state::network_policy::PolicyEvaluation]| {
            let text = if isolated {
                "Isolated"
            } else if res.iter().any(|p| p.allows_traffic) {
                "Allowed"
            } else if !res.is_empty() {
                "Denied"
            } else {
                "No policy"
            };
            let auto = format!("np.sim.{ix}.{}", label.trim_end_matches(':').to_lowercase());
            let auto_label = format!("{label} {text}");
            let value: AnyElement = if isolated {
                div().text_color(amber::A400).child("Isolated").into_any_element()
            } else if res.iter().any(|p| p.allows_traffic) {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .text_color(emerald::E400)
                    .child(icon("check-circle-2", 10.).text_color(emerald::E400))
                    .child("Allowed")
                    .into_any_element()
            } else if !res.is_empty() {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .text_color(red::R400)
                    .child(icon("x-circle", 10.).text_color(red::R400))
                    .child("Denied")
                    .into_any_element()
            } else {
                div().text_color(slate::S500).child("No policy").into_any_element()
            };
            div().flex().flex_row().items_center().gap(px(4.)).child(div().text_color(slate::S500).child(label.to_string())).child(value).auto_id(auto, "text", auto_label)
        };
        let n = egress.len() + ingress.len();
        let detail_row = |k: usize, dirn: &str, p: &kore_core::state::network_policy::PolicyEvaluation| {
            let (bg, fg) = if p.allows_traffic {
                (theme::hexa(0x10b981, 0.1), emerald::E400)
            } else {
                (theme::hexa(0xef4444, 0.1), red::R400)
            };
            div()
                .px(px(8.))
                .py(px(4.))
                .rounded(px(4.))
                .text_10()
                .bg(bg)
                .text_color(fg)
                .child(crate::views::rbac::spans(vec![
                    (dirn.to_string(), slate::S500),
                    (format!(" {}: {}", p.policy_name, p.reason), fg),
                ]))
                .auto_id(format!("np.sim.{ix}.eval.{k}"), "text", format!("{dirn} {}: {}", p.policy_name, p.reason))
        };
        div()
            .max_w(relative(0.85))
            .min_w_0()
            .rounded(px(8.))
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_xs_tw()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .min_w_0()
                            .font_mono()
                            .text_color(slate::S300)
                            .child(div().truncate().child(s.source.clone()))
                            .child(icon("arrow-right", 14.).text_color(slate::S500))
                            .child(div().truncate().child(s.dest.clone()))
                            .auto_id(format!("np.sim.{ix}.route"), "text", format!("{} → {}", s.source, s.dest)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .text_10()
                            .font_weight(FontWeight::SEMIBOLD)
                            .map(|d| {
                                if r.allowed {
                                    d.bg(theme::hexa(0x10b981, 0.2))
                                        .text_color(emerald::E400)
                                        .child(icon("check-circle-2", 12.).text_color(emerald::E400))
                                        .child("ALLOWED")
                                } else {
                                    d.bg(theme::hexa(0xef4444, 0.2)).text_color(red::R400).child(icon("x-circle", 12.).text_color(red::R400)).child("DENIED")
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .px(px(12.))
                    .pb(px(8.))
                    .text_color(slate::S400)
                    .child(crate::ui::text::prose_text(r.summary.clone()))
                    .auto_id(format!("np.sim.{ix}.summary"), "text", r.summary.clone()),
            )
            .child(
                div()
                    .px(px(12.))
                    .pb(px(8.))
                    .flex()
                    .flex_row()
                    .gap(px(12.))
                    .child(dir("Egress:", r.egress_evaluation.isolated, &egress))
                    .child(dir("Ingress:", r.ingress_evaluation.isolated, &ingress)),
            )
            .when(n > 0, |d| {
                d.child(
                    div()
                        .id(("np-sim-toggle", ix))
                        .group("np-sim-toggle")
                        .w_full()
                        .px(px(12.))
                        .py(px(6.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .text_10()
                        .text_color(slate::S500)
                        .hover(|s| s.text_color(slate::S300))
                        .border_t_1()
                        .border_color(theme::hexa(0x1e293b, 0.3))
                        .cursor_pointer()
                        .child(
                            icon(if expanded { "chevron-up" } else { "chevron-down" }, 12.)
                                .text_color(slate::S500)
                                .group_hover("np-sim-toggle", |s| s.text_color(slate::S300)),
                        )
                        .child(format!("Policy details ({n} evaluations)"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = this.messages.get_mut(ix) {
                                m.expanded = !m.expanded;
                            }
                            cx.notify();
                        }))
                        .auto_id(format!("np.sim.{ix}.details"), "button", format!("Policy details ({n} evaluations)")),
                )
                .when(expanded, |d| {
                    d.child(
                        div()
                            .px(px(12.))
                            .pb(px(8.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .children(egress.iter().enumerate().map(|(k, p)| detail_row(k, "egress", p)))
                            .children(ingress.iter().enumerate().map(|(k, p)| detail_row(egress.len() + k, "ingress", p)))
                            .with_animation(("np-sim-expand", ix), Animation::new(Duration::from_millis(200)), |el, t| el.opacity(t)),
                    )
                })
            })
            .auto_id(format!("np.sim.{ix}"), "status", if r.allowed { "ALLOWED" } else { "DENIED" })
    }

    fn render_chat(&mut self, g: &NetworkPolicyGraph, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let configured = self.configured(cx);
        let questions = suggested_questions(g);
        let disabled = self.streaming || !configured;

        let mut list = div().flex().flex_col().gap(px(16.));
        if self.messages.is_empty() {
            list = list.size_full().child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .px(px(24.))
                    .child(div().size(px(56.)).rounded(px(16.)).flex().items_center().justify_center().mb(px(16.)).child(icon("bot", 28.).text_color(Rgba { a: 0.6, ..accent })))
                    .child(div().text_sm_tw().text_color(slate::S400).mb(px(4.)).child("Ask about your network policies"))
                    .child(
                        div()
                            .text_xs_tw()
                            .text_color(slate::S600)
                            .mb(px(24.))
                            .child("\"Can X talk to Y?\", \"Which pods are isolated?\", \"What does this policy do?\""),
                    )
                    .when(!configured, |d| {
                        d.child(div().mb(px(24.)).max_w(px(384.)).rounded(px(8.)).border_1().border_color(theme::hexa(0xf59e0b, 0.3)).overflow_hidden().child(crate::views::banners::ai_config_warning()))
                    })
                    .child(div().flex().flex_row().flex_wrap().justify_center().gap(px(8.)).max_w(px(512.)).children(questions.iter().enumerate().map(|(i, q)| {
                        let q2 = q.clone();
                        div()
                            .id(("np-q", i))
                            .px(px(12.))
                            .py(px(6.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(slate::S700)
                            .text_xs_tw()
                            .text_color(slate::S300)
                            .map(|d| if disabled { d.opacity(0.4) } else { d.cursor_pointer().hover(move |s| s.text_color(accent)) })
                            .child(q.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.streaming && this.configured(cx) {
                                    this.send(q2.clone(), cx);
                                }
                            }))
                            .auto_id(format!("np.question.{i}"), "button", q.clone())
                            .disabled(disabled)
                    }))),
            );
        } else {
            // Message row width: view - chat padding - policy panel - gutter.
            let row_width = f32::from(window.viewport_size().width)
                - crate::views::sidebar::WIDTH
                - 32.
                - if self.panel_open { 300. } else { 0. }
                - 32.
                - if self.gutter.overflows() { scrollbar::WIDTH } else { 0. };
            for i in 0..self.messages.len() {
                let m = &self.messages[i];
                let bubble = chat_bubble_sized(format!("np-msg-{i}"), &m.msg, BubbleVariant::Compact, !m.msg.is_user(), Some(row_width), window, cx);
                let mut item = div().flex().flex_col().child(bubble);
                if m.msg.is_user() {
                    if let Some(s) = &m.sim {
                        let expanded = m.expanded;
                        item = item.child(div().mt(px(8.)).flex().flex_row().justify_end().child(self.render_sim_card(i, s, expanded, cx)));
                    }
                }
                list = list.child(item);
            }
            if self.streaming && self.messages.last().map(|m| m.msg.is_user()).unwrap_or(true) {
                let status = self.tool_status.clone();
                list = list.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .text_xs_tw()
                        .text_color(slate::S500)
                        .when(status.is_some(), |d| d.child(icon("wrench", 14.).text_color(accent)))
                        .child(spinning_icon("np-thinking", "loader-2", 14., accent))
                        .child(status.clone().unwrap_or_else(|| "Thinking...".into()))
                        .auto_id("np.tool-status", "status", status.unwrap_or_else(|| "Thinking...".into())),
                );
            }
        }

        let gutter = self.gutter.overflows();
        let messages = div()
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_row()
            .child(
                div()
                    .id("np-messages")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px(px(16.))
                    .py(px(12.))
                    .child(list),
            )
            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone()));

        let strip = (!self.messages.is_empty() && !questions.is_empty()).then(|| {
            div()
                .flex_none()
                .px(px(16.))
                .py(px(8.))
                .border_t_1()
                .border_color(theme::hexa(0x1e293b, 0.3))
                .child(div().id("np-strip").flex().flex_row().gap(px(6.)).overflow_x_scroll().track_scroll(&self.strip_scroll).children(questions.iter().enumerate().map(|(i, q)| {
                    let q2 = q.clone();
                    div()
                        .id(("np-strip-q", i))
                        .flex_none()
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(slate::S800)
                        .text_10()
                        .text_color(slate::S400)
                        .whitespace_nowrap()
                        .map(|d| if disabled { d.opacity(0.4) } else { d.cursor_pointer().hover(move |s| s.text_color(accent)) })
                        .child(q.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.streaming && this.configured(cx) {
                                this.send(q2.clone(), cx);
                            }
                        }))
                        .auto_id(format!("np.strip.{i}"), "button", q.clone())
                })))
                .child(hscrollbar(self.strip_scroll.clone()))
        });

        let focused = self.input.read(cx).is_focused(window);
        let can_send = !self.input_text.trim().is_empty() && !self.streaming && configured;
        let has_messages = !self.messages.is_empty();
        let form = div()
            .flex_none()
            .px(px(16.))
            .py(px(12.))
            .border_t_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.8))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .when(has_messages, |d| {
                        let streaming = self.streaming;
                        d.child(
                            div()
                                .id("np-clear")
                                .group("np-clear")
                                .flex_none()
                                .p(px(8.))
                                .rounded(px(8.))
                                .map(|d| if streaming { d.opacity(0.4) } else { d.cursor_pointer().hover(|s| s.bg(theme::hexa(0x1d2939, 0.5))) })
                                .child(icon("trash-2", 16.).text_color(slate::S500).when(!streaming, |s| s.group_hover("np-clear", |s| s.text_color(red::R400))))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.streaming {
                                        this.messages.clear();
                                        cx.notify();
                                    }
                                }))
                                .auto_id("np.clear", "button", "Clear chat"),
                        )
                    })
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(theme::BACKGROUND)
                            .border_1()
                            .border_color(slate::S800)
                            .when(focused, |d| d.child(crate::views::rbac::focus_ring(cx).rounded(px(10.))))
                            .when(disabled, |d| d.opacity(0.5))
                            .child(self.input.clone())
                            .auto_id("np.input", "textbox", "Ask about network policies"),
                    )
                    .child(
                        div()
                            .id("np-send")
                            .flex_none()
                            .p(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .map(|d| {
                                if can_send {
                                    d.border_color(theme::PREFLIGHT_BORDER).cursor_pointer()
                                } else {
                                    d.bg(theme::hexa(0x1d2939, 0.3)).border_color(slate::S800)
                                }
                            })
                            .child(icon("send", 16.).text_color(if can_send { accent } else { slate::S600 }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let t = this.input_text.clone();
                                if !t.trim().is_empty() && this.configured(cx) {
                                    this.send(t, cx);
                                }
                            }))
                            .auto_id("np.send", "button", "Send message")
                            .disabled(!can_send),
                    ),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_10()
                    .text_color(slate::S600)
                    .text_center()
                    .child("AI responses may be inaccurate. Simulation results are computed locally."),
            );

        div().flex_1().min_w_0().flex().flex_col().overflow_hidden().child(messages).children(strip).child(form)
    }

    fn render_policy_card(&self, p: &PolicySummary, cx: &mut Context<Self>) -> impl IntoElement {
        let rules_label = |p: &PolicySummary| {
            let mut v = Vec::new();
            if p.ingress_rule_count > 0 {
                v.push(format!("{} ingress rule{}", p.ingress_rule_count, if p.ingress_rule_count > 1 { "s" } else { "" }));
            }
            if p.egress_rule_count > 0 {
                v.push(format!("{} egress rule{}", p.egress_rule_count, if p.egress_rule_count > 1 { "s" } else { "" }));
            }
            v.join(", ")
        };
        let accent = theme::accent(cx);
        let selected = self.selected_policy.as_deref() == Some(p.name.as_str());
        let name = p.name.clone();
        let pill = |t: String, bg: Rgba, fg: Rgba| div().px(px(6.)).py(px(2.)).rounded(px(4.)).text_9().font_weight(FontWeight::MEDIUM).bg(bg).text_color(fg).child(t);
        let pairs = selector_pairs(&p.pod_selector);
        div()
            .id(SharedString::from(format!("np-card-{}", p.name)))
            .w_full()
            .p(px(10.))
            .rounded(px(8.))
            .border_1()
            .cursor_pointer()
            .map(|d| {
                if selected {
                    d.border_color(theme::PREFLIGHT_BORDER)
                } else {
                    d.border_color(theme::hexa(0x1e293b, 0.5)).bg(theme::hexa(0x0b1221, 0.5)).hover(|s| s.border_color(slate::S700))
                }
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .mb(px(6.))
                    .child(icon("shield", 12.).text_color(accent))
                    .child(div().min_w_0().truncate().text_11().font_weight(FontWeight::MEDIUM).text_color(slate::S200).child(p.name.clone())),
            )
            .child(
                div()
                    .text_10()
                    .text_color(slate::S500)
                    .mb(px(6.))
                    .child(format!("ns: {}", p.namespace))
                    .auto_id(format!("np.policy.{}.ns", p.name), "text", format!("ns: {}", p.namespace)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(4.))
                    .mb(px(6.))
                    .children(p.policy_types.iter().map(|t| {
                        if t == "Ingress" {
                            pill(t.clone(), theme::hexa(0x10b981, 0.15), emerald::E400)
                        } else {
                            pill(t.clone(), theme::hexa(0x3b82f6, 0.15), misc::BLUE400)
                        }
                    }))
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .text_9()
                            .bg(slate::S800)
                            .text_color(slate::S400)
                            .child(format!("{} pod{}", p.affected_pod_count, if p.affected_pod_count != 1 { "s" } else { "" })),
                    )
                    .auto_id(format!("np.policy.{}.types", p.name), "text", p.policy_types.join(", ")),
            )
            .map(|d| {
                if pairs.is_empty() {
                    d.child(
                        div().flex().child(
                            div()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(4.))
                                .text_9()
                                .font_mono()
                                .italic()
                                .bg(theme::hexa(0x1e293b, 0.8))
                                .text_color(slate::S400)
                                .child("all pods"),
                        )
                        .auto_id(format!("np.policy.{}.selector", p.name), "text", "all pods"),
                    )
                } else {
                    let label = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ");
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(px(4.))
                            .children(pairs.iter().map(|(k, v)| {
                                div().text_9().px(px(6.)).py(px(2.)).rounded(px(4.)).bg(theme::hexa(0x1e293b, 0.8)).text_color(slate::S400).font_mono().child(format!("{k}={v}"))
                            }))
                            .auto_id(format!("np.policy.{}.selector", p.name), "text", label),
                    )
                }
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .mt(px(6.))
                    .text_9()
                    .text_color(slate::S500)
                    .when(p.ingress_rule_count > 0, |d| {
                        d.child(format!("{} ingress rule{}", p.ingress_rule_count, if p.ingress_rule_count > 1 { "s" } else { "" }))
                    })
                    .when(p.egress_rule_count > 0, |d| {
                        d.child(format!("{} egress rule{}", p.egress_rule_count, if p.egress_rule_count > 1 { "s" } else { "" }))
                    })
                    .auto_id(format!("np.policy.{}.rules", p.name), "text", rules_label(p)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_policy = if this.selected_policy.as_deref() == Some(name.as_str()) { None } else { Some(name.clone()) };
                cx.notify();
            }))
            .auto_id(format!("np.policy.{}", p.name), "button", p.name.clone())
            .selected(selected)
    }

    fn render_panel(&self, g: &NetworkPolicyGraph, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let open = self.panel_open;
        let q = self.policy_search.to_lowercase();
        let filtered: Vec<&PolicySummary> =
            g.policies.iter().filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q) || p.namespace.to_lowercase().contains(&q)).collect();
        let focused = self.filter.read(cx).is_focused(window);
        let toggle = div()
            .id("np-panel-toggle")
            .group("np-toggle")
            .absolute()
            .left(px(-24.))
            .top(px(12.))
            .w(px(24.))
            .h(px(32.))
            .bg(theme::hexa(0x101828, 0.9))
            .border_t_1()
            .border_b_1()
            .border_l_1()
            .border_color(slate::S800)
            .rounded_tl(px(6.))
            .rounded_bl(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(
                icon(if open { "chevron-down" } else { "chevron-right" }, 12.)
                    .text_color(slate::S500)
                    .group_hover("np-toggle", |s| s.text_color(slate::S300))
                    .with_transformation(Transformation::rotate(percentage(0.75))), // -90deg; percentage() debug-asserts 0..=1
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.panel_open = !this.panel_open;
                cx.notify();
            }))
            .auto_id("np.panel.toggle", "button", if open { "Collapse policies" } else { "Expand policies" });
        let has_search = !self.policy_search.is_empty();
        let panel = open.then(|| {
            div()
                .w(px(300.))
                .h_full()
                .glass()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .p(px(12.))
                        .border_b_1()
                        .border_color(theme::hexa(0x1e293b, 0.5))
                        .child(
                            div()
                                .mb(px(8.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .text_xs_tw()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(slate::S300)
                                .child(icon("shield", 14.).text_color(accent))
                                .child(format!("Network Policies ({})", g.policies.len())),
                        )
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .h(px(26.5))
                                .bg(theme::BACKGROUND)
                                .border_1()
                                .border_color(slate::S800)
                                .rounded(px(4.))
                                .when(focused, |d| d.child(crate::views::rbac::focus_ring(cx)))
                                .child(self.filter.clone())
                                .child(div().absolute().left(px(7.)).top_0().bottom_0().flex().items_center().child(icon("search", 12.).text_color(slate::S500)))
                                .when(has_search, |d| {
                                    d.child(
                                        div()
                                            .id("np-filter-clear")
                                            .group("np-filter-clear")
                                            .absolute()
                                            .right(px(5.))
                                            .top_0()
                                            .bottom_0()
                                            .flex()
                                            .items_center()
                                            .cursor_pointer()
                                            .child(icon("x", 12.).text_color(slate::S500).group_hover("np-filter-clear", |s| s.text_color(slate::S300)))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.policy_search.clear();
                                                this.filter.update(cx, |f, cx| f.set_text("", cx));
                                                cx.notify();
                                            })),
                                    )
                                })
                                .auto_id("np.filter", "textbox", "Filter policies..."),
                        ),
                )
                .child(
                    div()
                        .id("np-policy-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(filtered.iter().map(|p| self.render_policy_card(p, cx).into_any_element()).collect::<Vec<_>>())
                        .when(filtered.is_empty(), |d| d.child(div().py(px(16.)).text_center().text_10().text_color(slate::S600).child("No matching policies"))),
                )
        });
        div().relative().flex_none().h_full().w(px(if open { 300. } else { 0. })).child(toggle).children(panel)
    }
}

impl Render for NetworkPolicyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        if std::mem::take(&mut self.blur_input) && self.input.read(cx).is_focused(window) {
            window.blur(cx);
        }
        if self.loading && self.graph.is_none() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .child(spinning_icon("np-loading", "loader-2", 32., accent))
                .child(div().text_sm_tw().text_color(slate::S400).child("Loading network policies..."))
                .auto_id("view.network-policies", "region", "Loading network policies...")
                .into_any_element();
        }
        if let (Some(e), None) = (&self.error, &self.graph) {
            let state = Self::render_state(
                icon("shield-x", 40.).text_color(red::R400).into_any_element(),
                "Failed to load network policies",
                red::R400,
                Some(e.clone()),
                448.,
            );
            if !self.forbidden.read(cx).visible() {
                return state;
            }
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().flex_1().min_h_0().child(state))
                .child(div().flex_none().p(px(16.)).child(self.forbidden.clone()))
                .into_any_element();
        }
        let Some(g) = self.graph.clone() else { return div().into_any_element() };
        if g.groups.is_empty() && g.policies.is_empty() {
            let sub = match &self.namespace {
                Some(ns) => format!("No NetworkPolicies in \"{ns}\""),
                None => "Select a namespace to view network policies".to_string(),
            };
            return Self::render_state(icon("inbox", 48.).text_color(slate::S600).into_any_element(), "No network policies found", slate::S400, Some(sub), 10000.);
        }
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(self.render_stats(&g, cx))
            .when(self.forbidden.read(cx).visible(), |d| d.child(div().flex_none().px(px(16.)).pt(px(12.)).child(self.forbidden.clone())))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .child(self.render_chat(&g, window, cx))
                    .child(self.render_panel(&g, window, cx)),
            )
            .with_animation("np-in", Animation::new(Duration::from_millis(300)), |el, t| el.opacity(t))
            .auto_id("view.network-policies", "region", "Network Policies")
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_simulation, suggested_questions};
    use kore_core::state::network_policy::{NetworkPolicyGraph, PodGroup, PodInfo};
    use std::collections::HashMap;

    fn group(name: &str, pod: &str) -> PodGroup {
        PodGroup {
            id: format!("Deployment/ns/{name}"),
            name: name.into(),
            namespace: "ns".into(),
            kind: "Deployment".into(),
            pod_count: 1,
            labels: HashMap::new(),
            is_isolated_ingress: true,
            is_isolated_egress: false,
            matching_policies: vec![],
            pods: vec![PodInfo { name: pod.into(), namespace: "ns".into(), labels: HashMap::new(), status: "Running".into() }],
        }
    }

    #[test]
    fn detects_simulation_questions() {
        let gs = vec![group("logger", "logger-abc"), group("web", "web-xyz")];
        assert_eq!(detect_simulation("can logger talk to web", &gs), Some((0, 1)));
        assert_eq!(detect_simulation("Can web reach to logger?", &gs), Some((1, 0)));
        // The pattern requires "to"/"with" after the verb.
        assert_eq!(detect_simulation("Can web reach logger?", &gs), None);
        assert_eq!(detect_simulation("can web-xyz connect with log??", &gs), Some((1, 0)));
        assert_eq!(detect_simulation("can web talk to web", &gs), None);
        assert_eq!(detect_simulation("what does it do", &gs), None);
    }

    #[test]
    fn suggestions_are_capped_at_four() {
        let g = NetworkPolicyGraph { groups: vec![group("a", "a-1"), group("b", "b-1")], external_cidrs: vec![], edges: vec![], policies: vec![] };
        let q = suggested_questions(&g);
        assert_eq!(q, vec!["Can a talk to b?", "Which pods are isolated and why?", "Summarize the network security posture"]);
    }
}
