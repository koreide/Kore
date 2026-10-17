//! AI side panel (spec 03 §6.6). Shown while `AppState::ai_panel` is
//! `Some(AiPanelRequest)` (resource context when opened from a details view).
//!
//! The conversation survives close and reopen. Root drops this view on close,
//! so the conversation lives in an app-global [`AiPanelSession`]. The stream
//! keeps running while the panel is closed, so closing mid-stream never leaves
//! the input disabled (quirk 18 — fixed).

use crate::automation::Automation;
use crate::keys;
use crate::state::ai_config::{self, AiConfigModel};
use crate::state::ai_stream::{self, AiUpdate};
use crate::state::app_state::AiPanelRequest;
use crate::state::AppState;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::scrollbar;
use crate::ui::text::caps;
use crate::ui::widgets::spinning_icon;
use crate::views::ai_chat::{clear_button, focus_ring, send_button};
use crate::views::ai_chat_bubble::{chat_bubble_sized, BubbleVariant, DisplayMessage};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::{AIStreamEvent, DiagnoseRequest};
use std::time::Duration;

const QUICK_ACTIONS: [(&str, &str); 3] = [
    ("Diagnose", "Diagnose this resource and identify any issues."),
    ("Why is this failing?", "Why is this resource failing? Analyze the status, events, and conditions."),
    ("Suggest fixes", "Suggest fixes for the current issues with this resource."),
];

pub const MODAL_ID: &str = "ai-panel";

/// Conversation state that outlives the panel view.
pub struct AiPanelSession {
    pub messages: Vec<DisplayMessage>,
    pub streaming: bool,
    stream: Option<Task<()>>,
}

struct GlobalSession(Entity<AiPanelSession>);
impl Global for GlobalSession {}

fn session(cx: &mut App) -> Entity<AiPanelSession> {
    if let Some(g) = cx.try_global::<GlobalSession>() {
        return g.0.clone();
    }
    let e = cx.new(|_| AiPanelSession { messages: Vec::new(), streaming: false, stream: None });
    cx.set_global(GlobalSession(e.clone()));
    e
}

impl AiPanelSession {
    fn send(&mut self, text: &str, req: &AiPanelRequest, config: &ai_config::AiConfig, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() || self.streaming || !ai_config::is_provider_configured(config) {
            return;
        }
        self.messages.push(DisplayMessage::user(text));
        self.streaming = true;
        let request = DiagnoseRequest {
            kind: req.kind.clone(),
            namespace: req.namespace.clone(),
            name: req.name.clone(),
            prompt: Some(text.to_string()),
            session_id: Some(ai_stream::session_id("ai-session")),
        };
        self.stream = Some(ai_stream::start_diagnose(cx, config, request, |this: &mut Self, up, cx| this.on_update(up, cx)));
        cx.notify();
    }

    fn on_update(&mut self, up: AiUpdate, cx: &mut Context<Self>) {
        match up {
            AiUpdate::Failed(msg) => {
                self.streaming = false;
                self.messages.push(DisplayMessage::assistant(format!("[Error: {msg}]")));
            }
            AiUpdate::Events(evs) => {
                for ev in evs {
                    match ev {
                        AIStreamEvent::Chunk { content } if !content.is_empty() => match self.messages.last_mut() {
                            Some(m) if !m.is_user() => m.content.push_str(&content),
                            _ => self.messages.push(DisplayMessage::assistant(content)),
                        },
                        AIStreamEvent::Done => self.streaming = false,
                        AIStreamEvent::Error { message } => {
                            self.streaming = false;
                            let msg = if message.is_empty() { "An unknown error occurred.".to_string() } else { message };
                            match self.messages.last_mut() {
                                Some(m) if !m.is_user() => m.content.push_str(&format!("\n\n[Error: {msg}]")),
                                _ => self.messages.push(DisplayMessage::assistant(format!("[Error: {msg}]"))),
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        // Clears messages and starts a new session id (the old listener stays).
        self.messages.clear();
        cx.notify();
    }
}

pub struct AiPanel {
    app: Entity<AppState>,
    req: AiPanelRequest,
    config: Entity<AiConfigModel>,
    session: Entity<AiPanelSession>,
    input: Entity<TextInput>,
    scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    _subs: Vec<Subscription>,
}

impl AiPanel {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let req = app.read(cx).ai_panel.clone().unwrap_or_default();
        let config = ai_config::model(cx);
        let session = session(cx);
        let input = cx.new(|cx| {
            TextInput::new(
                "Ask about this resource...",
                InputStyle { placeholder_color: slate::S600, padding: (12., 12., 8., 8.), ..Default::default() },
                window,
                cx,
            )
        });
        let subs = vec![
            cx.observe(&config, |this, _, cx| {
                this.sync_input(cx);
                cx.notify();
            }),
            cx.observe(&session, |this, _, cx| {
                this.sync_input(cx);
                this.scroll.scroll_to_bottom();
                cx.notify();
            }),
            cx.subscribe(&input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::Enter { .. } => this.submit(cx),
                InputEvent::Change(_) | InputEvent::Focus | InputEvent::Blur => cx.notify(),
                _ => {}
            }),
            cx.on_release(|this: &mut Self, cx| this.app.update(cx, |s, cx| s.set_modal(MODAL_ID, false, cx))),
            // Esc (window listener while open) closes the panel, even from its input.
            keys::on_window_key(cx, |this: &mut Self, k, _, cx| {
                if k.key == "escape" && k.plain() {
                    let a = this.app.read(cx);
                    if a.palette_open || a.shortcuts_open || a.confirm.is_some() {
                        return false;
                    }
                    this.close(cx);
                    return true;
                }
                false
            }),
        ];
        let a = app.clone();
        cx.defer(move |cx| a.update(cx, |s, cx| s.set_modal(MODAL_ID, true, cx)));
        let t = crate::backend::sleep(cx, Duration::from_millis(200));
        let input2 = input.clone();
        cx.spawn_in(window, async move |this, cx| {
            t.await;
            // A disabled <input> cannot take focus.
            let disabled = this
                .update(cx, |this: &mut Self, cx| this.session.read(cx).streaming || !this.configured(cx))
                .unwrap_or(true);
            if !disabled {
                cx.update(|window, cx| input2.update(cx, |i, cx| i.focus(window, cx))).ok();
            }
        })
        .detach();
        let mut this = AiPanel {
            app,
            req,
            config,
            session,
            input,
            scroll: ScrollHandle::new(),
            gutter: scrollbar::Gutter::default(),
            _subs: subs,
        };
        this.sync_input(cx);
        this.scroll.scroll_to_bottom();
        this
    }

    fn configured(&self, cx: &App) -> bool {
        self.config.read(cx).is_configured()
    }

    fn sync_input(&mut self, cx: &mut Context<Self>) {
        let configured = self.configured(cx);
        let streaming = self.session.read(cx).streaming;
        self.input.update(cx, |i, cx| {
            i.set_placeholder(
                if !configured {
                    "Configure an AI provider to get started..."
                } else if streaming {
                    "Waiting for response..."
                } else {
                    "Ask about this resource..."
                },
                cx,
            )
        });
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.app.update(cx, |s, cx| {
            s.set_modal(MODAL_ID, false, cx);
            s.close_ai_panel(cx);
        });
    }

    fn send(&mut self, text: String, cx: &mut Context<Self>) {
        let config = self.config.read(cx).config.clone();
        let req = self.req.clone();
        self.session.update(cx, |s, cx| s.send(&text, &req, &config, cx));
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().to_string();
        if text.trim().is_empty() || self.session.read(cx).streaming || !self.configured(cx) {
            return;
        }
        self.input.update(cx, |i, cx| i.set_text("", cx));
        self.send(text, cx);
    }
}

impl Render for AiPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let configured = self.configured(cx);
        let (messages, streaming) = {
            let s = self.session.read(cx);
            (s.messages.clone(), s.streaming)
        };
        let has_messages = !messages.is_empty();
        let disabled = streaming || !configured;
        let text = self.input.read(cx).text().trim().to_string();
        let can_send = !text.is_empty() && !streaming && configured;
        let focused = self.input.read(cx).is_focused(window) && !disabled;
        let width = px(420.).min(window.viewport_size().width * 0.9);

        let header = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(16.))
            .py(px(12.))
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.8))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(div().size(px(32.)).rounded(px(8.)).flex().items_center().justify_center().child(icon("bot", 18.).text_color(accent)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("AI Assistant"))
                            .child(div().text_10().text_color(slate::S500).child("Kubernetes troubleshooting")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        clear_button("ai-panel-clear", has_messages)
                            .when(has_messages, |d| d.on_click(cx.listener(|this, _, _, cx| this.session.update(cx, |s, cx| s.clear(cx)))))
                            .auto_id("ai-panel.clear", "button", "Clear chat")
                            .disabled(!has_messages),
                    )
                    .child(
                        div()
                            .id("ai-panel-close")
                            .group("ai-panel-close")
                            .p(px(6.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hexa(0x1d2939, 0.5)))
                            .child(icon("x", 16.).text_color(slate::S400).group_hover("ai-panel-close", |s| s.text_color(slate::S200)))
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                            .auto_id("ai-panel.close", "button", "Close AI panel"),
                    ),
            );

        let context = match (&self.req.kind, &self.req.namespace, &self.req.name) {
            (Some(k), Some(ns), Some(n)) => Some(
                div()
                    .flex_none()
                    .px(px(16.))
                    .py(px(8.))
                    .border_b_1()
                    .border_color(theme::hexa(0x1e293b, 0.5))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .text_xs_tw()
                    .child(icon("sparkles", 14.).text_color(accent))
                    .child(div().flex_none().text_color(slate::S400).child("Context:"))
                    .child(div().min_w_0().truncate().font_mono().text_color(accent).child(format!("{k}/{ns}/{n}")))
                    .auto_id("ai-panel.context", "text", format!("{k}/{ns}/{n}")),
            ),
            _ => None,
        };

        let quick = (!has_messages).then(|| {
            div()
                .flex_none()
                .px(px(16.))
                .py(px(12.))
                .border_b_1()
                .border_color(theme::hexa(0x1e293b, 0.5))
                .child(div().mb(px(8.)).text_10().text_color(slate::S500).child(caps("Quick Actions", 10.)))
                .child(div().flex().flex_row().flex_wrap().gap(px(6.)).children(QUICK_ACTIONS.iter().map(|(label, prompt)| {
                    let prompt = prompt.to_string();
                    div()
                        .id(SharedString::from(format!("ai-quick-{label}")))
                        .px(px(12.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(slate::S700)
                        .text_xs_tw()
                        .text_color(slate::S300)
                        .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
                        .when(!disabled, |d| {
                            d.cursor_pointer()
                                .hover(|s| s.text_color(accent))
                                .on_click(cx.listener(move |this, _, _, cx| this.send(prompt.clone(), cx)))
                        })
                        .child(label.to_string())
                        .auto_id(format!("ai-panel.quick.{label}"), "button", label.to_string())
                        .disabled(disabled)
                })))
        });

        let mut list = div().flex().flex_col().gap(px(16.)).px(px(16.)).py(px(12.)).min_h_full();
        if !has_messages {
            list = list.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .px(px(24.))
                    .child(div().size(px(56.)).rounded(px(16.)).mb(px(16.)).flex().items_center().justify_center().child(icon("bot", 28.).text_color(Rgba { a: 0.6, ..accent })))
                    .child(div().text_sm_tw().text_color(slate::S400).mb(px(4.)).child("Ask about your Kubernetes resources"))
                    .child(div().text_xs_tw().text_color(slate::S600).child("Diagnose issues, understand failures, and get fix suggestions.")),
            );
        }
        let row_w = f32::from(width) - 1. - 32. - if self.gutter.overflows() { scrollbar::WIDTH } else { 0. };
        for (i, m) in messages.iter().enumerate() {
            list = list.child(chat_bubble_sized(
                format!("ai-panel-msg-{i}"),
                m,
                BubbleVariant::Compact,
                !m.is_user(),
                Some(row_w),
                window,
                cx,
            ));
        }
        if streaming {
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .text_xs_tw()
                    .text_color(slate::S500)
                    .child(spinning_icon("ai-panel-think", "loader-2", 14., accent))
                    .child("Thinking..."),
            );
        }
        let gutter = self.gutter.overflows();
        let body = div()
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_row()
            .child(
                div()
                    .id("ai-panel-messages")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(list),
            )
            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone()));

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
                    .child(
                        div()
                            .id("ai-panel-input")
                            .flex_1()
                            .min_w_0()
                            .h(px(38.))
                            .rounded(px(8.))
                            .bg(theme::BACKGROUND)
                            .border_1()
                            .border_color(slate::S800)
                            .relative().when(focused, |d| d.child(focus_ring(accent).rounded(px(11.))))
                            .when(disabled, |d| d.opacity(0.5))
                            .child(self.input.clone())
                            // Disabled input: swallow clicks so it cannot take focus.
                            .when(disabled, |d| {
                                d.child(div().id("ai-panel-input-block").absolute().inset_0().occlude().cursor_not_allowed())
                            })
                            .auto_id("ai-panel.input", "textbox", "Ask about this resource"),
                    )
                    .child(
                        send_button("ai-panel-send", can_send, 8., 8., accent)
                            .when(can_send, |d| d.on_click(cx.listener(|this, _, _, cx| this.submit(cx))))
                            .auto_id("ai-panel.send", "button", "Send message")
                            .disabled(!can_send),
                    ),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_10()
                    .text_color(slate::S600)
                    .text_center()
                    .child("AI responses may be inaccurate. Always verify before acting."),
            );

        let panel = div()
            .id("ai-panel")
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .w(width)
            .occlude()
            .flex()
            .flex_col()
            .bg(theme::SURFACE)
            .border_l_1()
            .border_color(slate::S800)
            .shadow(vec![BoxShadow {
                color: theme::hexa(0, 0.25).into(),
                offset: point(px(0.), px(25.)),
                blur_radius: px(50.),
                spread_radius: px(-12.),
                inset: false,
            }])
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(header)
            .when(!configured, |d| d.child(crate::views::banners::ai_config_warning()))
            .children(context)
            .children(quick)
            .child(body)
            .child(form)
            .with_animation("ai-panel-slide", Animation::new(Duration::from_millis(250)).with_easing(ease_out_quint()), move |d, t| {
                d.right(-width * (1. - t))
            });

        deferred(
        div()
            .id("ai-panel-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme::hexa(0, 0.3))
            .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
            .child(panel)
            .auto_id("ai-panel", "dialog", "AI Assistant"),
        )
        .with_priority(1)
    }
}
