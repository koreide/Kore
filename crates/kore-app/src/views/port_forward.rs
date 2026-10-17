//! Port-forward sidebar of the pod view (spec 02 §10). Fix 6 lives in the
//! backend (`--context` of the selected context).

use crate::automation::Automation;
use crate::backend;
use crate::state::resource_item::ResourceItem;
use crate::state::AppState;
use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::native_controls::number_stepper;
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::spinning_icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PfStatus {
    Connecting,
    Active,
    Error,
}

#[derive(Clone, Debug)]
pub struct Forward {
    pub id: String,
    pub local_port: u16,
    pub pod_port: u16,
    pub status: PfStatus,
    pub message: String,
}

/// JS `parseInt(s, 10)`: optional sign + leading digits, `None` for NaN.
pub fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let v: i64 = digits.parse().unwrap_or(i64::MAX);
    Some(if neg { -v } else { v })
}

fn random_port() -> u16 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    (49152 + h.finish() % (65535 - 49152 + 1)) as u16
}

/// Validate the form. `Ok((local, pod))` or the error text.
pub fn validate(pod_input: &str, local_input: &str, in_use: impl Fn(u16) -> bool, mut rand: impl FnMut() -> u16) -> Result<(u16, u16), String> {
    let pod = match parse_int(pod_input) {
        Some(p) if (1..=65535).contains(&p) => p as u16,
        _ => return Err("Pod port must be a valid number between 1 and 65535".into()),
    };
    let input_local = parse_int(local_input);
    if local_input.trim().is_empty() || input_local == Some(0) {
        let mut attempts = 0;
        loop {
            let p = rand();
            attempts += 1;
            if attempts > 100 {
                return Err("Unable to find an available port. Please specify a port manually.".into());
            }
            if !in_use(p) {
                return Ok((p, pod));
            }
        }
    }
    match input_local {
        Some(l) if (1..=65535).contains(&l) => {
            let l = l as u16;
            if in_use(l) {
                Err(format!("Port {l} is already in use by another port forward"))
            } else {
                Ok((l, pod))
            }
        }
        _ => Err("Local port must be a valid number between 1 and 65535".into()),
    }
}

pub struct PortForwardPanel {
    #[allow(dead_code)]
    app: Entity<AppState>,
    item: ResourceItem,
    namespace: String,
    forwards: Vec<Forward>,
    pod_input: Entity<TextInput>,
    local_input: Entity<TextInput>,
    starting: bool,
    error: Option<String>,
    tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

fn input_style() -> InputStyle {
    InputStyle {
        text_color: slate::S100,
        placeholder_color: slate::S500,
        font_size: 14.,
        line_height: 20.,
        padding: (12., 32., 8., 8.),
        ..Default::default()
    }
}

impl PortForwardPanel {
    pub fn new(app: Entity<AppState>, item: ResourceItem, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let namespace = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let pod_input = cx.new(|cx| {
            let mut i = TextInput::new("80", input_style(), window, cx);
            i.set_text("80", cx);
            i
        });
        let local_input = cx.new(|cx| {
            let mut i = TextInput::new("8080", input_style(), window, cx);
            i.set_text("8080", cx);
            i
        });
        let on_input = |this: &mut Self, _: Entity<TextInput>, ev: &InputEvent, cx: &mut Context<Self>| match ev {
            InputEvent::Change(_) => {
                this.error = None;
                cx.notify();
            }
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
            _ => {}
        };
        let subs = vec![cx.subscribe(&pod_input, on_input), cx.subscribe(&local_input, on_input)];
        PortForwardPanel {
            app,
            item,
            namespace,
            forwards: Vec::new(),
            pod_input,
            local_input,
            starting: false,
            error: None,
            tasks: Vec::new(),
            _subs: subs,
        }
    }

    /// Number of active (LIVE) forwards; the pod view colors Ports with it.
    pub fn active_count(&self) -> usize {
        self.forwards.iter().filter(|f| f.status == PfStatus::Active).count()
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.starting || self.pod_input.read(cx).text().is_empty() {
            return;
        }
        self.error = None;
        let pod_s = self.pod_input.read(cx).text().to_string();
        let local_s = self.local_input.read(cx).text().to_string();
        let active: Vec<u16> = self.forwards.iter().filter(|f| f.status == PfStatus::Active).map(|f| f.local_port).collect();
        let (local, pod) = match validate(&pod_s, &local_s, |p| active.contains(&p), random_port) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        self.starting = true;
        let id = format!("{}/{}/{}/{}", self.namespace, self.item.name, local, pod);
        self.forwards.push(Forward {
            id: id.clone(),
            local_port: local,
            pod_port: pod,
            status: PfStatus::Connecting,
            message: "Connecting...".into(),
        });
        let (ns, name) = (self.namespace.clone(), self.item.name.clone());
        let task = backend::run(
            cx,
            move |core| async move { core.start_port_forward(ns, name, local, pod).await },
            move |this: &mut Self, res, cx| {
                this.starting = false;
                match res {
                    Ok(v) => {
                        let port = v["localPort"].as_u64().filter(|p| *p > 0).map(|p| p as u16).unwrap_or(local);
                        if let Some(f) = this.forwards.iter_mut().find(|f| f.id == id) {
                            f.status = PfStatus::Active;
                            f.message = "Connected".into();
                            f.local_port = port;
                        }
                        toast(cx, format!("Port forward active on :{port}"), ToastKind::Success);
                        this.pod_input.update(cx, |i, cx| i.set_text("80", cx));
                        this.local_input.update(cx, |i, cx| i.set_text("8080", cx));
                    }
                    Err(e) => {
                        let msg = backend::format_error(&e);
                        if let Some(f) = this.forwards.iter_mut().find(|f| f.id == id) {
                            f.status = PfStatus::Error;
                            f.message = msg.clone();
                        }
                        this.error = Some(msg);
                    }
                }
                cx.notify();
            },
        );
        self.tasks.push(task);
        cx.notify();
    }

    fn stop(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(f) = self.forwards.iter().find(|f| f.id == id).cloned() else { return };
        let (ns, name) = (self.namespace.clone(), self.item.name.clone());
        let task = backend::run(
            cx,
            move |core| async move { core.stop_port_forward(ns, name, f.local_port, f.pod_port).await },
            move |this: &mut Self, res, cx| {
                match res {
                    Ok(()) => this.forwards.retain(|x| x.id != id),
                    Err(e) => this.error = Some(backend::format_error(&e)),
                }
                cx.notify();
            },
        );
        self.tasks.retain(|t| !t.is_ready());
        self.tasks.push(task);
    }

    fn field(&self, label: AnyElement, input: &Entity<TextInput>, id: &'static str, focused: bool, accent: Rgba, cx: &App) -> impl IntoElement {
        let value = input.read(cx).text().to_string();
        div()
            .child(div().mb(px(6.)).text_xs_tw().text_color(slate::S400).child(label))
            .child(
                div()
                    .relative()
                    .w_full()
                    .bg(theme::BACKGROUND)
                    .border_1()
                    .border_color(if focused { accent } else { slate::S800 })
                    .rounded(px(4.))
                    .when(self.starting, |d| d.opacity(0.5))
                    .child(input.clone())
                    .child(
                        div()
                            .absolute()
                            .right(px(12.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(number_stepper(slate::S300)),
                    )
                    .auto_id(id, "spinbutton", value),
            )
    }

    fn render_card(&self, f: &Forward, accent: Rgba, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, border, text, label) = match f.status {
            PfStatus::Active => (theme::hexa(0x22c55e, 0.1), theme::hexa(0x22c55e, 0.5), theme::hex(0x4ade80), "LIVE"),
            PfStatus::Connecting => (theme::hexa(0xeab308, 0.1), theme::hexa(0xeab308, 0.5), theme::hex(0xfacc15), "CONNECTING"),
            PfStatus::Error => (theme::hexa(0xef4444, 0.1), theme::hexa(0xef4444, 0.5), red::R400, "ERROR"),
        };
        let indicator: AnyElement = match f.status {
            PfStatus::Active => div()
                .relative()
                .size(px(8.))
                .child(div().absolute().inset_0().rounded_full().bg(theme::hex(0x22c55e)))
                .child(
                    div().absolute().inset_0().with_animation(
                        SharedString::from(format!("pf-ping-{}", f.id)),
                        Animation::new(Duration::from_millis(1000)).repeat(),
                        |el, t| {
                            let p = (t / 0.75).min(1.);
                            let e = 1. - (1. - p).powi(3);
                            let g = 4. * e;
                            el.child(
                                div()
                                    .absolute()
                                    .top(px(-g))
                                    .left(px(-g))
                                    .size(px(8. + 2. * g))
                                    .rounded_full()
                                    .bg(theme::hexa(0x22c55e, 0.75 * (1. - e))),
                            )
                        },
                    ),
                )
                .into_any_element(),
            PfStatus::Connecting => spinning_icon(SharedString::from(format!("pf-spin-{}", f.id)), "loader-2", 12., theme::hex(0xfacc15)).into_any_element(),
            PfStatus::Error => icon("alert-circle", 12.).text_color(red::R400).into_any_element(),
        };
        let active = f.status == PfStatus::Active;
        let port = f.local_port;
        let id = f.id.clone();
        let addr = format!("127.0.0.1:{port}");
        div()
            .p(px(12.))
            .rounded(px(4.))
            .border_1()
            .bg(bg)
            .border_color(border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .justify_between()
                    .mb(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(indicator)
                            .child(div().text_xs_tw().font_weight(FontWeight::SEMIBOLD).text_color(text).child(label)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("pf-stop-{}", f.id)))
                            .group("pf-stop")
                            .p(px(4.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hexa(0xef4444, 0.2)))
                            .child(icon("x", 16.).text_color(red::R400).group_hover("pf-stop", |s| s.text_color(red::R300)))
                            .on_click(cx.listener(move |this, _, _, cx| this.stop(id.clone(), cx)))
                            .auto_id(format!("pf.stop.{port}"), "button", "Stop port forward"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .text_xs_tw()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_color(slate::S400).child("Local:"))
                            .when(active, |d| {
                                let url = format!("http://127.0.0.1:{port}");
                                d.child(
                                    div()
                                        .id(SharedString::from(format!("pf-link-{port}")))
                                        .group("pf-link")
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(4.))
                                        .font_mono()
                                        .text_color(accent)
                                        .cursor_pointer()
                                        .hover(move |s| s.text_color(Rgba { a: 0.8, ..accent }).underline())
                                        .child(addr.clone())
                                        .child(
                                            icon("external-link", 12.)
                                                .text_color(accent)
                                                .group_hover("pf-link", move |s| s.text_color(Rgba { a: 0.8, ..accent })),
                                        )
                                        .on_click(move |_, _, cx| cx.open_url(&url))
                                        .auto_id(format!("pf.link.{port}"), "link", addr.clone()),
                                )
                            })
                            .when(!active, |d| d.child(div().font_mono().text_color(slate::S300).child(addr.clone()))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_color(slate::S400).child("Pod:"))
                            .child(div().font_mono().text_color(slate::S300).child(format!("{}:{}", self.item.name, f.pod_port)))
                            .auto_id(format!("pf.card.{port}.pod"), "text", format!("{}:{}", self.item.name, f.pod_port)),
                    )
                    .when(!f.message.is_empty(), |d| {
                        d.child(
                            div()
                                .mt(px(4.))
                                .text_color(slate::S500)
                                .child(f.message.clone())
                                .auto_id(format!("pf.card.{port}.message"), "text", f.message.clone()),
                        )
                    }),
            )
            .with_animation(
                SharedString::from(format!("pf-card-{}", f.id)),
                Animation::new(Duration::from_millis(200)),
                |el, t| el.opacity(t),
            )
            .auto_id(format!("pf.card.{port}"), "status", label)
    }
}

impl Render for PortForwardPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let pod_focused = self.pod_input.read(cx).is_focused(window);
        let local_focused = self.local_input.read(cx).is_focused(window);
        let disabled = self.starting || self.pod_input.read(cx).text().is_empty();
        let cards: Vec<AnyElement> = self.forwards.clone().iter().map(|f| self.render_card(f, accent, cx).into_any_element()).collect();
        let pod_label = div().child("Pod Port").into_any_element();
        let local_label = div()
            .flex()
            .flex_row()
            .child("Local Port")
            .child(div().ml(px(4.)).text_color(slate::S500).child("(leave blank for auto)"))
            .into_any_element();
        div()
            .w(px(320.))
            .h_full()
            .flex_none()
            .border_l_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(16.))
                    .border_b_1()
                    .border_color(slate::S800)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(8.))
                            .child(icon("plug", 20.).text_color(accent))
                            .child(div().text_lg_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("Port Forwarding")),
                    )
                    .child(div().text_xs_tw().text_color(slate::S400).child("Forward local ports to pod ports")),
            )
            .child(
                div()
                    .p(px(16.))
                    .border_b_1()
                    .border_color(slate::S800)
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(self.field(pod_label, &self.pod_input, "pf.podport", pod_focused, accent, cx))
                    .child(self.field(local_label, &self.local_input, "pf.localport", local_focused, accent, cx))
                    .when_some(self.error.clone(), |d, e| {
                        d.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .p(px(8.))
                                .bg(theme::hexa(0xef4444, 0.1))
                                .border_1()
                                .border_color(theme::hexa(0xef4444, 0.5))
                                .rounded(px(4.))
                                .text_xs_tw()
                                .text_color(red::R400)
                                .child(icon("alert-circle", 16.).text_color(red::R400))
                                .child(div().flex_1().min_w_0().child(e.clone()))
                                .with_animation("pf-error", Animation::new(Duration::from_millis(200)), |el, t| {
                                    el.opacity(t).mt(px(-10. * (1. - t)))
                                })
                                .auto_id("pf.error", "alert", e),
                        )
                    })
                    .child(
                        div()
                            .id("pf-start")
                            .w_full()
                            .px(px(16.))
                            .py(px(8.))
                            .border_1()
                            .border_color(accent)
                            .rounded(px(4.))
                            .text_sm_tw()
                            .text_color(accent)
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .when(disabled, |d| d.opacity(0.5).cursor_not_allowed())
                            .when(!disabled, |d| d.cursor_pointer())
                            .when(self.starting, |d| {
                                d.child(spinning_icon("pf-starting", "loader-2", 16., accent)).child("Starting...")
                            })
                            .when(!self.starting, |d| d.child(icon("plug", 16.).text_color(accent)).child("Start Forward"))
                            .on_click(cx.listener(|this, _, _, cx| this.start(cx)))
                            .auto_id("pf.start", "button", "Start Forward")
                            .disabled(disabled),
                    ),
            )
            .child(
                div()
                    .id("pf-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .when(self.forwards.is_empty(), |d| {
                        d.child(
                            div()
                                .py(px(32.))
                                .flex()
                                .justify_center()
                                .text_sm_tw()
                                .text_color(slate::S500)
                                .child("No active port forwards")
                                .auto_id("pf.empty", "status", "No active port forwards"),
                        )
                    })
                    .children(cards),
            )
            .with_animation("pf-enter", Animation::new(Duration::from_millis(300)), |el, t| {
                let e = 1. - (1. - t).powi(3);
                el.opacity(e).ml(px(-20. * (1. - e)))
            })
            .auto_id("view.port-forward", "region", "Port Forwarding")
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_int, validate};

    #[test]
    fn parse() {
        assert_eq!(parse_int("80"), Some(80));
        assert_eq!(parse_int(" 18080x"), Some(18080));
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("-1"), Some(-1));
        assert_eq!(parse_int("abc"), None);
    }

    #[test]
    fn validation() {
        let none = |_: u16| false;
        assert_eq!(validate("80", "18080", none, || 50000), Ok((18080, 80)));
        assert_eq!(validate("70000", "8080", none, || 50000), Err("Pod port must be a valid number between 1 and 65535".into()));
        assert_eq!(validate("", "8080", none, || 50000), Err("Pod port must be a valid number between 1 and 65535".into()));
        assert_eq!(validate("80", "", none, || 50000), Ok((50000, 80)));
        assert_eq!(validate("80", "0", none, || 50001), Ok((50001, 80)));
        assert_eq!(validate("80", "70000", none, || 1), Err("Local port must be a valid number between 1 and 65535".into()));
        assert_eq!(
            validate("80", "18080", |p| p == 18080, || 1),
            Err("Port 18080 is already in use by another port forward".into())
        );
        assert_eq!(
            validate("80", "", |_| true, || 50000),
            Err("Unable to find an available port. Please specify a port manually.".into())
        );
    }
}
