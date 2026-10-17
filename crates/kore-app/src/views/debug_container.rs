//! Debug container modal (spec 02 §11). Open from creation; the owner drops
//! it after `DebugEvent::Created` / `Cancelled`. Registers itself as an open
//! modal (Fix 4) so Esc does not navigate back; Esc closes the dialog.

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::resource_item::ResourceItem;
use crate::state::AppState;
use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::native_controls::{checkbox, radio, NativeSelect, SelectEvent, SelectStyle};
use crate::ui::text::caps;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;

pub enum DebugEvent {
    Created { container: String, image: String },
    Cancelled,
}

impl EventEmitter<DebugEvent> for DebugContainerModal {}

/// (label, image, description)
pub const DEBUG_IMAGES: [(&str, &str, &str); 4] = [
    ("netshoot", "nicolaka/netshoot", "Network troubleshooting"),
    ("busybox", "busybox:latest", "Minimal Unix utilities"),
    ("ubuntu", "ubuntu:latest", "Full Ubuntu environment"),
    ("curl", "curlimages/curl", "cURL for HTTP debugging"),
];
const CUSTOM: &str = "custom";
const MODAL_ID: &str = "debug-container";

/// The image that will be created (`custom` → trimmed custom text).
pub fn resolve_image(choice: &str, custom: &str) -> String {
    if choice == CUSTOM {
        custom.trim().to_string()
    } else {
        choice.to_string()
    }
}

/// Regular (non-init) container names of the pod JSON.
pub fn regular_containers(pod: &serde_json::Value) -> Vec<String> {
    pod["spec"]["containers"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str().map(String::from)).collect()
}

pub struct DebugContainerModal {
    app: Entity<AppState>,
    item: ResourceItem,
    namespace: String,
    image: String,
    custom: Entity<TextInput>,
    share: bool,
    target: Entity<NativeSelect>,
    containers: Vec<String>,
    loading: bool,
    error: Option<String>,
    task: Option<Task<()>>,
    describe: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl DebugContainerModal {
    pub fn new(app: Entity<AppState>, item: ResourceItem, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let namespace = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let custom = cx.new(|cx| {
            TextInput::new(
                "e.g. alpine:latest",
                InputStyle {
                    text_color: slate::S200,
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
        let weak = app.downgrade();
        let target = cx.new(|cx| {
            NativeSelect::new(
                "debug.target",
                vec![(String::new(), "Select a container...".into())],
                String::new(),
                SelectStyle {
                    padding: (12., 8.),
                    radius: 6.,
                    font_size: 14.,
                    line_height: 20.,
                    text_color: slate::S200,
                    bg: theme::BACKGROUND,
                    border: slate::S800,
                    full_width: true,
                },
                weak,
                cx,
            )
        });
        let subs = vec![
            cx.subscribe(&custom, |_, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change(_) | InputEvent::Focus | InputEvent::Blur) {
                    cx.notify();
                }
            }),
            cx.subscribe(&target, |_, _, _: &SelectEvent, cx| cx.notify()),
            keys::on_window_key(cx, |this: &mut Self, key, _window, cx| {
                if key.key == "escape" && !this.target.read(cx).is_open() {
                    if !this.loading {
                        this.cancel(cx);
                    }
                    return true;
                }
                false
            }),
        ];
        let a = app.clone();
        cx.on_release(move |_, cx| {
            a.update(cx, |s, cx| s.set_modal(MODAL_ID, false, cx));
        })
        .detach();
        let a = app.clone();
        cx.defer(move |cx| a.update(cx, |s, cx| s.set_modal(MODAL_ID, true, cx)));
        let mut this = DebugContainerModal {
            app,
            item,
            namespace,
            image: DEBUG_IMAGES[0].1.to_string(),
            custom,
            share: false,
            target,
            containers: Vec::new(),
            loading: false,
            error: None,
            task: None,
            describe: None,
            _subs: subs,
        };
        let (ns, pod) = (this.namespace.clone(), this.item.name.clone());
        this.describe = Some(backend::run(
            cx,
            move |core| async move { core.describe_pod(ns, pod).await },
            |this: &mut Self, res, cx| {
                if let Ok(v) = res {
                    this.containers = regular_containers(&v);
                    let mut opts = vec![(String::new(), "Select a container...".to_string())];
                    opts.extend(this.containers.iter().map(|c| (c.clone(), c.clone())));
                    this.target.update(cx, |s, cx| s.set_options(opts, cx));
                    cx.notify();
                }
            },
        ));
        this
    }

    fn resolved(&self, cx: &App) -> String {
        resolve_image(&self.image, self.custom.read(cx).text())
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| s.set_modal(MODAL_ID, false, cx));
        cx.emit(DebugEvent::Cancelled);
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let image = self.resolved(cx);
        if image.is_empty() {
            self.error = Some("Please specify a container image".into());
            cx.notify();
            return;
        }
        self.loading = true;
        self.error = None;
        let sel = self.target.read(cx).selected.clone();
        let target = (self.share && !sel.is_empty()).then_some(sel);
        let (ns, pod) = (self.namespace.clone(), self.item.name.clone());
        let img = image.clone();
        self.task = Some(backend::run(
            cx,
            move |core| async move { core.add_debug_container(ns, pod, img, target, Some(vec!["/bin/sh".into()])).await },
            move |this: &mut Self, res, cx| {
                this.loading = false;
                match res {
                    Ok(name) => {
                        this.app.update(cx, |s, cx| s.set_modal(MODAL_ID, false, cx));
                        cx.emit(DebugEvent::Created { container: name, image });
                    }
                    Err(e) => this.error = Some(backend::format_error(&e)),
                }
                cx.notify();
            },
        ));
        cx.notify();
    }

    fn option_row(&self, id: String, selected: bool, accent: Rgba, cx: &mut Context<Self>) -> Stateful<Div> {
        let value = id.clone();
        div()
            .id(SharedString::from(format!("debug-opt-{id}")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .p(px(10.))
            .rounded(px(6.))
            .border_1()
            .cursor_pointer()
            // border-accent/50 + bg-accent/10 render as preflight gray, no tint.
            .when(selected, |d| d.border_color(theme::PREFLIGHT_BORDER))
            .when(!selected, |d| {
                d.border_color(slate::S800).hover(|s| s.border_color(slate::S700).bg(theme::hexa(0x1d2939, 0.2)))
            })
            .child(radio(selected, 14., accent))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.image = value.clone();
                cx.notify();
            }))
    }

    fn render_form(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        let resolved = self.resolved(cx);
        let can_submit = !resolved.is_empty();
        let custom_focused = self.custom.read(cx).is_focused(window);
        let mut options: Vec<AnyElement> = DEBUG_IMAGES
            .iter()
            .map(|(label, image, desc)| {
                let sel = self.image == *image;
                self.option_row(image.to_string(), sel, accent, cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            // Mixed 14px/12px inline text: the line box is ~23.5px.
                            .min_h(px(23.5))
                            .flex()
                            .flex_row()
                            .items_baseline()
                            .child(div().text_sm_tw().text_color(slate::S200).child(*label))
                            .child(div().ml(px(8.)).text_xs_tw().text_color(slate::S500).child(*desc)),
                    )
                    .child(div().max_w(px(140.)).truncate().text_10().font_mono().text_color(slate::S600).child(*image))
                    .auto_id(format!("debug.image.{label}"), "radio", *label)
                    .selected(sel)
                    .into_any_element()
            })
            .collect();
        let custom_sel = self.image == CUSTOM;
        options.push(
            self.option_row(CUSTOM.into(), custom_sel, accent, cx)
                .child(div().text_sm_tw().text_color(slate::S200).child("Custom"))
                .auto_id("debug.image.custom", "radio", "Custom")
                .selected(custom_sel)
                .into_any_element(),
        );
        if custom_sel {
            options.push(
                div()
                    .mt(px(4.))
                    .w_full()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(if custom_focused { theme::PREFLIGHT_BORDER } else { slate::S800 })
                    .bg(theme::BACKGROUND)
                    .child(self.custom.clone())
                    .auto_id("debug.custom", "textbox", "e.g. alpine:latest")
                    .into_any_element(),
            );
        }
        let pod = self.item.name.clone();
        div()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(16.))
                    .mb(px(20.))
                    .child(div().p(px(8.)).rounded_full().child(icon("bug", 24.).text_color(accent)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_lg_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("Debug Container"))
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_xs_tw()
                                    .text_color(slate::S400)
                                    .child({
                                        let prefix = "Inject an ephemeral container into ";
                                        let run = |len: usize, family: &str, color: Rgba| TextRun {
                                            len,
                                            font: font(SharedString::from(family.to_string())),
                                            color: color.into(),
                                            background_color: None,
                                            underline: None,
                                            strikethrough: None,
                                        };
                                        StyledText::new(format!("{prefix}{pod}")).with_runs(vec![
                                            run(prefix.len(), theme::SANS, slate::S400),
                                            run(pod.len(), theme::MONO, accent),
                                        ])
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("debug-close")
                            .group("debug-close")
                            .cursor_pointer()
                            .child(icon("x", 20.).text_color(slate::S400).group_hover("debug-close", |s| s.text_color(slate::S200)))
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
                            .auto_id("debug.close", "button", "Close"),
                    ),
            )
            .child(
                div()
                    .mb(px(16.))
                    .child(div().mb(px(8.)).text_xs_tw().text_color(slate::S400).child(caps("Debug Image", 12.)))
                    .child(div().flex().flex_col().gap(px(6.)).children(options)),
            )
            .child(
                div()
                    .mb(px(16.))
                    .child(
                        div()
                            .id("debug-share")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .child(checkbox(self.share, 14., accent))
                            .child(div().text_sm_tw().text_color(slate::S300).child("Share process namespace"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.share = !this.share;
                                cx.notify();
                            }))
                            .auto_id("debug.share", "checkbox", "Share process namespace")
                            .selected(self.share),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .ml(px(20.))
                            .text_10()
                            .text_color(slate::S500)
                            .child("Target a container to share its PID namespace (see processes with ps)"),
                    ),
            )
            .when(self.share && !self.containers.is_empty(), |d| {
                d.child(
                    div()
                        .mb(px(16.))
                        .child(div().mb(px(6.)).text_xs_tw().text_color(slate::S400).child(caps("Target Container", 12.)))
                        .child(self.target.clone()),
                )
            })
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .mb(px(16.))
                        .p(px(10.))
                        .bg(theme::hexa(0xef4444, 0.1))
                        .border_1()
                        .border_color(theme::hexa(0xef4444, 0.5))
                        .rounded(px(6.))
                        .text_xs_tw()
                        .text_color(red::R400)
                        .child(e.clone())
                        .auto_id("debug.error", "alert", e),
                )
            })
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("debug-cancel")
                            .px(px(16.))
                            .py(px(8.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(slate::S800)
                            .text_sm_tw()
                            .text_color(slate::S100)
                            .cursor_pointer()
                            .hover(|s| s.border_color(slate::S700).bg(theme::hexa(0x1d2939, 0.4)))
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
                            .auto_id("debug.cancel", "button", "Cancel"),
                    )
                    .child(
                        div()
                            .id("debug-create")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(16.))
                            .py(px(8.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(theme::PREFLIGHT_BORDER)
                            .text_sm_tw()
                            .text_color(accent)
                            .when(!can_submit, |d| d.opacity(0.5).cursor_not_allowed())
                            .when(can_submit, |d| d.cursor_pointer())
                            .child(icon("bug", 16.).text_color(accent))
                            .child("Create & Connect")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if can_submit {
                                    this.submit(cx);
                                }
                            }))
                            .auto_id("debug.create", "button", "Create & Connect")
                            .disabled(!can_submit),
                    ),
            )
            .into_any_element()
    }

    fn render_loading(&self, cx: &mut Context<Self>) -> AnyElement {
        let accent = theme::accent(cx);
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .py(px(40.))
            .child(
                div().p(px(12.)).rounded_full().mb(px(16.)).child(icon("bug", 32.).text_color(accent).with_animation(
                    "debug-pulse",
                    Animation::new(Duration::from_millis(2000)).repeat(),
                    |s, t| s.opacity(1. - 0.5 * (1. - (2. * t - 1.).abs())),
                )),
            )
            .child(
                div()
                    .mb(px(8.))
                    .text_lg_tw()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(slate::S100)
                    .child("Creating debug container..."),
            )
            .child(
                div()
                    .mb(px(12.))
                    .text_sm_tw()
                    .text_color(slate::S400)
                    .text_center()
                    .child("Waiting for container to start. This may take up to 30 seconds."),
            )
            .child(
                div()
                    .px(px(12.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .font_mono()
                    .text_xs_tw()
                    .text_color(Rgba { a: 0.8, ..accent })
                    .child(self.resolved(cx)),
            )
            .auto_id("debug.loading", "status", "Creating debug container...")
            .into_any_element()
    }
}

impl Render for DebugContainerModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let body = if self.loading { self.render_loading(cx) } else { self.render_form(window, cx) };
        let loading = self.loading;
        deferred(
            anchored().position_mode(AnchoredPositionMode::Window).position(point(px(0.), px(0.))).child(
                div()
                    .id("debug-backdrop")
                    .w(vp.width)
                    .h(vp.height)
                    .bg(theme::hexa(0, 0.6))
                    .p(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .occlude()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !loading {
                            this.cancel(cx);
                        }
                    }))
                    .child(
                        div()
                            .id("debug-dialog")
                            .w_full()
                            .max_w(px(448.))
                            .bg(theme::SURFACE)
                            .border_1()
                            .border_color(theme::PREFLIGHT_BORDER)
                            .rounded(px(8.))
                            .shadow(vec![
                                BoxShadow {
                                    color: theme::hexa(0, 0.1).into(),
                                    offset: point(px(0.), px(20.)),
                                    blur_radius: px(25.),
                                    spread_radius: px(-5.),
                                    inset: false,
                                },
                                BoxShadow {
                                    color: theme::hexa(0, 0.1).into(),
                                    offset: point(px(0.), px(8.)),
                                    blur_radius: px(10.),
                                    spread_radius: px(-6.),
                                    inset: false,
                                },
                            ])
                            .occlude()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .p(px(24.))
                            .child(body)
                            .auto_id("debug", "dialog", "Debug Container"),
                    ),
            ),
        )
        .with_priority(3)
    }
}

#[cfg(test)]
mod tests {
    use super::{regular_containers, resolve_image};

    #[test]
    fn image_resolution() {
        assert_eq!(resolve_image("busybox:latest", "x"), "busybox:latest");
        assert_eq!(resolve_image("custom", "  alpine:3 "), "alpine:3");
        assert_eq!(resolve_image("custom", "   "), "");
    }

    #[test]
    fn containers() {
        let v = serde_json::json!({"spec": {"initContainers": [{"name": "i"}], "containers": [{"name": "nginx"}]}});
        assert_eq!(regular_containers(&v), vec!["nginx"]);
    }
}
