//! AI Settings, AI Chat and the AI Panel against `kore_testkit::MockAi`.

use super::support::{boot_with_mock_ai, seed_ai_config, KoreExt};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use kore_testkit::{fixtures, mock_ai::MARKDOWN_ANSWER};

const PROVIDERS: [(&str, &str); 5] = [
    ("openai", "OpenAI"),
    ("anthropic", "Anthropic"),
    ("ollama", "Ollama"),
    ("claude_cli", "Claude Code"),
    ("cursor_agent", "Cursor Agent"),
];

fn open_settings(k: &mut Kore) {
    k.click("sidebar.settings");
    k.wait_until("settings view", |k| k.state()["view"] == "settings");
    k.wait_for_element("settings.ai.provider.openai");
}

fn open_chat(k: &mut Kore) {
    k.click("sidebar.view.chat");
    k.wait_until("chat view", |k| k.state()["view"] == "chat");
    k.wait_for_element("view.chat");
}

/// Text of the chat bubble `<prefix>-<i>` (e.g. `chat-msg-1`).
fn wait_msg(k: &mut Kore, id: &str, what: &str, pred: impl FnMut(&str) -> bool) -> String {
    k.wait_label(&format!("{id}.text"), what, pred)
}

/// Scenario 30-cluster-ai-settings: the AI Provider section lists the five
/// providers; Ollama shows Model + Base URL, loads the mock's model list and
/// Test Connection reports "Connected" with a success toast.
#[gpui_kit::test]
fn ai_settings_ollama_mock_connects(cx: &mut TestAppContext) {
    let (mut k, mock) = boot_with_mock_ai(cx);
    open_settings(&mut k);
    for (id, label) in PROVIDERS {
        assert_eq!(k.label(&format!("settings.ai.provider.{id}")).as_deref(), Some(label));
    }
    assert!(k.is_selected("settings.ai.provider.ollama"));
    // The availability probe goes to the configured base URL (the mock): Ollama is available.
    k.wait_until("ollama available", |k| k.element("settings.ai.provider.ollama").is_some_and(|e| !e.disabled));
    k.wait_for_element("settings.ai.base-url");
    assert!(k.element("settings.ai.api-key").is_none(), "Ollama needs no API key");
    // Model list loaded from the mock (`GET /api/tags`): the select shows mock-model.
    k.wait_label("settings.ai.model", "is mock-model", |l| l == "mock-model");
    assert!(!k.is_disabled("settings.ai.test"));
    k.click("settings.ai.test");
    k.wait_toast("Connection successful", |t| t == "Connection successful");
    k.wait_label("settings.ai.test", "Connected", |l| l == "Connected");
    let cfg = k.cx.update(|_, cx| crate::state::ai_config::current(cx));
    assert_eq!(cfg.base_url.as_deref(), Some(mock.url.as_str()));
}

/// Scenario 30-cluster-ai-settings (second half): selecting OpenAI swaps Base
/// URL for the API Key field, disables Test Connection without a key, and AI
/// Chat then shows the "Configure an AI provider" warning with a disabled input.
#[gpui_kit::test]
fn ai_settings_openai_without_key_disables_chat(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_settings(&mut k);
    k.wait_for_element("settings.ai.base-url");
    k.click("settings.ai.provider.openai");
    k.wait_for_element("settings.ai.api-key");
    assert!(k.is_selected("settings.ai.provider.openai"));
    assert!(k.element("settings.ai.base-url").is_none(), "no Base URL for OpenAI");
    let cfg = k.cx.update(|_, cx| crate::state::ai_config::current(cx));
    assert_eq!(cfg.provider, "openai");
    assert_eq!(cfg.base_url, None);
    assert!(k.is_disabled("settings.ai.test"), "Test Connection needs a key");
    assert_eq!(k.label("settings.ai.test").as_deref(), Some("Test Connection"));

    open_chat(&mut k);
    k.wait_label("banner.ai-config", "config warning", |l| l == "Configure an AI provider");
    assert!(k.is_disabled("chat.send"));
    assert!(k.is_disabled("chat.suggestion.Cluster health"));
}

/// Scenario 30-cluster-ai-test-connection-fix (Deliberate Fix 12): with the
/// Base URL on the mock's always-503 endpoint, Test Connection shows
/// "Failed - Retry" and a "Connection failed" toast, never "Connected".
#[gpui_kit::test]
fn test_connection_reports_http_errors(cx: &mut TestAppContext) {
    let (mut k, mock) = boot_with_mock_ai(cx);
    open_settings(&mut k);
    k.wait_label("settings.ai.model", "mock-model", |l| l == "mock-model");
    k.replace_text("settings.ai.base-url", &mock.broken_url);
    k.wait_until("base url edited", |k| {
        k.cx.update(|_, cx| crate::state::ai_config::current(cx)).base_url.as_deref() == Some(mock.broken_url.as_str())
    });
    k.click("settings.ai.test");
    k.wait_toast("Connection failed", |t| t.starts_with("Connection failed"));
    k.wait_label("settings.ai.test", "Failed - Retry", |l| l != "Testing...");
    assert_eq!(k.label("settings.ai.test").as_deref(), Some("Failed - Retry"));
    assert!(!k.toasts().iter().any(|t| t == "Connection successful"));
}

/// Scenario 30-cluster-ai-ollama-probe-fix (Deliberate Fix 13): the Ollama
/// availability probe uses the configured Base URL. Core's default Ollama URL
/// is the (answering) mock here, so probing the default would wrongly say
/// "available"; with Base URL on a closed port, reopening Settings marks
/// Ollama "not found" (disabled).
#[gpui_kit::test]
fn ollama_probe_uses_configured_base_url(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_settings(&mut k);
    k.wait_until("ollama available", |k| k.element("settings.ai.provider.ollama").is_some_and(|e| !e.disabled));
    k.replace_text("settings.ai.base-url", kore_testkit::CLOSED);
    k.wait_until("base url edited", |k| {
        k.cx.update(|_, cx| crate::state::ai_config::current(cx)).base_url.as_deref() == Some(kore_testkit::CLOSED)
    });
    // Leave Settings and come back so the probe runs again.
    k.click("sidebar.view.dashboard");
    k.wait_until("dashboard", |k| k.state()["view"] == "dashboard");
    open_settings(&mut k);
    k.wait_label("settings.ai.base-url", "base url field", |_| true);
    k.wait_until("ollama not found", |k| k.element("settings.ai.provider.ollama").is_some_and(|e| e.disabled));
    // Still the selected provider; only marked unavailable.
    assert!(k.is_selected("settings.ai.provider.ollama"));
}

/// Scenario 30-cluster-ai-chat: the empty state offers the suggestions; "Cluster
/// health" triggers the get_cluster_health tool call (status pill), then the
/// mock's markdown answer arrives and the pill goes away.
#[gpui_kit::test]
fn ai_chat_tool_call_then_answer(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_chat(&mut k);
    for s in ["Cluster health", "Failing pods", "High restarts", "Recent warnings", "Resource overview"] {
        let id = format!("chat.suggestion.{s}");
        k.wait_until(&format!("{id} enabled"), |k| k.element(&id).is_some_and(|e| !e.disabled));
    }
    assert!(k.element("banner.ai-config").is_none(), "configured: no warning");
    k.click("chat.suggestion.Cluster health");
    wait_msg(&mut k, "chat-msg-0", "user prompt", |l| l == "Give me an overview of the cluster health. Are there any issues?");
    k.wait_label("chat.tool-status", "Querying...", |l| l == "Querying get cluster health...");
    wait_msg(&mut k, "chat-msg-1", "markdown answer", |l| l == MARKDOWN_ANSWER);
    k.wait_gone("chat.tool-status");
    k.wait_gone("chat.progress");
    // The empty state is gone once there are messages.
    assert!(k.element("chat.suggestion.Cluster health").is_none());
    assert!(!k.is_disabled("chat.clear"));
}

/// Scenario 30-cluster-ai-chat (input): Shift+Enter inserts a newline, Enter
/// sends the two-line message; the answer follows.
#[gpui_kit::test]
fn ai_chat_shift_enter_newline_enter_sends(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_chat(&mut k);
    k.wait_until("input enabled", |k| k.element("chat.suggestion.Cluster health").is_some_and(|e| !e.disabled));
    k.click("chat.input");
    k.type_text("hello");
    k.keys("shift-enter");
    k.type_text("world");
    // Not sent yet: Shift+Enter only added a line.
    assert!(k.element("chat-msg-0").is_none());
    assert!(!k.is_disabled("chat.send"));
    k.keys("enter");
    wait_msg(&mut k, "chat-msg-0", "two-line user message", |l| l == "hello\nworld");
    wait_msg(&mut k, "chat-msg-1", "answer", |l| l == MARKDOWN_ANSWER);
    // The input was cleared on send.
    assert!(k.is_disabled("chat.send"));
}

/// Scenario 30-cluster-ai-error: an HTTP 500 ("boom") from the provider shows
/// up as a bold "Error:" assistant message with the status and body.
#[gpui_kit::test]
fn ai_chat_shows_provider_error(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_chat(&mut k);
    k.wait_until("input enabled", |k| k.element("chat.suggestion.Cluster health").is_some_and(|e| !e.disabled));
    k.click("chat.input");
    k.type_text("please boom");
    k.keys("enter");
    let msg = wait_msg(&mut k, "chat-msg-1", "error message", |l| l.starts_with("**Error:**"));
    assert!(msg.contains("500"), "status in {msg:?}");
    assert!(msg.contains("boom"), "body in {msg:?}");
    k.wait_gone("chat.progress");
}

/// Scenario 30-cluster-bug-chat-copy (quirk 10.13, fixed): hovering the
/// assistant bubble and clicking the outer part of its copy button (outside
/// the bubble, where it used to be clipped away) copies the raw message and
/// shows "Copied to clipboard".
#[gpui_kit::test]
fn chat_copy_button_outer_corner_copies(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_chat(&mut k);
    k.wait_until("input enabled", |k| k.element("chat.suggestion.Cluster health").is_some_and(|e| !e.disabled));
    k.click("chat.input");
    k.type_text("hello there");
    k.keys("enter");
    wait_msg(&mut k, "chat-msg-1", "answer", |l| l == MARKDOWN_ANSWER);
    assert!(k.element("chat-msg-0.copy").is_none(), "user messages have no copy button");
    k.hover("chat-msg-1.text");
    let copy = k.wait_for_element("chat-msg-1.copy");
    let text = k.wait_for_element("chat-msg-1.text");
    // The top-right corner of the button lies outside the (clipping) bubble.
    let (x, y) = (copy.bounds.x + copy.bounds.w - 3., copy.bounds.y + 3.);
    assert!(y < text.bounds.y && x > text.bounds.x + text.bounds.w, "button corner {x},{y} sticks out of the bubble {:?}", text.bounds);
    k.cx.simulate_click(gpui_kit::point(gpui_kit::px(x), gpui_kit::px(y)), gpui_kit::Modifiers::default());
    k.frame();
    k.wait_toast("Copied to clipboard", |t| t == "Copied to clipboard");
    let clip = k.cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(clip.as_deref(), Some(MARKDOWN_ANSWER));
    assert!(MARKDOWN_ANSWER.contains("Cluster **looks** fine."));
}

/// Scenario 30-cluster-ai-panel: the AI panel on a pod's details shows the
/// resource context and quick actions; "Diagnose" streams the mock answer;
/// closing (X) and reopening keeps the conversation.
#[gpui_kit::test]
fn ai_panel_diagnose_streams_and_survives_reopen(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.click("table.row.kore-demo/multi-container");
    k.wait_until("pod details", |k| k.state()["view"] == "details");
    k.click("header.ai");
    k.wait_label("ai-panel", "AI Assistant", |l| l == "AI Assistant");
    k.wait_settled("ai-panel");
    assert_eq!(k.label("ai-panel.context").as_deref(), Some("pods/kore-demo/multi-container"));
    for q in ["Diagnose", "Why is this failing?", "Suggest fixes"] {
        let id = format!("ai-panel.quick.{q}");
        k.wait_until(&format!("{id} enabled"), |k| k.element(&id).is_some_and(|e| !e.disabled));
    }
    k.click("ai-panel.quick.Diagnose");
    wait_msg(&mut k, "ai-panel-msg-0", "user prompt", |l| l == "Diagnose this resource and identify any issues.");
    // Streamed in chunks: the text grows until the final chunk.
    wait_msg(&mut k, "ai-panel-msg-1", "streamed answer", |l| l == "Diagnosis OK — KORE-MOCK-STREAM");
    k.wait_until("stream done", |k| k.element("ai-panel.send").is_some() && k.element("ai-panel.quick.Diagnose").is_none());

    k.click("ai-panel.close");
    k.wait_gone("ai-panel");
    assert!(k.state()["aiPanel"].is_null());
    k.click("header.ai");
    k.wait_for_element("ai-panel");
    k.wait_settled("ai-panel");
    wait_msg(&mut k, "ai-panel-msg-1", "kept answer", |l| l.contains("KORE-MOCK-STREAM"));
    k.click("ai-panel.close");
    k.wait_gone("ai-panel");
}

/// Scenario 30-cluster-ai-settings: an unconfigured provider (the OpenAI default,
/// no key) shows the AI Chat warning from boot.
#[gpui_kit::test]
fn ai_chat_unconfigured_by_default(cx: &mut TestAppContext) {
    seed_ai_config("openai", "gpt-4o", None);
    let (env, c1, c2) = fixtures::harness_env();
    let mut k = Kore::boot_env(cx, env, c1, c2);
    open_chat(&mut k);
    k.wait_for_element("banner.ai-config");
    assert!(k.is_disabled("chat.suggestion.Cluster health"));
}
