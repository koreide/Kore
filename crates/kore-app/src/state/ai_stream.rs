//! Streaming helper for the AI surfaces (`ai_chat` / `ai_diagnose`, spec 03 §6.3).
//!
//! The Core call returns a `Subscription`; the returned `Task` drains it,
//! coalescing bursts into one `AiUpdate::Events` per batch. Keep the task in a
//! field: dropping it cancels the request.

use crate::backend;
use crate::state::ai_config::AiConfig;
use gpui_kit::*;
use kore_core::{AIChatRequest, AIStreamEvent, DiagnoseRequest};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub enum AiUpdate {
    /// One or more stream events, in order.
    Events(Vec<AIStreamEvent>),
    /// The Core call itself failed; formatted message.
    Failed(String),
}

/// `<prefix>-<Date.now()>-<n>` session ids (`chat`, `ai-session`, `np-ai`).
pub fn session_id(prefix: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) + 1;
    format!("{prefix}-{}-{n}", chrono::Utc::now().timestamp_millis())
}

/// Start `ai_chat` (tool-calling chat) and feed its events to `on`.
pub fn start_chat<V: 'static>(
    cx: &mut Context<V>,
    config: &AiConfig,
    request: AIChatRequest,
    on: impl Fn(&mut V, AiUpdate, &mut Context<V>) + 'static,
) -> Task<()> {
    let config = config.to_core();
    let core = backend::core(cx);
    drive(cx, async move { core.ai_chat(config, request).await }, on)
}

/// Start `ai_diagnose` (streaming single answer) and feed its events to `on`.
pub fn start_diagnose<V: 'static>(
    cx: &mut Context<V>,
    config: &AiConfig,
    request: DiagnoseRequest,
    on: impl Fn(&mut V, AiUpdate, &mut Context<V>) + 'static,
) -> Task<()> {
    let config = config.to_core();
    let core = backend::core(cx);
    drive(cx, async move { core.ai_diagnose(config, request).await }, on)
}

fn drive<V: 'static>(
    cx: &mut Context<V>,
    start: impl std::future::Future<Output = Result<kore_core::Subscription<AIStreamEvent>, kore_core::K8sError>> + 'static,
    on: impl Fn(&mut V, AiUpdate, &mut Context<V>) + 'static,
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        let mut sub = match start.await {
            Ok(s) => s,
            Err(e) => {
                let msg = backend::format_error(&e);
                this.update(cx, |v, cx| on(v, AiUpdate::Failed(msg), cx)).ok();
                return;
            }
        };
        while let Some(first) = sub.recv().await {
            let mut batch = vec![first];
            batch.extend(sub.try_recv_batch(256));
            let end = batch.iter().any(|e| matches!(e, AIStreamEvent::Done | AIStreamEvent::Error { .. }));
            if this.update(cx, |v, cx| on(v, AiUpdate::Events(batch), cx)).is_err() || end {
                return;
            }
        }
        // Stream closed without a terminal event (cancelled backend task):
        // finish so the surface never stays stuck in "streaming".
        this.update(cx, |v, cx| on(v, AiUpdate::Events(vec![AIStreamEvent::Done]), cx)).ok();
    })
}
