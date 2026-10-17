//! AI Chat view (`view = "chat"`, the default landing view; spec 03 §6.7).
//! Tool-calling cluster chat over `ai_chat`, markdown answers, tool status
//! pill, Enter to send / Shift+Enter for a newline.

use crate::automation::Automation;
use crate::state::ai_config::{self, AiConfig, AiConfigModel};
use crate::state::ai_stream::{self, AiUpdate};
use crate::state::AppState;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::InputEvent;
use crate::ui::scrollbar;
use crate::ui::textarea::TextArea;
use crate::ui::widgets::spinning_icon;
use crate::views::ai_chat_bubble::{chat_bubble_sized, BubbleVariant, DisplayMessage};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kore_core::{AIChatRequest, AIStreamEvent, ChatMessage};
use std::time::Duration;

const SUGGESTIONS: [(&str, &str, &str); 5] = [
    ("activity", "Cluster health", "Give me an overview of the cluster health. Are there any issues?"),
    ("alert-triangle", "Failing pods", "What pods are currently failing or in an error state? Why are they failing?"),
    ("arrow-up-circle", "High restarts", "Which pods have the most restarts? What's causing them?"),
    ("bell", "Recent warnings", "Show me recent warning events in the cluster and explain what they mean."),
    ("layout-list", "Resource overview", "Give me an overview of all deployments and their status."),
];

pub struct AiChatView {
    app: Entity<AppState>,
    config: Entity<AiConfigModel>,
    input: Entity<TextArea>,
    messages: Vec<DisplayMessage>,
    streaming: bool,
    tool_status: Option<String>,
    stream: Option<Task<()>>,
    scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    _subs: Vec<Subscription>,
}

impl AiChatView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = ai_config::model(cx);
        let input = cx.new(|cx| {
            let mut t = TextArea::new("Ask about your cluster...", window, cx);
            t.placeholder_color = slate::S600;
            t.max_height = 120. - 22.;
            t
        });
        let subs = vec![
            cx.observe(&config, |this, _, cx| {
                this.sync_input(cx);
                cx.notify();
            }),
            cx.subscribe(&input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::Enter { shift: false } => {
                    let text = this.input.read(cx).text().to_string();
                    this.send(text, cx);
                }
                InputEvent::Change(_) | InputEvent::Focus | InputEvent::Blur => cx.notify(),
                _ => {}
            }),
        ];
        // Focus the textarea 200ms after mount, unless the view is gone by then
        // (the user navigated away) or the user already focused a text field.
        let t = crate::backend::sleep(cx, Duration::from_millis(200));
        cx.spawn_in(window, async move |this, cx| {
            t.await;
            this.update_in(cx, |this: &mut Self, window, cx| {
                // A disabled <textarea> cannot take focus.
                if !this.input.read(cx).disabled && !crate::keys::text_entry_focused(window) {
                    this.input.update(cx, |i, cx| i.focus(window, cx))
                }
            })
            .ok();
        })
        .detach();
        let mut this = AiChatView {
            app,
            config,
            input,
            messages: Vec::new(),
            streaming: false,
            tool_status: None,
            stream: None,
            scroll: ScrollHandle::new(),
            gutter: scrollbar::Gutter::default(),
            _subs: subs,
        };
        this.sync_input(cx);
        this
    }

    fn configured(&self, cx: &App) -> bool {
        self.config.read(cx).is_configured()
    }

    /// Placeholder + disabled state of the textarea.
    fn sync_input(&mut self, cx: &mut Context<Self>) {
        let configured = self.configured(cx);
        let streaming = self.streaming;
        self.input.update(cx, |i, cx| {
            i.disabled = streaming || !configured;
            i.set_placeholder(
                if !configured {
                    "Configure an AI provider to get started..."
                } else if streaming {
                    "Waiting for response..."
                } else {
                    "Ask about your cluster..."
                },
                cx,
            );
        });
    }

    fn send(&mut self, text: String, cx: &mut Context<Self>) {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() || self.streaming || !self.configured(cx) {
            return;
        }
        self.messages.push(DisplayMessage::user(trimmed));
        self.input.update(cx, |i, cx| i.set_text("", cx));
        self.streaming = true;
        self.tool_status = None;
        self.sync_input(cx);
        let config: AiConfig = self.config.read(cx).config.clone();
        let ns = self.app.read(cx).namespace.clone();
        let request = AIChatRequest {
            messages: self.messages.iter().map(|m| ChatMessage { role: m.role.clone(), content: m.content.clone() }).collect(),
            session_id: ai_stream::session_id("chat"),
            namespace: (ns != "*").then_some(ns),
        };
        // Replaces (and so cancels) the previous session's listener.
        self.stream = Some(ai_stream::start_chat(cx, &config, request, |this: &mut Self, up, cx| this.on_update(up, cx)));
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn on_update(&mut self, up: AiUpdate, cx: &mut Context<Self>) {
        match up {
            AiUpdate::Failed(msg) => {
                self.streaming = false;
                self.tool_status = None;
                self.messages.push(DisplayMessage::assistant(format!("**Error:** {msg}")));
            }
            AiUpdate::Events(evs) => {
                for ev in evs {
                    match ev {
                        AIStreamEvent::Chunk { content } if !content.is_empty() => {
                            self.tool_status = None;
                            match self.messages.last_mut() {
                                Some(m) if !m.is_user() => m.content.push_str(&content),
                                _ => self.messages.push(DisplayMessage::assistant(content)),
                            }
                        }
                        AIStreamEvent::Chunk { .. } => {}
                        AIStreamEvent::Status { message } if !message.is_empty() => self.tool_status = Some(message),
                        AIStreamEvent::Status { .. } => {}
                        AIStreamEvent::Done => {
                            self.streaming = false;
                            self.tool_status = None;
                        }
                        AIStreamEvent::Error { message } => {
                            self.streaming = false;
                            self.tool_status = None;
                            let msg = if message.is_empty() { "An unknown error occurred.".to_string() } else { message };
                            match self.messages.last_mut() {
                                Some(m) if !m.is_user() => m.content.push_str(&format!("\n\n**Error:** {msg}")),
                                _ => self.messages.push(DisplayMessage::assistant(format!("**Error:** {msg}"))),
                            }
                        }
                    }
                }
            }
        }
        self.sync_input(cx);
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        // Empties messages + tool status; the listener stays.
        self.messages.clear();
        self.tool_status = None;
        cx.notify();
    }

    fn render_empty(&self, configured: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.streaming || !configured;
        let accent = theme::accent(cx);
        let grid_w = 512.;
        let cell_w = ((grid_w - 16.) / 3.) - 0.05;
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .pt(px(64.))
            .pb(px(32.))
            .child(
                div()
                    .size(px(64.))
                    .rounded(px(16.))
                    .mb(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("bot", 32.).text_color(Rgba { a: 0.6, ..accent })),
            )
            .child(
                div()
                    .text_lg_tw()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(slate::S100)
                    .mb(px(6.))
                    .child("What can I help you with?"),
            )
            .child(
                div()
                    .text_sm_tw()
                    .text_color(slate::S500)
                    .mb(px(32.))
                    .max_w(px(448.))
                    .text_center()
                    .child("Ask anything about your cluster. I'll query live data to give you accurate answers."),
            )
            .child(
                div().w_full().max_w(px(grid_w)).flex().flex_row().flex_wrap().gap(px(8.)).children(SUGGESTIONS.iter().map(
                    |(ic, label, prompt)| {
                        let g: SharedString = format!("sugg-{label}").into();
                        let prompt = prompt.to_string();
                        div()
                            .id(g.clone())
                            .group(g.clone())
                            .w(px(cell_w))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(slate::S800)
                            .bg(theme::hexa(0x101828, 0.6))
                            .text_xs_tw()
                            .text_color(slate::S300)
                            .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
                            .when(!disabled, |d| {
                                d.cursor_pointer()
                                    .hover(|s| s.text_color(accent))
                                    .on_click(cx.listener(move |this, _, _, cx| this.send(prompt.clone(), cx)))
                            })
                            .child(
                                icon(ic, 14.)
                                    .text_color(slate::S300)
                                    .when(!disabled, |i| i.group_hover(g.clone(), |s| s.text_color(accent))),
                            )
                            .child(label.to_string())
                            .auto_id(format!("chat.suggestion.{label}"), "button", label.to_string())
                            .disabled(disabled)
                    },
                )),
            )
    }
}

/// Trash "Clear chat" button (AI Chat header and AI Panel).
pub fn clear_button(id: &'static str, enabled: bool) -> Stateful<Div> {
    let g: SharedString = format!("{id}-g").into();
    div()
        .id(id)
        .group(g.clone())
        .p(px(6.))
        .rounded(px(6.))
        .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(theme::hexa(0x1d2939, 0.5))))
        .when(!enabled, |d| d.cursor_not_allowed())
        .child(
            icon("trash-2", 16.)
                .text_color(if enabled { slate::S400 } else { slate::S700 })
                .when(enabled, |i| i.group_hover(g, |s| s.text_color(theme::red::R400))),
        )
}

/// Send button (panel: padding 8 / radius 8; chat: padding 10 / radius 12).
pub fn send_button(id: &'static str, active: bool, pad: f32, radius: f32, accent: Rgba) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .p(px(pad))
        .rounded(px(radius))
        .border_1()
        .map(|d| {
            if active {
                d.border_color(theme::PREFLIGHT_BORDER).cursor_pointer().child(icon("send", 16.).text_color(accent))
            } else {
                d.border_color(slate::S800)
                    .bg(theme::hexa(0x1d2939, 0.3))
                    .cursor_not_allowed()
                    .child(icon("send", 16.).text_color(slate::S600))
            }
        })
}

/// AIConfigWarning with the AI Chat's `px-6` override (spec 01 §13.4).
pub fn config_warning(pad_x: f32) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(pad_x))
        .py(px(10.))
        .border_b_1()
        .border_color(theme::hexa(0x1e293b, 0.5))
        .bg(theme::hexa(0xf59e0b, 0.05))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .text_xs_tw()
        .child(icon("alert-circle", 14.).text_color(theme::amber::A400))
        .child(div().text_color(theme::hexa(0xfcd34d, 0.9)).child("Configure an AI provider in Settings to use this feature"))
        .auto_id("banner.ai-config", "alert", "Configure an AI provider")
}

/// 2px accent@40% focus ring (`*:focus-visible` box-shadow), drawn as an
/// overlay border: GPUI shadows fill under the element, which would tint a
/// transparent input. Add as a child of a `.relative()` box.
pub fn focus_ring(accent: Rgba) -> Div {
    div()
        .absolute()
        .top(px(-3.))
        .left(px(-3.))
        .right(px(-3.))
        .bottom(px(-3.))
        .rounded(px(6.))
        .border_2()
        .border_color(Rgba { a: 0.4, ..accent })
}

impl Render for AiChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let configured = self.configured(cx);
        let has_messages = !self.messages.is_empty();
        let input_text = self.input.read(cx).text().trim().to_string();
        let can_send = !input_text.is_empty() && !self.streaming && configured;
        let input_disabled = self.streaming || !configured;
        let focused = self.input.read(cx).is_focused(window) && !input_disabled;

        let header = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(24.))
            .py(px(12.))
            .border_b_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .child(div().size(px(32.)).rounded(px(8.)).flex().items_center().justify_center().child(icon("bot", 18.).text_color(accent)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("Kore AI"))
                            .child(div().text_10().text_color(slate::S500).child("Cluster assistant with live data")),
                    ),
            )
            .child(
                clear_button("chat-clear", has_messages)
                    .when(has_messages, |d| d.on_click(cx.listener(|this, _, _, cx| this.clear(cx))))
                    .auto_id("chat.clear", "button", "Clear chat")
                    .disabled(!has_messages),
            );

        // Messages
        // Row width for shrink-to-fit bubbles: window - sidebar (256) - main
        // padding (32) - scrollbar gutter, capped at the 768px column, - px-6.
        let area = f32::from(window.viewport_size().width) - 256. - 32. - if self.gutter.overflows() { scrollbar::WIDTH } else { 0. };
        let row_w = area.min(768.) - 48.;
        let mut list = div().flex().flex_col().gap(px(20.));
        for (i, m) in self.messages.iter().enumerate() {
            list = list.child(chat_bubble_sized(
                format!("chat-msg-{i}"),
                m,
                BubbleVariant::Comfortable,
                !m.is_user(),
                Some(row_w),
                window,
                cx,
            ));
        }
        if let Some(status) = &self.tool_status {
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme::PREFLIGHT_BORDER)
                    .text_xs_tw()
                    .text_color(accent)
                    .child(icon("wrench", 14.).text_color(accent))
                    .child(spinning_icon("chat-tool-spin", "loader-2", 12., accent))
                    .child(status.clone())
                    .auto_id("chat.tool-status", "status", status.clone()),
            );
        }
        // While streaming without a tool status: "Thinking..." until the first
        // chunk (there used to be no indicator at all), then "Generating...".
        if self.streaming && self.tool_status.is_none() {
            let label = if self.messages.last().is_some_and(|m| !m.is_user()) { "Generating..." } else { "Thinking..." };
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(40.))
                    .text_xs_tw()
                    .text_color(slate::S500)
                    .child(spinning_icon("chat-gen-spin", "loader-2", 14., accent))
                    .child(label)
                    .auto_id("chat.progress", "status", label),
            );
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
                    .id("chat-messages")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(768.))
                                    .px(px(24.))
                                    .py(px(24.))
                                    .when(!has_messages && !self.streaming, |d| d.child(self.render_empty(configured, cx)))
                                    .child(list),
                            ),
                    ),
            )
            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone()));

        let input_box = div()
            .id("chat-input-box")
            .flex_1()
            .min_w_0()
            .px(px(16.))
            .py(px(10.))
            .rounded(px(12.))
            .bg(theme::BACKGROUND)
            .border_1()
            .border_color(slate::S800)
            .relative().when(focused, |d| d.child(focus_ring(accent).rounded(px(15.))))
            .when(input_disabled, |d| d.opacity(0.5))
            .on_click(cx.listener(|this, _, window, cx| {
                if !this.input.read(cx).disabled {
                    this.input.update(cx, |i, cx| i.focus(window, cx))
                }
            }))
            .child(self.input.clone());

        let input_area = div()
            .flex_none()
            .border_t_1()
            .border_color(slate::S800)
            .bg(theme::hexa(0x101828, 0.5))
            .px(px(24.))
            .py(px(16.))
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(768.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_end()
                            .gap(px(12.))
                            .child(input_box.auto_id("chat.input", "textbox", "Ask about your cluster"))
                            .child(
                                send_button("chat-send", can_send, 10., 12., accent)
                                    .when(can_send, |d| {
                                        d.on_click(cx.listener(|this, _, _, cx| {
                                            let t = this.input.read(cx).text().to_string();
                                            this.send(t, cx)
                                        }))
                                    })
                                    .auto_id("chat.send", "button", "Send message")
                                    .disabled(!can_send),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_10()
                            .text_color(slate::S600)
                            .text_center()
                            .child("Enter to send, Shift+Enter for newline. AI queries live cluster data."),
                    ),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .when(!configured, |d| d.child(config_warning(24.)))
            .child(messages)
            .child(input_area)
            .auto_id("view.chat", "region", "AI Chat")
    }
}
