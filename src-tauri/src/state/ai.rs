use crate::constants::MAX_AI_RESPONSE_BYTES;
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
    #[serde(rename = "cursor_agent")]
    CursorAgent,
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
    #[serde(rename = "status")]
    Status { message: String },
}

// ── Chat Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIChatRequest {
    pub messages: Vec<ChatMessage>,
    pub session_id: String,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone)]
struct ToolCall {
    id: String,
    name: String,
    arguments: serde_json::Value,
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Strip ANSI escape sequences from a string (e.g. color codes, cursor movement).
fn strip_ansi_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                              // Consume until we hit a letter (the terminator)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

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
        "namespace" | "namespaces" => Some(ResourceKind::Namespaces),
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

// ── Tool Definitions ────────────────────────────────────────────────────

fn build_tool_definitions(provider: &AIProvider) -> Vec<serde_json::Value> {
    let tools = [
        json!({
            "name": "list_resources",
            "description": "List Kubernetes resources of a given kind. Returns summarized fields for each resource.",
            "parameters": {
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["pods", "deployments", "services", "nodes", "events", "configmaps", "secrets", "ingresses", "jobs", "cronjobs"],
                        "description": "The resource kind to list"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Namespace to filter by. Omit for all namespaces or cluster-scoped resources."
                    },
                    "label_selector": {
                        "type": "string",
                        "description": "Label selector to filter resources (e.g. 'app=nginx')"
                    }
                },
                "required": ["kind"]
            }
        }),
        json!({
            "name": "get_cluster_health",
            "description": "Get overall cluster health including health score, pod counts, node status, restart hotlist, pending pods, and recent warnings.",
            "parameters": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "describe_resource",
            "description": "Get full details (spec, status, conditions) for a specific resource. Returns truncated JSON.",
            "parameters": {
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["pods", "deployments", "services", "nodes", "events", "configmaps", "secrets", "ingresses", "jobs", "cronjobs"],
                        "description": "The resource kind"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "The namespace of the resource"
                    },
                    "name": {
                        "type": "string",
                        "description": "The name of the resource"
                    }
                },
                "required": ["kind", "namespace", "name"]
            }
        }),
        json!({
            "name": "get_resource_events",
            "description": "Get events for a specific Kubernetes resource.",
            "parameters": {
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "description": "The resource kind (e.g. 'Pod', 'Deployment')"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "The namespace of the resource"
                    },
                    "name": {
                        "type": "string",
                        "description": "The name of the resource"
                    }
                },
                "required": ["kind", "namespace", "name"]
            }
        }),
        json!({
            "name": "get_pod_logs",
            "description": "Get recent logs for a pod.",
            "parameters": {
                "type": "object",
                "properties": {
                    "namespace": {
                        "type": "string",
                        "description": "Pod namespace"
                    },
                    "pod": {
                        "type": "string",
                        "description": "Pod name"
                    },
                    "tail_lines": {
                        "type": "integer",
                        "description": "Number of recent log lines to fetch (default 100)"
                    },
                    "container": {
                        "type": "string",
                        "description": "Container name (for multi-container pods)"
                    }
                },
                "required": ["namespace", "pod"]
            }
        }),
        json!({
            "name": "get_pod_metrics",
            "description": "Get CPU and memory metrics for a pod (requires Metrics Server).",
            "parameters": {
                "type": "object",
                "properties": {
                    "namespace": {
                        "type": "string",
                        "description": "Pod namespace"
                    },
                    "pod": {
                        "type": "string",
                        "description": "Pod name"
                    }
                },
                "required": ["namespace", "pod"]
            }
        }),
        json!({
            "name": "search_resources",
            "description": "Search across all resource kinds for resources matching a query string.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (matches against name, namespace, labels)"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Namespace to limit search to"
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "list_namespaces",
            "description": "List all available namespaces in the cluster.",
            "parameters": {
                "type": "object",
                "properties": {}
            }
        }),
    ];

    match provider {
        AIProvider::OpenAI | AIProvider::Ollama => tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": t
                })
            })
            .collect(),
        AIProvider::Anthropic => tools
            .iter()
            .map(|t| {
                json!({
                    "name": t["name"],
                    "description": t["description"],
                    "input_schema": t["parameters"]
                })
            })
            .collect(),
        AIProvider::ClaudeCli | AIProvider::CursorAgent => vec![],
    }
}

/// Summarize a list of resources to reduce token usage.
fn summarize_resources(kind: &str, resources: &[serde_json::Value]) -> serde_json::Value {
    let items: Vec<serde_json::Value> = resources
        .iter()
        .take(100) // cap at 100
        .map(|r| {
            let name = r
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let namespace = r
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let creation = r
                .pointer("/metadata/creationTimestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match kind.to_lowercase().as_str() {
                "pods" => {
                    let phase = r
                        .pointer("/status/phase")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown");
                    let restarts: i64 = r
                        .pointer("/status/containerStatuses")
                        .and_then(|v| v.as_array())
                        .map(|cs| {
                            cs.iter()
                                .map(|c| {
                                    c.get("restartCount").and_then(|v| v.as_i64()).unwrap_or(0)
                                })
                                .sum()
                        })
                        .unwrap_or(0);
                    let ready = r
                        .pointer("/status/containerStatuses")
                        .and_then(|v| v.as_array())
                        .map(|cs| {
                            let total = cs.len();
                            let rdy = cs
                                .iter()
                                .filter(|c| {
                                    c.get("ready").and_then(|v| v.as_bool()).unwrap_or(false)
                                })
                                .count();
                            format!("{rdy}/{total}")
                        })
                        .unwrap_or_default();
                    let node = r
                        .pointer("/spec/nodeName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    // Detect issues
                    let mut issues = Vec::new();
                    if let Some(cs) = r
                        .pointer("/status/containerStatuses")
                        .and_then(|v| v.as_array())
                    {
                        for c in cs {
                            if let Some(waiting) =
                                c.pointer("/state/waiting/reason").and_then(|v| v.as_str())
                            {
                                issues.push(waiting.to_string());
                            }
                        }
                    }

                    json!({
                        "name": name, "namespace": namespace, "status": phase,
                        "restarts": restarts, "ready": ready, "node": node,
                        "age": creation, "issues": issues
                    })
                }
                "deployments" => {
                    let desired = r
                        .pointer("/spec/replicas")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let ready = r
                        .pointer("/status/readyReplicas")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let available = r
                        .pointer("/status/availableReplicas")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    json!({
                        "name": name, "namespace": namespace,
                        "replicas_desired": desired, "replicas_ready": ready,
                        "replicas_available": available, "age": creation
                    })
                }
                "services" => {
                    let svc_type = r
                        .pointer("/spec/type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ClusterIP");
                    let cluster_ip = r
                        .pointer("/spec/clusterIP")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let ports: Vec<String> = r
                        .pointer("/spec/ports")
                        .and_then(|v| v.as_array())
                        .map(|ps| {
                            ps.iter()
                                .map(|p| {
                                    let port = p.get("port").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let proto =
                                        p.get("protocol").and_then(|v| v.as_str()).unwrap_or("TCP");
                                    format!("{port}/{proto}")
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    json!({
                        "name": name, "namespace": namespace,
                        "type": svc_type, "clusterIP": cluster_ip,
                        "ports": ports.join(", "), "age": creation
                    })
                }
                "nodes" => {
                    let status = r
                        .pointer("/status/conditions")
                        .and_then(|v| v.as_array())
                        .and_then(|conds| {
                            conds
                                .iter()
                                .find(|c| c.get("type").and_then(|v| v.as_str()) == Some("Ready"))
                        })
                        .and_then(|c| c.get("status").and_then(|v| v.as_str()))
                        .map(|s| if s == "True" { "Ready" } else { "NotReady" })
                        .unwrap_or("Unknown");
                    let version = r
                        .pointer("/status/nodeInfo/kubeletVersion")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    json!({
                        "name": name, "status": status,
                        "version": version, "age": creation
                    })
                }
                "events" => {
                    let reason = r.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                    let message = r.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    let event_type = r.get("type").and_then(|v| v.as_str()).unwrap_or("Normal");
                    let count = r.get("count").and_then(|v| v.as_i64()).unwrap_or(1);
                    let last = r
                        .get("lastTimestamp")
                        .or_else(|| r.pointer("/metadata/creationTimestamp"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let involved = r
                        .pointer("/involvedObject/name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    json!({
                        "type": event_type, "reason": reason, "message": message,
                        "count": count, "last_seen": last,
                        "involved_object": involved, "namespace": namespace
                    })
                }
                _ => {
                    let labels = r.pointer("/metadata/labels").cloned().unwrap_or(json!({}));
                    json!({
                        "name": name, "namespace": namespace,
                        "age": creation, "labels": labels
                    })
                }
            }
        })
        .collect();

    json!({
        "kind": kind,
        "count": resources.len(),
        "items": items
    })
}

/// Execute a tool call against K8sState and return the result as JSON.
async fn execute_tool(
    state: &K8sState,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    match tool_name {
        "list_resources" => {
            let kind_str = arguments
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("pods");
            let rk = parse_resource_kind(kind_str)
                .ok_or_else(|| format!("Unknown resource kind: {kind_str}"))?;
            let namespace = arguments
                .get("namespace")
                .and_then(|v| v.as_str())
                .map(String::from);
            let label_selector = arguments
                .get("label_selector")
                .and_then(|v| v.as_str())
                .map(String::from);
            let resources = state
                .list_resources(rk, namespace, label_selector)
                .await
                .map_err(|e| e.to_string())?;
            Ok(summarize_resources(kind_str, &resources))
        }
        "get_cluster_health" => {
            let health = state
                .get_cluster_health()
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(health).map_err(|e| e.to_string())
        }
        "describe_resource" => {
            let kind_str = arguments
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("pods");
            let rk = parse_resource_kind(kind_str)
                .ok_or_else(|| format!("Unknown resource kind: {kind_str}"))?;
            let namespace = arguments
                .get("namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("name is required")?
                .to_string();
            let desc = state
                .describe_resource(rk, namespace, name)
                .await
                .map_err(|e| e.to_string())?;
            // Truncate to 8KB
            let pretty = serde_json::to_string_pretty(&desc).unwrap_or_default();
            if pretty.len() > 8_000 {
                Ok(json!({
                    "data": &pretty[..8_000],
                    "truncated": true
                }))
            } else {
                Ok(desc)
            }
        }
        "get_resource_events" => {
            let kind = arguments
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("Pod")
                .to_string();
            let namespace = arguments
                .get("namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("name is required")?
                .to_string();
            let events = state
                .list_events_for_resource(kind, namespace, name)
                .await
                .map_err(|e| e.to_string())?;
            let summarized: Vec<serde_json::Value> = events
                .iter()
                .take(30)
                .map(|e| {
                    json!({
                        "type": e.get("type").and_then(|v| v.as_str()).unwrap_or("Normal"),
                        "reason": e.get("reason").and_then(|v| v.as_str()).unwrap_or(""),
                        "message": e.get("message").and_then(|v| v.as_str()).unwrap_or(""),
                        "count": e.get("count").and_then(|v| v.as_i64()).unwrap_or(1),
                    })
                })
                .collect();
            Ok(json!({ "events": summarized }))
        }
        "get_pod_logs" => {
            let namespace = arguments
                .get("namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let pod = arguments
                .get("pod")
                .and_then(|v| v.as_str())
                .ok_or("pod is required")?
                .to_string();
            let tail_lines = arguments
                .get("tail_lines")
                .and_then(|v| v.as_i64())
                .or(Some(100));
            let container = arguments
                .get("container")
                .and_then(|v| v.as_str())
                .map(String::from);
            let logs = state
                .fetch_logs(namespace, pod, container, tail_lines, false)
                .await
                .map_err(|e| e.to_string())?;
            // Truncate logs to 6KB
            let truncated = if logs.len() > 6_000 {
                format!("[truncated]...\n{}", &logs[logs.len() - 6_000..])
            } else {
                logs
            };
            Ok(json!({ "logs": truncated }))
        }
        "get_pod_metrics" => {
            let namespace = arguments
                .get("namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let pod = arguments
                .get("pod")
                .and_then(|v| v.as_str())
                .ok_or("pod is required")?
                .to_string();
            let metrics = state
                .get_pod_metrics(namespace, pod)
                .await
                .map_err(|e| e.to_string())?;
            Ok(metrics)
        }
        "search_resources" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or("query is required")?
                .to_string();
            let namespace = arguments
                .get("namespace")
                .and_then(|v| v.as_str())
                .map(String::from);
            let results = state
                .search_resources(query, namespace)
                .await
                .map_err(|e| e.to_string())?;
            let summarized: Vec<serde_json::Value> = results
                .iter()
                .take(50)
                .map(|r| {
                    let name = r
                        .pointer("/metadata/name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let ns = r
                        .pointer("/metadata/namespace")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let kind = r.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    json!({ "name": name, "namespace": ns, "kind": kind })
                })
                .collect();
            Ok(json!({ "results": summarized }))
        }
        "list_namespaces" => {
            let namespaces = state.list_namespaces().await.map_err(|e| e.to_string())?;
            Ok(json!({ "namespaces": namespaces }))
        }
        _ => Err(format!("Unknown tool: {tool_name}")),
    }
}

fn build_chat_system_prompt(context_name: Option<&str>, namespace: Option<&str>) -> String {
    let ctx_info = context_name
        .map(|c| format!("Current cluster context: `{c}`\n"))
        .unwrap_or_default();
    let ns_info = namespace
        .filter(|ns| !ns.is_empty() && *ns != "*")
        .map(|ns| format!("Current namespace: `{ns}`\n"))
        .unwrap_or_else(|| "Namespace: all namespaces\n".to_string());

    format!(
        "You are Kore AI, a Kubernetes cluster assistant embedded in the Kore desktop IDE. \
         You help users understand and troubleshoot their Kubernetes clusters.\n\n\
         {ctx_info}{ns_info}\n\
         Guidelines:\n\
         - Use the available tools to query live cluster data before answering.\n\
         - Be concise but thorough. Start with a brief summary, then details.\n\
         - Reference specific resource names and namespaces.\n\
         - For problems, list concrete actionable suggestions.\n\
         - If the cluster looks healthy, say so and mention what you checked.\n\
         - Format responses in Markdown with headers for readability.\n\
         - When listing resources, use tables or bullet points.\n\
         - Always call tools rather than guessing — use real data."
    )
}

/// Known tool names for validating text-parsed tool calls from Ollama.
const KNOWN_TOOL_NAMES: &[&str] = &[
    "list_resources",
    "get_cluster_health",
    "describe_resource",
    "get_resource_events",
    "get_pod_logs",
    "get_pod_metrics",
    "search_resources",
    "list_namespaces",
];

/// Try to parse tool calls from plain text content.
///
/// Some Ollama models don't use the structured tool_calls field and instead
/// output JSON like `{"name": "list_resources", "parameters": {"kind": "pods"}}`
/// directly in the message content.
fn parse_tool_calls_from_text(text: &str) -> Vec<ToolCall> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![];
    }

    let mut calls = Vec::new();

    // Try to find JSON objects in the text that look like tool calls.
    // We scan for `{` and try to parse balanced JSON objects.
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Try to find the matching closing brace
            let mut depth = 0;
            let mut j = i;
            while j < bytes.len() {
                match bytes[j] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            // Try parsing this substring as JSON
                            let candidate = &trimmed[i..=j];
                            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(candidate) {
                                if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                                    if KNOWN_TOOL_NAMES.contains(&name) {
                                        let arguments = obj
                                            .get("parameters")
                                            .or_else(|| obj.get("arguments"))
                                            .cloned()
                                            .unwrap_or(json!({}));
                                        calls.push(ToolCall {
                                            id: format!("ollama-text-{}", calls.len()),
                                            name: name.to_string(),
                                            arguments,
                                        });
                                    }
                                }
                            }
                            i = j + 1;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if depth != 0 {
                // Unbalanced braces, skip this `{`
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    calls
}

/// Extract tool calls from a provider's non-streaming response body.
fn extract_tool_calls(provider: &AIProvider, body: &serde_json::Value) -> Vec<ToolCall> {
    match provider {
        AIProvider::OpenAI => {
            // OpenAI: choices[0].message.tool_calls[{id, function: {name, arguments}}]
            body.pointer("/choices/0/message/tool_calls")
                .and_then(|v| v.as_array())
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|tc| {
                            let id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let name = tc
                                .pointer("/function/name")
                                .and_then(|v| v.as_str())?
                                .to_string();
                            let args_str = tc
                                .pointer("/function/arguments")
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}");
                            let arguments = serde_json::from_str(args_str).unwrap_or(json!({}));
                            Some(ToolCall {
                                id,
                                name,
                                arguments,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        AIProvider::Ollama => {
            // Ollama: message.tool_calls[{function: {name, arguments}}]
            // arguments is already an object (not a JSON string)
            let structured: Vec<ToolCall> = body
                .pointer("/message/tool_calls")
                .and_then(|v| v.as_array())
                .map(|calls| {
                    calls
                        .iter()
                        .enumerate()
                        .filter_map(|(i, tc)| {
                            let name = tc
                                .pointer("/function/name")
                                .and_then(|v| v.as_str())?
                                .to_string();
                            let arguments = tc
                                .pointer("/function/arguments")
                                .cloned()
                                .unwrap_or(json!({}));
                            Some(ToolCall {
                                id: format!("ollama-{i}"),
                                name,
                                arguments,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            if !structured.is_empty() {
                return structured;
            }

            // Fallback: some Ollama models emit tool calls as JSON text in message.content
            // instead of using the structured tool_calls field.
            let text = body
                .pointer("/message/content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            parse_tool_calls_from_text(text)
        }
        AIProvider::Anthropic => {
            // Anthropic: content[{type: "tool_use", id, name, input}]
            body.get("content")
                .and_then(|v| v.as_array())
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| {
                            if b.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
                                return None;
                            }
                            let id = b
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let name = b.get("name").and_then(|v| v.as_str())?.to_string();
                            let arguments = b.get("input").cloned().unwrap_or(json!({}));
                            Some(ToolCall {
                                id,
                                name,
                                arguments,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        AIProvider::ClaudeCli | AIProvider::CursorAgent => vec![],
    }
}

/// Extract text content from a provider's non-streaming response.
fn extract_text_content(provider: &AIProvider, body: &serde_json::Value) -> String {
    match provider {
        AIProvider::OpenAI => body
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        AIProvider::Ollama => body
            .pointer("/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        AIProvider::Anthropic => body
            .get("content")
            .and_then(|v| v.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                            b.get("text").and_then(|v| v.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default(),
        AIProvider::ClaudeCli | AIProvider::CursorAgent => String::new(),
    }
}

/// Make a non-streaming API call to the AI provider with tool definitions.
async fn make_chat_api_call(
    http: &HttpClient,
    config: &AIConfig,
    system_prompt: &str,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
) -> std::result::Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    // Build full message array with system prompt for OpenAI/Ollama
    let full_messages = {
        let mut msgs = vec![json!({"role": "system", "content": system_prompt})];
        msgs.extend_from_slice(messages);
        msgs
    };

    let response = match config.provider {
        AIProvider::OpenAI => {
            let api_key = config.api_key.as_deref().ok_or("OpenAI API key required")?;
            let mut req_body = json!({
                "model": &config.model,
                "messages": full_messages,
            });
            if !tools.is_empty() {
                req_body["tools"] = json!(tools);
            }
            http.post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Content-Type", "application/json")
                .json(&req_body)
                .send()
                .await?
        }
        AIProvider::Anthropic => {
            let api_key = config
                .api_key
                .as_deref()
                .ok_or("Anthropic API key required")?;
            let mut req_body = json!({
                "model": &config.model,
                "system": system_prompt,
                "messages": messages,
                "max_tokens": 4096,
            });
            if !tools.is_empty() {
                req_body["tools"] = json!(tools);
            }
            http.post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&req_body)
                .send()
                .await?
        }
        AIProvider::Ollama => {
            let base = config
                .base_url
                .as_deref()
                .unwrap_or("http://localhost:11434");
            let mut req_body = json!({
                "model": &config.model,
                "messages": full_messages,
                "stream": false,
            });
            if !tools.is_empty() {
                req_body["tools"] = json!(tools);
            }
            http.post(format!("{base}/api/chat"))
                .header("Content-Type", "application/json")
                .json(&req_body)
                .send()
                .await?
        }
        AIProvider::ClaudeCli => {
            return Err("Claude CLI does not use HTTP API".into());
        }
        AIProvider::CursorAgent => {
            return Err("Cursor Agent does not use HTTP API".into());
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("AI provider returned HTTP {status}: {body}").into());
    }

    let body: serde_json::Value = response.json().await?;
    Ok(body)
}

/// The core tool-calling chat loop.
async fn run_chat_loop(
    state: &K8sState,
    app: &AppHandle,
    event_name: &str,
    config: &AIConfig,
    system_prompt: &str,
    frontend_messages: &[ChatMessage],
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let http = HttpClient::new();
    let tools = build_tool_definitions(&config.provider);

    // Build initial message array from frontend messages
    let mut messages: Vec<serde_json::Value> = frontend_messages
        .iter()
        .map(|m| {
            json!({
                "role": &m.role,
                "content": &m.content
            })
        })
        .collect();

    let max_rounds = 10;
    for round in 0..max_rounds {
        info!(round, provider = ?config.provider, "AI chat loop round");
        let body = make_chat_api_call(&http, config, system_prompt, &messages, &tools).await?;

        let tool_calls = extract_tool_calls(&config.provider, &body);
        let text = extract_text_content(&config.provider, &body);

        info!(
            round,
            tool_calls = tool_calls.len(),
            has_text = !text.is_empty(),
            text_len = text.len(),
            "AI chat response parsed"
        );

        if tool_calls.is_empty() {
            // No tool calls — stream the final text back
            if !text.is_empty() {
                // Send in chunks to simulate streaming for smoother UX
                for chunk in text.as_bytes().chunks(80) {
                    let chunk_str = String::from_utf8_lossy(chunk).to_string();
                    app.emit(event_name, &AIStreamEvent::Chunk { content: chunk_str })?;
                    tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;
                }
            }
            app.emit(event_name, &AIStreamEvent::Done)?;
            return Ok(());
        }

        // Check if Ollama tool calls came from text (not the structured field).
        // Models that output tool calls as plain text don't understand the tool
        // protocol, so we handle them differently.
        let ollama_text_tools = matches!(config.provider, AIProvider::Ollama)
            && body
                .pointer("/message/tool_calls")
                .and_then(|v| v.as_array())
                .is_none_or(|a| a.is_empty());

        // Execute each tool call and collect results
        let mut tool_results: Vec<(String, String)> = Vec::new();
        for tc in &tool_calls {
            let status_msg = format!("Querying {}...", tc.name.replace('_', " "));
            app.emit(
                event_name,
                &AIStreamEvent::Status {
                    message: status_msg,
                },
            )?;

            let result = match execute_tool(state, &tc.name, &tc.arguments).await {
                Ok(val) => serde_json::to_string(&val).unwrap_or_else(|_| "{}".to_string()),
                Err(e) => format!("{{\"error\": \"{}\"}}", e.replace('"', "'")),
            };

            tool_results.push((tc.name.clone(), result));
        }

        if ollama_text_tools {
            // The model wrote tool calls as text — it doesn't support the tool protocol.
            // Inject results as a user message with context and make a final call without tools.
            messages.push(json!({"role": "assistant", "content": "Let me look that up."}));

            let mut context_parts: Vec<String> = Vec::new();
            for (name, result) in &tool_results {
                // Truncate very large results to keep context manageable
                let truncated = if result.len() > 8_000 {
                    format!("{}...[truncated]", &result[..8_000])
                } else {
                    result.clone()
                };
                context_parts.push(format!(
                    "## Result of {}\n```json\n{}\n```",
                    name.replace('_', " "),
                    truncated
                ));
            }
            messages.push(json!({
                "role": "user",
                "content": format!(
                    "Here is the live cluster data you requested. \
                     Please analyze it and answer my original question.\n\n{}",
                    context_parts.join("\n\n")
                )
            }));

            // Make a final call WITHOUT tools to get a natural language answer
            let final_body =
                make_chat_api_call(&http, config, system_prompt, &messages, &[]).await?;
            let final_text = extract_text_content(&config.provider, &final_body);

            if !final_text.is_empty() {
                for chunk in final_text.as_bytes().chunks(80) {
                    let chunk_str = String::from_utf8_lossy(chunk).to_string();
                    app.emit(event_name, &AIStreamEvent::Chunk { content: chunk_str })?;
                    tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;
                }
            }
            app.emit(event_name, &AIStreamEvent::Done)?;
            return Ok(());
        }

        // Standard path: model supports structured tool calling.
        // Add the assistant's response with tool calls to messages.
        match config.provider {
            AIProvider::OpenAI => {
                let assistant_msg = body
                    .pointer("/choices/0/message")
                    .cloned()
                    .unwrap_or(json!({"role": "assistant", "content": null}));
                messages.push(assistant_msg);
            }
            AIProvider::Ollama => {
                let assistant_msg = body
                    .get("message")
                    .cloned()
                    .unwrap_or(json!({"role": "assistant", "content": ""}));
                messages.push(assistant_msg);
            }
            AIProvider::Anthropic => {
                messages.push(json!({
                    "role": "assistant",
                    "content": body.get("content").cloned().unwrap_or(json!([]))
                }));
            }
            AIProvider::ClaudeCli | AIProvider::CursorAgent => {}
        }

        // Add tool results to messages
        for (i, (_name, result)) in tool_results.iter().enumerate() {
            match config.provider {
                AIProvider::OpenAI => {
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": &tool_calls[i].id,
                        "content": result
                    }));
                }
                AIProvider::Ollama => {
                    messages.push(json!({
                        "role": "tool",
                        "content": result
                    }));
                }
                AIProvider::Anthropic => {
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": &tool_calls[i].id,
                            "content": result
                        }]
                    }));
                }
                AIProvider::ClaudeCli | AIProvider::CursorAgent => {}
            }
        }
    }

    // Max rounds exceeded — send what we have
    app.emit(
        event_name,
        &AIStreamEvent::Chunk {
            content: "\n\n*[Reached maximum tool call rounds]*".to_string(),
        },
    )?;
    app.emit(event_name, &AIStreamEvent::Done)?;
    Ok(())
}

/// Claude CLI chat fallback — pre-gather cluster health context.
async fn run_chat_claude_cli(
    state: &K8sState,
    app: &AppHandle,
    event_name: &str,
    config: &AIConfig,
    system_prompt: &str,
    frontend_messages: &[ChatMessage],
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Pre-gather cluster health context
    let health_context = match state.get_cluster_health().await {
        Ok(health) => {
            app.emit(
                event_name,
                &AIStreamEvent::Status {
                    message: "Thinking...".to_string(),
                },
            )?;
            serde_json::to_string_pretty(&health).unwrap_or_default()
        }
        Err(e) => format!("Failed to get cluster health: {e}"),
    };

    // Build the full prompt with context
    let user_messages: Vec<String> = frontend_messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.clone())
        .collect();
    let last_user_msg = user_messages.last().cloned().unwrap_or_default();

    let enriched_prompt = format!(
        "{system_prompt}\n\n## Current Cluster State\n```json\n{health_context}\n```\n\n\
         ## User Question\n{last_user_msg}"
    );

    stream_claude_cli_response(app, event_name, config, "", &enriched_prompt).await
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

    /// AI Chat — standalone cluster-wide chat with tool calling.
    pub async fn ai_chat(
        &self,
        app: AppHandle,
        config: AIConfig,
        request: AIChatRequest,
    ) -> Result<()> {
        let session_id = request.session_id.clone();
        let event_name = format!("ai-chat://{session_id}");

        let context_name = self.current_context_name().await;
        let system_prompt =
            build_chat_system_prompt(context_name.as_deref(), request.namespace.as_deref());

        info!(
            session_id = %session_id,
            provider = ?config.provider,
            messages = request.messages.len(),
            "Starting AI chat"
        );

        let state = self.clone();
        let messages = request.messages.clone();

        tauri::async_runtime::spawn(async move {
            let result = if matches!(config.provider, AIProvider::ClaudeCli) {
                run_chat_claude_cli(
                    &state,
                    &app,
                    &event_name,
                    &config,
                    &system_prompt,
                    &messages,
                )
                .await
            } else if matches!(config.provider, AIProvider::CursorAgent) {
                run_chat_cursor_agent(
                    &state,
                    &app,
                    &event_name,
                    &config,
                    &system_prompt,
                    &messages,
                )
                .await
            } else {
                run_chat_loop(
                    &state,
                    &app,
                    &event_name,
                    &config,
                    &system_prompt,
                    &messages,
                )
                .await
            };

            if let Err(e) = result {
                error!(error = %e, "AI chat failed");
                let _ = app.emit(
                    &event_name,
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

    /// Check if Ollama is reachable at the given (or default) base URL.
    pub async fn ollama_available(base_url: Option<&str>) -> Result<bool> {
        let http = HttpClient::new();
        let base = base_url.unwrap_or("http://localhost:11434");
        let resp = http
            .get(format!("{base}/api/tags"))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await;
        match resp {
            Ok(r) => Ok(r.status().is_success()),
            Err(_) => Ok(false),
        }
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

    /// Check if the `agent` (Cursor Agent) CLI is available in PATH.
    pub async fn cursor_agent_cli_available() -> Result<bool> {
        let output = tokio::process::Command::new("agent")
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

    /// List available Cursor Agent models by parsing `agent models` output.
    pub async fn list_cursor_agent_models() -> Result<Vec<String>> {
        if !Self::cursor_agent_cli_available().await? {
            return Err(K8sError::Validation(
                "Cursor Agent CLI is not installed or not in PATH".into(),
            ));
        }

        let output = tokio::process::Command::new("agent")
            .arg("models")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| K8sError::Validation(format!("Failed to run agent models: {e}")))?;

        if !output.status.success() {
            return Err(K8sError::Validation("agent models command failed".into()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let clean = strip_ansi_codes(&stdout);

        let mut models = Vec::new();
        for line in clean.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || !trimmed.contains(" - ") {
                continue;
            }
            if let Some(id) = trimmed.split(" - ").next() {
                let id = id.trim();
                if !id.is_empty() && !id.contains(' ') && id != "Available" && id != "Tip:" {
                    models.push(id.to_string());
                }
            }
        }

        Ok(models)
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
            AIProvider::CursorAgent => Self::cursor_agent_cli_available().await?,
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
    // CLI-based providers use a subprocess instead of HTTP
    if matches!(config.provider, AIProvider::ClaudeCli) {
        return stream_claude_cli_response(app, event_name, config, system_prompt, user_message)
            .await;
    }
    if matches!(config.provider, AIProvider::CursorAgent) {
        return stream_cursor_agent_response(app, event_name, config, system_prompt, user_message)
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
        AIProvider::ClaudeCli | AIProvider::CursorAgent => {
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
    let mut total_bytes: usize = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        let text = String::from_utf8_lossy(&chunk);
        total_bytes += chunk.len();

        if total_bytes > MAX_AI_RESPONSE_BYTES {
            app.emit(
                event_name,
                &AIStreamEvent::Error {
                    message: format!(
                        "AI response exceeded maximum size ({MAX_AI_RESPONSE_BYTES} bytes)"
                    ),
                },
            )?;
            break;
        }

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
                AIProvider::ClaudeCli | AIProvider::CursorAgent => unreachable!(),
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
    let mut total_bytes: usize = 0;

    while let Some(line) = reader.next_line().await? {
        total_bytes += line.len();

        if total_bytes > MAX_AI_RESPONSE_BYTES {
            app.emit(
                event_name,
                &AIStreamEvent::Error {
                    message: format!(
                        "AI response exceeded maximum size ({MAX_AI_RESPONSE_BYTES} bytes)"
                    ),
                },
            )?;
            let _ = child.kill().await;
            break;
        }

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
                if !already_received_chunks {
                    if let Some(text) = val.get("result").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            return SSEContent::Text(text.to_string());
                        }
                    }
                }
                return SSEContent::Done;
            }

            if top_type == "content_block_delta" {
                let text = val
                    .pointer("/delta/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return SSEContent::Text(text);
            }

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

// ── Cursor Agent CLI streaming ───────────────────────────────────────────

/// Cursor Agent chat — pre-gather cluster health context, then stream via `agent` subprocess.
async fn run_chat_cursor_agent(
    state: &K8sState,
    app: &AppHandle,
    event_name: &str,
    config: &AIConfig,
    system_prompt: &str,
    frontend_messages: &[ChatMessage],
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let health_context = match state.get_cluster_health().await {
        Ok(health) => {
            app.emit(
                event_name,
                &AIStreamEvent::Status {
                    message: "Thinking...".to_string(),
                },
            )?;
            serde_json::to_string_pretty(&health).unwrap_or_default()
        }
        Err(e) => format!("Failed to get cluster health: {e}"),
    };

    let user_messages: Vec<String> = frontend_messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.clone())
        .collect();
    let last_user_msg = user_messages.last().cloned().unwrap_or_default();

    let enriched_prompt = format!(
        "{system_prompt}\n\n## Current Cluster State\n```json\n{health_context}\n```\n\n\
         ## User Question\n{last_user_msg}"
    );

    stream_cursor_agent_response(app, event_name, config, "", &enriched_prompt).await
}

/// Stream AI response via the `agent` (Cursor Agent) CLI subprocess.
async fn stream_cursor_agent_response(
    app: &AppHandle,
    event_name: &str,
    config: &AIConfig,
    system_prompt: &str,
    user_message: &str,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let full_prompt = format!("{system_prompt}\n\n{user_message}");

    let mut cmd = tokio::process::Command::new("agent");
    cmd.arg("-p")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--stream-partial-output")
        .arg("--model")
        .arg(&config.model)
        .arg("--mode")
        .arg("ask")
        .arg("--trust")
        .arg("--workspace")
        .arg("/tmp")
        .arg(&full_prompt)
        .env_remove("CURSOR_AGENT_ENTRYPOINT")
        .env_remove("CURSORAGENT")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn agent CLI: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture agent CLI stdout")?;

    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to capture agent CLI stderr")?;

    let stderr_handle = tokio::spawn(async move {
        let mut stderr_reader = BufReader::new(stderr);
        let mut stderr_buf = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr_reader, &mut stderr_buf).await;
        stderr_buf
    });

    let mut reader = BufReader::new(stdout).lines();
    let mut received_chunks = false;
    let mut total_bytes: usize = 0;

    while let Some(line) = reader.next_line().await? {
        total_bytes += line.len();

        if total_bytes > MAX_AI_RESPONSE_BYTES {
            app.emit(
                event_name,
                &AIStreamEvent::Error {
                    message: format!(
                        "AI response exceeded maximum size ({MAX_AI_RESPONSE_BYTES} bytes)"
                    ),
                },
            )?;
            let _ = child.kill().await;
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match parse_cursor_agent_stream_line(trimmed, received_chunks) {
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

    let status = child.wait().await?;
    let stderr_output = stderr_handle.await.unwrap_or_default();

    if !status.success() {
        let detail = if stderr_output.trim().is_empty() {
            format!("Cursor Agent exited with status {status}")
        } else {
            let last_lines: String = stderr_output
                .lines()
                .rev()
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            format!("Cursor Agent error: {last_lines}")
        };
        app.emit(
            event_name,
            &AIStreamEvent::Error {
                message: detail.clone(),
            },
        )?;
        return Err(detail.into());
    }

    app.emit(event_name, &AIStreamEvent::Done)?;
    Ok(())
}

/// Parse a Cursor Agent stream-json line (JSONL format).
///
/// The Cursor Agent CLI uses the same streaming format conventions:
/// - `{"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}`
/// - `{"type":"result","result":"full text",...}`
/// - `{"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}`
///
/// Falls back to treating any JSON with a top-level "text" or "content" field as text.
fn parse_cursor_agent_stream_line(line: &str, already_received_chunks: bool) -> SSEContent {
    let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(line);
    match parsed {
        Ok(val) => {
            let top_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

            if top_type == "result" {
                if !already_received_chunks {
                    if let Some(text) = val.get("result").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            return SSEContent::Text(text.to_string());
                        }
                    }
                }
                return SSEContent::Done;
            }

            if top_type == "content_block_delta" {
                let text = val
                    .pointer("/delta/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return SSEContent::Text(text);
            }

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

            // Fallback: plain text field
            if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    return SSEContent::Text(text.to_string());
                }
            }
            if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    return SSEContent::Text(content.to_string());
                }
            }

            SSEContent::Skip
        }
        Err(_) => SSEContent::Skip,
    }
}
