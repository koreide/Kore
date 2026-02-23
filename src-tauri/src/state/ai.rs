use crate::error::{K8sError, Result};
use crate::state::{K8sState, ResourceKind};
use rand::Rng;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{error, info, warn};

// ── Types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AIProvider {
    OpenAI,
    Anthropic,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfig {
    pub provider: AIProvider,
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnoseRequest {
    pub kind: String,
    pub namespace: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnoseResponse {
    pub analysis: String,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum AIStreamEvent {
    #[serde(rename = "chunk")]
    Chunk { content: String },
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    Error { message: String },
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Map a string kind (from the frontend) to `ResourceKind`.
fn parse_resource_kind(kind: &str) -> Option<ResourceKind> {
    match kind.to_lowercase().as_str() {
        "pod" | "pods" => Some(ResourceKind::Pods),
        "deployment" | "deployments" => Some(ResourceKind::Deployments),
        "service" | "services" => Some(ResourceKind::Services),
        "node" | "nodes" => Some(ResourceKind::Nodes),
        "event" | "events" => Some(ResourceKind::Events),
        "configmap" | "configmaps" => Some(ResourceKind::Configmaps),
        "secret" | "secrets" => Some(ResourceKind::Secrets),
        "ingress" | "ingresses" => Some(ResourceKind::Ingresses),
        "job" | "jobs" => Some(ResourceKind::Jobs),
        "cronjob" | "cronjobs" => Some(ResourceKind::Cronjobs),
        _ => None,
    }
}

fn build_system_prompt() -> String {
    "You are a Kubernetes troubleshooting assistant embedded in Kore, a desktop \
     Kubernetes management tool. Your job is to analyze resource state, events, \
     logs, and metrics to diagnose problems and suggest actionable fixes.\n\n\
     Guidelines:\n\
     - Be concise but thorough.\n\
     - Start with a summary of the problem.\n\
     - List concrete, actionable suggestions.\n\
     - Reference specific fields from the resource spec/status when relevant.\n\
     - If the resource looks healthy, say so and mention what you checked.\n\
     - Format your response in Markdown."
        .to_string()
}

fn build_user_message(
    request: &DiagnoseRequest,
    resource_json: Option<&serde_json::Value>,
    events: &[serde_json::Value],
    logs: Option<&str>,
    metrics: Option<&serde_json::Value>,
    context_name: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    parts.push(format!(
        "Diagnose this Kubernetes {} `{}` in namespace `{}`{}.",
        request.kind,
        request.name,
        request.namespace,
        context_name
            .map(|c| format!(" (context: {c})"))
            .unwrap_or_default()
    ));

    if let Some(resource) = resource_json {
        let pretty = serde_json::to_string_pretty(resource).unwrap_or_default();
        let truncated = if pretty.len() > 12_000 {
            format!("{}...\n[truncated]", &pretty[..12_000])
        } else {
            pretty
        };
        parts.push(format!("## Resource Description\n```json\n{truncated}\n```"));
    }

    if !events.is_empty() {
        let events_text: Vec<String> = events
            .iter()
            .take(30)
            .map(|e| {
                let reason = e.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                let message = e.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let count = e.get("count").and_then(|v| v.as_i64()).unwrap_or(1);
                let event_type = e.get("type").and_then(|v| v.as_str()).unwrap_or("Normal");
                let last = e
                    .get("lastTimestamp")
                    .or_else(|| e.pointer("/metadata/creationTimestamp"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                format!("- [{event_type}] {reason} (x{count}) at {last}: {message}")
            })
            .collect();
        let joined = events_text.join("\n");
        parts.push(format!("## Events\n{joined}"));
    } else {
        parts.push("## Events\nNo events found for this resource.".to_string());
    }

    if let Some(log_text) = logs {
        let truncated = if log_text.len() > 8_000 {
            let start = log_text.len() - 8_000;
            format!("[truncated]...\n{}", &log_text[start..])
        } else {
            log_text.to_string()
        };
        parts.push(format!("## Recent Logs (tail)\n```\n{truncated}\n```"));
    }

    if let Some(m) = metrics {
        let pretty = serde_json::to_string_pretty(m).unwrap_or_default();
        parts.push(format!("## Metrics\n```json\n{pretty}\n```"));
    }

    parts.join("\n\n")
}

// ── Implementation ───────────────────────────────────────────────────────

impl K8sState {
    /// Diagnose a Kubernetes resource using an AI provider.
    ///
    /// Gathers resource description, events, logs (for pods), and metrics,
    /// then streams the AI response back to the frontend via Tauri events.
    pub async fn ai_diagnose(
        &self,
        app: AppHandle,
        config: AIConfig,
        request: DiagnoseRequest,
    ) -> Result<()> {
        let session_id: String = {
            let mut rng = rand::thread_rng();
            (0..16).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
        };
        let event_name = format!("ai-response://{session_id}");

        info!(
            kind = %request.kind,
            name = %request.name,
            namespace = %request.namespace,
            provider = ?config.provider,
            "Starting AI diagnosis"
        );

        // Emit the session ID so the frontend knows where to listen
        if let Err(e) = app.emit(
            "ai-session-started",
            &json!({ "sessionId": session_id }),
        ) {
            error!(error = %e, "Failed to emit session start");
        }

        // ── Gather context ───────────────────────────────────────────

        let resource_kind = parse_resource_kind(&request.kind);

        // Describe the resource
        let resource_json = if let Some(ref rk) = resource_kind {
            match self
                .describe_resource(
                    rk.clone(),
                    request.namespace.clone(),
                    request.name.clone(),
                )
                .await
            {
                Ok(val) => Some(val),
                Err(e) => {
                    warn!(error = %e, "Failed to describe resource for AI context");
                    None
                }
            }
        } else {
            warn!(kind = %request.kind, "Unknown resource kind for AI diagnosis");
            None
        };

        // List events for the resource
        let events = match self
            .list_events_for_resource(
                request.kind.clone(),
                request.namespace.clone(),
                request.name.clone(),
            )
            .await
        {
            Ok(evts) => evts,
            Err(e) => {
                warn!(error = %e, "Failed to list events for AI context");
                Vec::new()
            }
        };

        // Fetch pod logs if the resource is a pod
        let logs = if matches!(
            request.kind.to_lowercase().as_str(),
            "pod" | "pods"
        ) {
            match self
                .fetch_logs(
                    request.namespace.clone(),
                    request.name.clone(),
                    None,
                    Some(200),
                    false,
                )
                .await
            {
                Ok(text) => Some(text),
                Err(e) => {
                    warn!(error = %e, "Failed to fetch pod logs for AI context");
                    None
                }
            }
        } else {
            None
        };

        // Fetch metrics if the resource is a pod
        let metrics = if matches!(
            request.kind.to_lowercase().as_str(),
            "pod" | "pods"
        ) {
            match self
                .get_pod_metrics(
                    request.namespace.clone(),
                    request.name.clone(),
                )
                .await
            {
                Ok(m) => Some(m),
                Err(e) => {
                    warn!(error = %e, "Failed to fetch metrics for AI context");
                    None
                }
            }
        } else {
            None
        };

        let context_name = self.current_context_name().await;
        let user_message = build_user_message(
            &request,
            resource_json.as_ref(),
            &events,
            logs.as_deref(),
            metrics.as_ref(),
            context_name.as_deref(),
        );
        let system_prompt = build_system_prompt();

        // ── Stream from AI provider (spawned task) ───────────────────

        let handle = app.clone();
        let evt = event_name.clone();

        tauri::async_runtime::spawn(async move {
            if let Err(e) = stream_ai_response(&handle, &evt, &config, &system_prompt, &user_message).await {
                error!(error = %e, "AI streaming failed");
                let _ = handle.emit(&evt, &AIStreamEvent::Error {
                    message: e.to_string(),
                });
            }
        });

        Ok(())
    }

    /// Test that the configured AI provider is reachable and the API key is valid.
    pub async fn ai_test_connection(config: &AIConfig) -> Result<bool> {
        let http = HttpClient::new();

        let result = match config.provider {
            AIProvider::OpenAI => {
                let api_key = config
                    .api_key
                    .as_deref()
                    .ok_or_else(|| K8sError::Validation("OpenAI API key is required".into()))?;

                let resp = http
                    .get("https://api.openai.com/v1/models")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .send()
                    .await
                    .map_err(|e| K8sError::Validation(format!("OpenAI connection failed: {e}")))?;

                resp.status().is_success()
            }
            AIProvider::Anthropic => {
                let api_key = config
                    .api_key
                    .as_deref()
                    .ok_or_else(|| K8sError::Validation("Anthropic API key is required".into()))?;

                let resp = http
                    .post("https://api.anthropic.com/v1/messages")
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .json(&json!({
                        "model": &config.model,
                        "max_tokens": 1,
                        "messages": [{"role": "user", "content": "hi"}]
                    }))
                    .send()
                    .await
                    .map_err(|e| K8sError::Validation(format!("Anthropic connection failed: {e}")))?;

                resp.status().is_success()
            }
            AIProvider::Ollama => {
                let base = config
                    .base_url
                    .as_deref()
                    .unwrap_or("http://localhost:11434");

                let resp = http
                    .get(format!("{base}/api/tags"))
                    .send()
                    .await
                    .map_err(|e| K8sError::Validation(format!("Ollama connection failed: {e}")))?;

                resp.status().is_success()
            }
        };

        info!(provider = ?config.provider, success = result, "AI connection test");
        Ok(result)
    }
}

// ── Streaming implementation ─────────────────────────────────────────────

async fn stream_ai_response(
    app: &AppHandle,
    event_name: &str,
    config: &AIConfig,
    system_prompt: &str,
    user_message: &str,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let http = HttpClient::new();

    let response = match config.provider {
        AIProvider::OpenAI => {
            let api_key = config
                .api_key
                .as_deref()
                .ok_or("OpenAI API key is required")?;

            http.post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Content-Type", "application/json")
                .json(&json!({
                    "model": &config.model,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_message}
                    ],
                    "stream": true
                }))
                .send()
                .await?
        }
        AIProvider::Anthropic => {
            let api_key = config
                .api_key
                .as_deref()
                .ok_or("Anthropic API key is required")?;

            http.post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&json!({
                    "model": &config.model,
                    "system": system_prompt,
                    "messages": [
                        {"role": "user", "content": user_message}
                    ],
                    "max_tokens": 4096,
                    "stream": true
                }))
                .send()
                .await?
        }
        AIProvider::Ollama => {
            let base = config
                .base_url
                .as_deref()
                .unwrap_or("http://localhost:11434");

            http.post(format!("{base}/api/chat"))
                .header("Content-Type", "application/json")
                .json(&json!({
                    "model": &config.model,
                    "messages": [
                        {"role": "system", "content": system_prompt},
                        {"role": "user", "content": user_message}
                    ],
                    "stream": true
                }))
                .send()
                .await?
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let msg = format!("AI provider returned HTTP {status}: {body}");
        app.emit(event_name, &AIStreamEvent::Error { message: msg.clone() })?;
        return Err(msg.into());
    }

    // Process the SSE / streaming response line by line
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);

        // Process complete lines from the buffer
        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            let content = match config.provider {
                AIProvider::OpenAI => parse_openai_sse_line(&line),
                AIProvider::Anthropic => parse_anthropic_sse_line(&line),
                AIProvider::Ollama => parse_ollama_stream_line(&line),
            };

            match content {
                SSEContent::Text(text) => {
                    if !text.is_empty() {
                        app.emit(event_name, &AIStreamEvent::Chunk { content: text })?;
                    }
                }
                SSEContent::Done => {
                    app.emit(event_name, &AIStreamEvent::Done)?;
                    return Ok(());
                }
                SSEContent::Skip => {}
            }
        }
    }

    // Stream ended — emit done if we haven't already
    app.emit(event_name, &AIStreamEvent::Done)?;
    Ok(())
}

enum SSEContent {
    Text(String),
    Done,
    Skip,
}

/// Parse an OpenAI SSE line. Lines look like:
/// `data: {"choices":[{"delta":{"content":"Hello"}}]}`
/// `data: [DONE]`
fn parse_openai_sse_line(line: &str) -> SSEContent {
    let data = match line.strip_prefix("data: ") {
        Some(d) => d.trim(),
        None => return SSEContent::Skip,
    };

    if data == "[DONE]" {
        return SSEContent::Done;
    }

    let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(data);
    match parsed {
        Ok(val) => {
            let content = val
                .pointer("/choices/0/delta/content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            SSEContent::Text(content)
        }
        Err(_) => SSEContent::Skip,
    }
}

/// Parse an Anthropic SSE line. Lines look like:
/// `event: content_block_delta`
/// `data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello"}}`
/// `event: message_stop`
fn parse_anthropic_sse_line(line: &str) -> SSEContent {
    let data = match line.strip_prefix("data: ") {
        Some(d) => d.trim(),
        None => {
            return SSEContent::Skip;
        }
    };

    let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(data);
    match parsed {
        Ok(val) => {
            let event_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match event_type {
                "content_block_delta" => {
                    let text = val
                        .pointer("/delta/text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    SSEContent::Text(text)
                }
                "message_stop" => SSEContent::Done,
                "message_delta" | "message_start" | "content_block_start"
                | "content_block_stop" | "ping" => SSEContent::Skip,
                _ => SSEContent::Skip,
            }
        }
        Err(_) => SSEContent::Skip,
    }
}

/// Parse an Ollama streaming JSON line. Each line is a complete JSON object:
/// `{"model":"...","message":{"role":"assistant","content":"Hello"},"done":false}`
/// `{"model":"...","message":{"role":"assistant","content":""},"done":true}`
fn parse_ollama_stream_line(line: &str) -> SSEContent {
    let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(line);
    match parsed {
        Ok(val) => {
            let done = val.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
            if done {
                return SSEContent::Done;
            }
            let content = val
                .pointer("/message/content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            SSEContent::Text(content)
        }
        Err(_) => SSEContent::Skip,
    }
}
