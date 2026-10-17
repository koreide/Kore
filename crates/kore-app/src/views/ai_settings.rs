//! AI Provider card content of the Settings page (`AISettings`, spec 01 §17.1 /
//! spec 03 §6.2). Edits go straight to the app-global [`AiConfigModel`].
//!
//! Deliberate fixes: Test Connection reports HTTP errors (Fix 12, via
//! `ai_test_connection_checked`). The Ollama availability probe uses the
//! configured base URL (Fix 13).

use crate::automation::Automation;
use crate::backend;
use crate::state::ai_config::{self, AiConfig, AiConfigModel, PROVIDERS};
use crate::state::AppState;
use crate::theme::{self, emerald, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::native_controls::{NativeSelect, SelectEvent, SelectStyle};
use crate::ui::toast::{toast, ToastKind};
use crate::ui::widgets::spinning_icon;
use crate::views::ai_chat::focus_ring;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
enum TestResult {
    Success,
    Error,
}

pub struct AiSettings {
    app: Entity<AppState>,
    config: Entity<AiConfigModel>,
    availability: HashMap<String, bool>,
    testing: bool,
    test_result: Option<TestResult>,
    models: Vec<String>,
    loading_models: bool,
    key_input: Entity<TextInput>,
    model_input: Entity<TextInput>,
    base_input: Entity<TextInput>,
    model_select: Entity<NativeSelect>,
    /// Provider + base URL the model list was fetched for.
    models_for: Option<(String, Option<String>)>,
    tasks: Vec<Task<()>>,
    models_task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

fn input_style(mono: bool) -> InputStyle {
    InputStyle {
        placeholder_color: slate::S600,
        padding: (12., 12., 8., 8.),
        font_family: if mono { theme::MONO.into() } else { theme::SANS.into() },
        ..Default::default()
    }
}

impl AiSettings {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = ai_config::model(cx);
        let c = config.read(cx).config.clone();
        let provider = c.provider().copied().unwrap_or(PROVIDERS[0]);
        let key_input = cx.new(|cx| {
            let mut t = TextInput::new(format!("Enter your {} API key", provider.label), InputStyle { masked: true, ..input_style(false) }, window, cx);
            t.set_text(c.api_key.clone().unwrap_or_default(), cx);
            t
        });
        let model_input = cx.new(|cx| {
            let mut t = TextInput::new(provider.default_model, input_style(true), window, cx);
            t.set_text(c.model.clone(), cx);
            t
        });
        let base_input = cx.new(|cx| {
            let mut t = TextInput::new(provider.default_base_url.unwrap_or("http://localhost:11434"), input_style(true), window, cx);
            t.set_text(c.base_url.clone().unwrap_or_default(), cx);
            t
        });
        let weak = app.downgrade();
        let model_select = cx.new(|cx| {
            let mut s = NativeSelect::new(
                "settings.ai.model",
                Vec::new(),
                c.model.clone(),
                SelectStyle {
                    padding: (12., 8.),
                    radius: 8.,
                    font_size: 14.,
                    line_height: 20.,
                    text_color: slate::S100,
                    bg: theme::hexa(0x101828, 0.6),
                    border: slate::S800,
                    full_width: true,
                },
                weak,
                cx,
            );
            s.show_arrow = false;
            s
        });
        let subs = vec![
            cx.observe(&config, |this, _, cx| this.on_config_changed(cx)),
            cx.subscribe(&key_input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::Change(v) => this.on_key_changed(v.clone(), cx),
                InputEvent::Focus | InputEvent::Blur => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&model_input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::Change(v) => {
                    let v = v.clone();
                    this.edit(cx, move |c| c.model = v);
                }
                InputEvent::Focus | InputEvent::Blur => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&base_input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::Change(v) => {
                    let v = v.clone();
                    this.edit(cx, move |c| c.base_url = (!v.is_empty()).then_some(v));
                }
                InputEvent::Focus | InputEvent::Blur => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&model_select, |this, _, ev: &SelectEvent, cx| {
                let SelectEvent::Change(v) = ev;
                let v = v.clone();
                this.edit(cx, move |c| c.model = v);
            }),
            cx.observe(&model_select, |_, _, cx| cx.notify()),
        ];
        let mut this = AiSettings {
            app,
            config,
            availability: HashMap::new(),
            testing: false,
            test_result: None,
            models: Vec::new(),
            loading_models: false,
            key_input,
            model_input,
            base_input,
            model_select,
            models_for: None,
            tasks: Vec::new(),
            models_task: None,
            _subs: subs,
        };
        // Availability check on mount (Fix 13: Ollama probe uses the configured URL).
        let base = c.base_url.clone();
        this.tasks.push(backend::run(cx, move |core| async move { core.check_providers_availability(base).await }, |this: &mut Self, res, cx| {
            if let Ok(m) = res {
                this.availability = m;
                cx.notify();
            }
        }));
        this.fetch_models(cx);
        this
    }

    fn cfg(&self, cx: &App) -> AiConfig {
        self.config.read(cx).config.clone()
    }

    /// Any field edit: update the shared config and reset the test result.
    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut AiConfig)) {
        let mut c = self.cfg(cx);
        f(&mut c);
        self.test_result = None;
        self.config.update(cx, |m, cx| m.set(c, cx));
        cx.notify();
    }

    fn on_key_changed(&mut self, v: String, cx: &mut Context<Self>) {
        let provider = self.cfg(cx).provider;
        let key = (!v.is_empty()).then_some(v.clone());
        self.edit(cx, move |c| c.api_key = key);
        // Persist to the OS keychain (store_api_key / delete_api_key).
        let p = provider.clone();
        let t = if v.is_empty() {
            backend::run(cx, move |core| async move { core.delete_api_key(p).await }, |_: &mut Self, _, _| {})
        } else {
            backend::run(cx, move |core| async move { core.store_api_key(p, v).await }, |_: &mut Self, _, _| {})
        };
        self.tasks.push(t);
    }

    /// Keep inputs in sync with the shared config; refetch models on
    /// provider / base URL change.
    fn on_config_changed(&mut self, cx: &mut Context<Self>) {
        let c = self.cfg(cx);
        let key = c.api_key.clone().unwrap_or_default();
        if self.key_input.read(cx).text() != key {
            self.key_input.update(cx, |i, cx| i.set_text(key, cx));
        }
        if self.model_input.read(cx).text() != c.model {
            let m = c.model.clone();
            self.model_input.update(cx, |i, cx| i.set_text(m, cx));
        }
        let base = c.base_url.clone().unwrap_or_default();
        if self.base_input.read(cx).text() != base {
            self.base_input.update(cx, |i, cx| i.set_text(base, cx));
        }
        if self.model_select.read(cx).selected != c.model {
            let m = c.model.clone();
            self.model_select.update(cx, |s, cx| s.set_selected(m, cx));
        }
        self.fetch_models(cx);
        cx.notify();
    }

    fn fetch_models(&mut self, cx: &mut Context<Self>) {
        let c = self.cfg(cx);
        let key = (c.provider.clone(), if c.provider == "ollama" { c.base_url.clone() } else { None });
        if self.models_for.as_ref() == Some(&key) {
            return;
        }
        self.models_for = Some(key);
        self.models.clear();
        if !matches!(c.provider.as_str(), "ollama" | "claude_cli" | "cursor_agent") {
            self.loading_models = false;
            self.models_task = None;
            return;
        }
        self.loading_models = true;
        let provider = c.provider.clone();
        let base = c.base_url.clone();
        self.models_task = Some(backend::run(
            cx,
            move |core| async move {
                match provider.as_str() {
                    "ollama" => core.list_ollama_models(base).await,
                    "claude_cli" => core.list_claude_models().await,
                    _ => core.list_cursor_agent_models().await,
                }
            },
            |this: &mut Self, res, cx| {
                this.loading_models = false;
                this.models = res.unwrap_or_default();
                let opts: Vec<(String, String)> = this.models.iter().map(|m| (m.clone(), m.clone())).collect();
                this.model_select.update(cx, |s, cx| s.set_options(opts, cx));
                let cur = this.cfg(cx).model;
                if let Some(first) = this.models.first().cloned() {
                    if !this.models.contains(&cur) {
                        this.config.update(cx, |m, cx| {
                            let mut c = m.config.clone();
                            c.model = first;
                            m.set(c, cx)
                        });
                    }
                }
                cx.notify();
            },
        ));
    }

    fn select_provider(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(p) = ai_config::provider_option(id) else { return };
        let cur = self.cfg(cx);
        let same = cur.provider == id;
        let c = AiConfig {
            provider: id.to_string(),
            model: p.default_model.to_string(),
            // Ollama's default comes from Core (`CoreConfig::ai_endpoints`), so a
            // sandboxed Core (tests) never points the app at a real local Ollama.
            base_url: p.default_base_url.map(|d| if id == "ollama" { crate::backend::core(cx).ollama_default_url() } else { d.to_string() }),
            api_key: if same { cur.api_key.clone() } else { None },
        };
        self.test_result = None;
        let label = p.label;
        self.key_input.update(cx, |i, cx| i.set_placeholder(format!("Enter your {label} API key"), cx));
        self.model_input.update(cx, |i, cx| i.set_placeholder(p.default_model, cx));
        self.config.update(cx, |m, cx| m.set(c, cx));
        cx.notify();
    }

    fn test_connection(&mut self, cx: &mut Context<Self>) {
        self.testing = true;
        self.test_result = None;
        let config = self.cfg(cx).to_core();
        self.tasks.push(backend::run(cx, move |core| async move { core.ai_test_connection_checked(config).await }, |this: &mut Self, res, cx| {
            this.testing = false;
            match res {
                Ok(()) => {
                    this.test_result = Some(TestResult::Success);
                    toast(cx, "Connection successful", ToastKind::Success);
                }
                Err(e) => {
                    this.test_result = Some(TestResult::Error);
                    toast(cx, format!("Connection failed: {}", backend::format_error(&e)), ToastKind::Error);
                }
            }
            cx.notify();
        }));
        cx.notify();
    }

    fn field(&self, label: &str, child: AnyElement) -> Div {
        div()
            .child(div().text_xs_tw().text_color(slate::S400).mb(px(6.)).child(label.to_string()))
            .child(child)
    }
}

/// Bordered single-line text field box (38px: py 8 + 20 + border).
pub fn text_box(id: impl Into<SharedString>, input: Entity<TextInput>, focused: bool, accent: Rgba, bg: Rgba, radius: f32) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .w_full()
        .h(px(38.))
        .bg(bg)
        .border_1()
        .border_color(slate::S800)
        // The field keeps its radius when focused; the ring (inset −3px) follows it.
        .rounded(px(radius))
        .relative().when(focused, |d| d.child(focus_ring(accent).rounded(px(radius + 3.))))
        .child(input)
}

impl Render for AiSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.app;
        let accent = theme::accent(cx);
        let c = self.cfg(cx);
        let current = c.provider().copied().unwrap_or(PROVIDERS[0]);
        let n = PROVIDERS.len();

        let seg = div()
            .flex()
            .flex_row()
            .rounded(px(8.))
            .border_1()
            .border_color(slate::S800)
            .overflow_hidden()
            .children(PROVIDERS.iter().enumerate().map(|(i, p)| {
                let selected = c.provider == p.id;
                let unavailable = self.availability.get(p.id).is_some_and(|a| !*a);
                let id = p.id;
                let g: SharedString = format!("ai-prov-{id}").into();
                div()
                    .id(g.clone())
                    .flex_1()
                    .px(px(12.))
                    .py(px(8.))
                    .text_xs_tw()
                    .font_weight(FontWeight::MEDIUM)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .when(i < n - 1, |d| d.border_r_1().border_color(slate::S800))
                    .map(|d| {
                        if selected {
                            d.text_color(accent)
                        } else if unavailable {
                            d.bg(theme::hexa(0x101828, 0.6)).text_color(slate::S600).opacity(0.4).cursor_not_allowed()
                        } else {
                            d.bg(theme::hexa(0x101828, 0.6))
                                .text_color(slate::S400)
                                .cursor_pointer()
                                .hover(|s| s.text_color(slate::S200).bg(theme::hexa(0x1d2939, 0.4)))
                        }
                    })
                    .when(!unavailable, |d| d.on_click(cx.listener(move |this, _, _, cx| this.select_provider(id, cx))))
                    .child(p.label)
                    .when(unavailable, |d| d.child(div().text_9().line_height(px(11.25)).mt(px(2.)).text_color(slate::S600).child("not found")))
                    .auto_id(format!("settings.ai.provider.{id}"), "radio", p.label)
                    .selected(selected)
                    .disabled(unavailable)
            }));

        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("settings-2", 16.).text_color(accent))
                    .child(div().text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("AI Provider")),
            )
            .child(seg);

        if current.needs_api_key {
            let f = self.key_input.read(cx).is_focused(window);
            col = col.child(self.field(
                "API Key",
                text_box("ai-key", self.key_input.clone(), f, accent, theme::hexa(0x101828, 0.6), 8.)
                    .auto_id("settings.ai.api-key", "textbox", "API Key")
                    .into_any_element(),
            ));
        }

        let model_el: AnyElement = if matches!(c.provider.as_str(), "ollama" | "claude_cli" | "cursor_agent") && !self.models.is_empty() {
            div().font_mono().child(self.model_select.clone()).into_any_element()
        } else if matches!(c.provider.as_str(), "ollama" | "claude_cli" | "cursor_agent") && self.loading_models {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(8.))
                .bg(theme::hexa(0x101828, 0.6))
                .border_1()
                .border_color(slate::S800)
                .rounded(px(8.))
                .text_sm_tw()
                .text_color(slate::S500)
                .child(spinning_icon("ai-models-loading", "loader-2", 14., slate::S500))
                .child("Loading models...")
                .into_any_element()
        } else {
            let f = self.model_input.read(cx).is_focused(window);
            text_box("ai-model", self.model_input.clone(), f, accent, theme::hexa(0x101828, 0.6), 8.)
                .auto_id("settings.ai.model-input", "textbox", "Model")
                .into_any_element()
        };
        col = col.child(self.field("Model", model_el));

        if current.needs_base_url {
            let f = self.base_input.read(cx).is_focused(window);
            col = col.child(self.field(
                "Base URL",
                text_box("ai-base", self.base_input.clone(), f, accent, theme::hexa(0x101828, 0.6), 8.)
                    .auto_id("settings.ai.base-url", "textbox", "Base URL")
                    .into_any_element(),
            ));
        }

        let disabled = self.testing || (current.needs_api_key && c.api_key.as_deref().unwrap_or("").is_empty());
        let (content, color): (AnyElement, Rgba) = if self.testing {
            (
                div().flex().flex_row().items_center().gap(px(8.)).child(spinning_icon("ai-testing", "loader-2", 16., accent)).child("Testing...").into_any_element(),
                accent,
            )
        } else {
            match self.test_result {
                Some(TestResult::Success) => (
                    div().flex().flex_row().items_center().gap(px(8.)).child(icon("check-circle-2", 16.).text_color(emerald::E400)).child("Connected").into_any_element(),
                    emerald::E400,
                ),
                Some(TestResult::Error) => (
                    div().flex().flex_row().items_center().gap(px(8.)).child(icon("alert-circle", 16.).text_color(red::R400)).child("Failed - Retry").into_any_element(),
                    red::R400,
                ),
                None => (div().child("Test Connection").into_any_element(), accent),
            }
        };
        let label = match (self.testing, self.test_result) {
            (true, _) => "Testing...",
            (_, Some(TestResult::Success)) => "Connected",
            (_, Some(TestResult::Error)) => "Failed - Retry",
            _ => "Test Connection",
        };
        let test = div()
            .id("ai-test-connection")
            .w_full()
            .px(px(16.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .text_sm_tw()
            .font_weight(FontWeight::MEDIUM)
            .flex()
            .items_center()
            .justify_center()
            .text_color(color)
            .map(|d| match (self.testing, self.test_result) {
                (false, Some(TestResult::Success)) => d.bg(theme::hexa(0x10b981, 0.1)).border_color(theme::hexa(0x10b981, 0.5)),
                (false, Some(TestResult::Error)) => d.bg(theme::hexa(0xef4444, 0.1)).border_color(theme::hexa(0xef4444, 0.5)),
                _ => d.border_color(theme::PREFLIGHT_BORDER),
            })
            .when(disabled, |d| d.opacity(0.4).cursor_not_allowed())
            .when(!disabled, |d| d.cursor_pointer().on_click(cx.listener(|this, _, _, cx| this.test_connection(cx))))
            .child(content)
            .auto_id("settings.ai.test", "button", label)
            .disabled(disabled);
        col.child(test)
    }
}
