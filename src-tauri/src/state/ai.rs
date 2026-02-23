use crate::error::{K8sError, Result};
use crate::state::{K8sState, ResourceKind};
use rand::Rng;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{error, info, warn};

// ── Types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AIProvider {
    OpenAI,
    Anthropic,
    Ollama,
    #[serde(rename = "claude_cli")]
    ClaudeCli,
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
    pub kind: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub session_id: Option<String>,
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

    // Use the user's prompt if provided, otherwise build a default diagnose message
    if let (Some(kind), Some(name), Some(ns)) = (&request.kind, &request.name, &request.namespace) {
        let user_prompt = request
            .prompt
            .as_deref()
            .unwrap_or("Diagnose this resource and identify any issues.");
        parts.push(format!(
            "{user_prompt}\n\nResource: Kubernetes {kind} `{name}` in namespace `{ns}`{}.",
            context_name
                .map(|c| format!(" (context: {c})"))
                .unwrap_or_default()
        ));
    } else if let Some(prompt) = &request.prompt {
        parts.push(prompt.clone());
    } else {
        parts.push("Diagnose the current Kubernetes cluster and identify any issues.".to_string());
    }

    if let Some(resource) = resource_json {
        let pretty = serde_json::to_string_pretty(resource).unwrap_or_default();
        let truncated = if pretty.len() > 12_000 {
            format!("{}...\n[truncated]", &pretty[..12_000])
        } else {
            pretty
        };
        parts.push(format!(
            "## Resource Description\n```json\n{truncated}\n```"
        ));
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
        // Use the frontend-provided session ID, or generate one as fallback
        let session_id: String = request.session_id.clone().unwrap_or_else(|| {
            let mut rng = rand::thread_rng();
            (0..16)
                .map(|_| format!("{:02x}", rng.gen::<u8>()))
                .collect()
        });
        let event_name = format!("ai-response://{session_id}");

        let kind_str = request.kind.clone().unwrap_or_default();
        let name_str = request.name.clone().unwrap_or_default();
        let ns_str = request.namespace.clone().unwrap_or_default();

        info!(
            kind = %kind_str,
            name = %name_str,
            namespace = %ns_str,
            provider = ?config.provider,
            "Starting AI diagnosis"
        );

        // ── Gather context (only if resource info is provided) ───────

        let has_resource =
            request.kind.is_some() && request.name.is_some() && request.namespace.is_some();

        let resource_kind = if has_resource {
            parse_resource_kind(&kind_str)
        } else {
            None
        };

        // Describe the resource
        let resource_json = if let Some(ref rk) = resource_kind {
            match self
                .describe_resource(rk.clone(), ns_str.clone(), name_str.clone())
                .await
            {
                Ok(val) => Some(val),
                Err(e) => {
                    warn!(error = %e, "Failed to describe resource for AI context");
                    None
                }
            }
        } else {
            None
        };

        // List events for the resource
        let events = if has_resource {
            match self
                .list_events_for_resource(kind_str.clone(), ns_str.clone(), name_str.clone())
                .await
            {
                Ok(evts) => evts,
                Err(e) => {
                    warn!(error = %e, "Failed to list events for AI context");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // Fetch pod logs if the resource is a pod
        let logs = if has_resource && matches!(kind_str.to_lowercase().as_str(), "pod" | "pods") {
            match self
                .fetch_logs(ns_str.clone(), name_str.clone(), None, Some(200), false)
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
        let metrics = if has_resource && matches!(kind_str.to_lowercase().as_str(), "pod" | "pods")
        {
            match self.get_pod_metrics(ns_str.clone(), name_str.clone()).await {
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
            if let Err(e) =
                stream_ai_response(&handle, &evt, &config, &system_prompt, &user_message).await
            {
                error!(error = %e, "AI streaming failed");
                let _ = handle.emit(
                    &evt,
                    &AIStreamEvent::Error {
                        message: e.to_string(),
                    },
                );
            }
        });

        Ok(())
    }

    /// List locally installed Ollama models.
    pub async fn list_ollama_models(base_url: Option<&str>) -> Result<Vec<String>> {
        let http = HttpClient::new();
        let base = base_url.unwrap_or("http://localhost:11434");

        let resp = http
            .get(format!("{base}/api/tags"))
            .send()
            .await
            .map_err(|e| K8sError::Validation(format!("Ollama connection failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(K8sError::Validation(format!(
                "Ollama returned HTTP {}",
                resp.status()
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| K8sError::Validation(format!("Failed to parse Ollama response: {e}")))?;

        let models = body
            .get("models")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        m.get("name")
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    /// Check if the `claude` CLI is available in PATH.
    pub async fn claude_cli_available() -> Result<bool> {
        let output = tokio::process::Command::new("claude")
            .arg("--version")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;

        match output {
            Ok(o) => Ok(o.status.success()),
            Err(_) => Ok(false),
        }
    }

    /// Return the known Claude CLI model aliases.
    pub async fn list_claude_models() -> Result<Vec<String>> {
        // Verify CLI is available first
        if !Self::claude_cli_available().await? {
            return Err(K8sError::Validation(
                "Claude CLI is not installed or not in PATH".into(),
            ));
        }
        Ok(vec![
            "opus".to_string(),
            "sonnet".to_string(),
            "haiku".to_string(),
        ])
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
                    .map_err(|e| {
                        K8sError::Validation(format!("Anthropic connection failed: {e}"))
                    })?;

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
            AIProvider::ClaudeCli => Self::claude_cli_available().await?,
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
    // Claude CLI uses a subprocess instead of HTTP
    if matches!(config.provider, AIProvider::ClaudeCli) {
        return stream_claude_cli_response(app, event_name, config, system_prompt, user_message)
            .await;
    }

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
        AIProvider::ClaudeCli => {
            // Handled above via early return
            unreachable!()
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let msg = format!("AI provider returned HTTP {status}: {body}");
        app.emit(
            event_name,
            &AIStreamEvent::Error {
                message: msg.clone(),
            },
        )?;
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
                AIProvider::ClaudeCli => unreachable!(),
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
                "message_delta"
                | "message_start"
                | "content_block_start"
                | "content_block_stop"
                | "ping" => SSEContent::Skip,
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

// ── Claude CLI streaming ─────────────────────────────────────────────────

/// Stream AI response via the `claude` CLI subprocess.
async fn stream_claude_cli_response(
    app: &AppHandle,
    event_name: &str,
    config: &AIConfig,
    system_prompt: &str,
    user_message: &str,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let full_prompt = format!("{system_prompt}\n\n{user_message}");

    let mut child = tokio::process::Command::new("claude")
        .arg("-p")
        .arg(&full_prompt)
        .arg("--model")
        .arg(&config.model)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--include-partial-messages")
        .arg("--verbose")
        .arg("--no-session-persistence")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env_remove("CLAUDECODE")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn claude CLI: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture claude CLI stdout")?;

    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to capture claude CLI stderr")?;

    // Drain stderr in background to prevent pipe deadlock, and capture output
    let stderr_handle = tokio::spawn(async move {
        let mut stderr_reader = BufReader::new(stderr);
        let mut stderr_buf = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr_reader, &mut stderr_buf).await;
        stderr_buf
    });

    let mut reader = BufReader::new(stdout).lines();
    let mut received_chunks = false;

    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match parse_claude_cli_stream_line(trimmed, received_chunks) {
            SSEContent::Text(text) => {
                if !text.is_empty() {
                    received_chunks = true;
                    app.emit(event_name, &AIStreamEvent::Chunk { content: text })?;
                }
            }
            SSEContent::Done => {
                app.emit(event_name, &AIStreamEvent::Done)?;
                let _ = child.wait().await;
                return Ok(());
            }
            SSEContent::Skip => {}
        }
    }

    // Wait for the process to finish
    let status = child.wait().await?;
    let stderr_output = stderr_handle.await.unwrap_or_default();

    if !status.success() {
        let detail = if stderr_output.trim().is_empty() {
            format!("Claude CLI exited with status {status}")
        } else {
            // Take last few lines of stderr for the error message
            let last_lines: String = stderr_output
                .lines()
                .rev()
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            format!("Claude CLI error: {last_lines}")
        };
        app.emit(
            event_name,
            &AIStreamEvent::Error {
                message: detail.clone(),
            },
        )?;
        return Err(detail.into());
    }

    // Stream ended — emit done
    app.emit(event_name, &AIStreamEvent::Done)?;
    Ok(())
}

/// Parse a Claude CLI stream-json line (JSONL format).
///
/// With `--include-partial-messages`, streaming deltas look like:
/// `{"type":"content_block_delta",...,"delta":{"type":"text_delta","text":"Hello"}}`
///
/// Without `--include-partial-messages`, complete messages look like:
/// `{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}],...}}`
///
/// Result events look like:
/// `{"type":"result","result":"full text",...}`
fn parse_claude_cli_stream_line(line: &str, already_received_chunks: bool) -> SSEContent {
    let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(line);
    match parsed {
        Ok(val) => {
            let top_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

            if top_type == "result" {
                // The result event contains the full text in "result" field.
                // Only use it if we haven't received any streaming deltas yet,
                // to avoid duplicating content.
                if !already_received_chunks {
                    if let Some(text) = val.get("result").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            return SSEContent::Text(text.to_string());
                        }
                    }
                }
                return SSEContent::Done;
            }

            // Streaming delta: content_block_delta at top level
            if top_type == "content_block_delta" {
                let text = val
                    .pointer("/delta/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return SSEContent::Text(text);
            }

            // Complete assistant message (emitted without --include-partial-messages)
            if top_type == "assistant" {
                let text = val
                    .pointer("/message/content")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| {
                        let parts: Vec<&str> = arr
                            .iter()
                            .filter_map(|block| {
                                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                    block.get("text").and_then(|t| t.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if parts.is_empty() {
                            None
                        } else {
                            Some(parts.join(""))
                        }
                    })
                    .unwrap_or_default();
                if !text.is_empty() {
                    return SSEContent::Text(text);
                }
                return SSEContent::Skip;
            }

            SSEContent::Skip
        }
        Err(_) => SSEContent::Skip,
    }
}
