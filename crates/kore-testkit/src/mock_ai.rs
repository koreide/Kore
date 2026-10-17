//! Deterministic fake LLM provider for AI scenarios.
//!
//! Speaks enough of three wire protocols for Kore's AI features:
//! - Ollama:    `GET /api/tags`, `POST /api/chat` (NDJSON stream or single JSON, tool calls)
//! - OpenAI:    `GET /v1/models`, `POST /v1/chat/completions` (SSE stream or JSON)
//! - Anthropic: `POST /v1/messages` (SSE stream or JSON)
//!
//! Port+1 answers every request with HTTP 503 (for Test Connection error reporting).
//!
//! Triggers in the last user message:
//! - contains "boom"   → HTTP 500 with body `boom`
//! - contains "health" (and no tool result yet) → a `get_cluster_health` tool call
//! - otherwise         → a fixed markdown answer (headings, bold, table, inline + block code)

use anyhow::Result;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const MARKDOWN_ANSWER: &str = "## Summary\n\nCluster **looks** fine.\n\n| Pod | Status |\n|---|---|\n| web | Running |\n\n`inline` and\n```\ncode block\n```";
const STREAM_CHUNKS: [&str; 3] = ["Diag", "nosis ", "OK — KORE-MOCK-STREAM"];

/// A running mock server (background thread with its own runtime). Lives until
/// the process exits; tests just start one each.
pub struct MockAi {
    /// `http://127.0.0.1:<port>`: speaks Ollama, OpenAI and Anthropic.
    pub url: String,
    /// Answers every request with HTTP 503.
    pub broken_url: String,
}

impl MockAi {
    /// Start on free ports (for tests).
    pub fn start() -> Result<MockAi> {
        Self::spawn(0, 0)
    }

    fn spawn(port: u16, broken_port: u16) -> Result<MockAi> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("mock-ai".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(anyhow::anyhow!(e)));
                    return;
                }
            };
            rt.block_on(async move {
                let bound = async {
                    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
                    let broken = TcpListener::bind(("127.0.0.1", broken_port)).await?;
                    anyhow::Ok((listener, broken))
                }
                .await;
                let (listener, broken) = match bound {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                };
                let url = format!("http://127.0.0.1:{}", listener.local_addr().map(|a| a.port()).unwrap_or(0));
                let broken_url = format!("http://127.0.0.1:{}", broken.local_addr().map(|a| a.port()).unwrap_or(0));
                let _ = tx.send(Ok(MockAi { url, broken_url }));
                tokio::spawn(async move {
                    while let Ok((mut sock, _)) = broken.accept().await {
                        tokio::spawn(async move {
                            if read_request(&mut sock).await.is_ok() {
                                let _ = respond(&mut sock, "503 Service Unavailable", "text/plain", "mock unavailable").await;
                            }
                        });
                    }
                });
                while let Ok((sock, _)) = listener.accept().await {
                    tokio::spawn(async move {
                        if let Err(e) = handle(sock).await {
                            eprintln!("mock AI: {e:#}");
                        }
                    });
                }
            });
        })?;
        rx.recv()?
    }
}

/// Serve on `port` (and the always-failing endpoint on `port + 1`) until killed:
/// `kore-harness mock-ai` for Layer 2 runs.
pub fn serve(port: u16) -> Result<()> {
    let mock = MockAi::spawn(port, port + 1)?;
    eprintln!("mock AI listening on {} (ollama, openai /v1, anthropic /v1/messages)", mock.url);
    eprintln!("mock AI failing endpoint on {} (HTTP 503 for everything)", mock.broken_url);
    loop {
        std::thread::park();
    }
}

struct Req {
    method: String,
    path: String,
    body: Value,
}

async fn read_request(sock: &mut TcpStream) -> Result<Req> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            anyhow::bail!("connection closed before headers");
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let first: Vec<&str> = lines.next().unwrap_or_default().split_whitespace().collect();
    let content_length = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = &buf[header_end..(header_end + content_length).min(buf.len())];
    Ok(Req {
        method: first.first().unwrap_or(&"").to_string(),
        path: first.get(1).unwrap_or(&"").split('?').next().unwrap_or("").to_string(),
        body: serde_json::from_slice(body).unwrap_or(Value::Null),
    })
}

fn last_user_text(body: &Value) -> String {
    body["messages"]
        .as_array()
        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
        .map(|m| match &m["content"] {
            Value::String(s) => s.clone(),
            // Anthropic content blocks
            Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(" "),
            _ => String::new(),
        })
        .unwrap_or_default()
        .to_lowercase()
}

fn has_tool_result(body: &Value) -> bool {
    body["messages"].as_array().is_some_and(|m| {
        m.iter().any(|m| {
            m["role"] == "tool"
                || m["content"].as_array().is_some_and(|b| b.iter().any(|b| b["type"] == "tool_result"))
        })
    })
}

async fn respond(sock: &mut TcpStream, status: &str, ctype: &str, body: &str) -> Result<()> {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    sock.write_all(resp.as_bytes()).await?;
    Ok(())
}

/// Streams with chunked transfer encoding, pausing between chunks so the UI visibly streams.
async fn stream(sock: &mut TcpStream, ctype: &str, parts: Vec<String>) -> Result<()> {
    sock.write_all(
        format!("HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await?;
    for p in parts {
        sock.write_all(format!("{:x}\r\n{p}\r\n", p.len()).as_bytes()).await?;
        sock.flush().await?;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    sock.write_all(b"0\r\n\r\n").await?;
    Ok(())
}

async fn handle(mut sock: TcpStream) -> Result<()> {
    let req = read_request(&mut sock).await?;
    let text = last_user_text(&req.body);
    let streaming = req.body["stream"].as_bool().unwrap_or(false);
    eprintln!("mock AI: {} {} stream={streaming} user={:?}", req.method, req.path, text.chars().take(60).collect::<String>());

    if text.contains("boom") {
        return respond(&mut sock, "500 Internal Server Error", "text/plain", "boom").await;
    }
    let wants_tool = text.contains("health") && !has_tool_result(&req.body);
    if !streaming && has_tool_result(&req.body) {
        // Keep the app's "Querying <tool>..." status visible long enough for OCR to see it.
        tokio::time::sleep(Duration::from_millis(2000)).await;
    }

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/tags") => {
            respond(&mut sock, "200 OK", "application/json", &json!({"models":[{"name":"mock-model"}]}).to_string()).await
        }
        ("GET", "/v1/models") => {
            respond(&mut sock, "200 OK", "application/json", &json!({"data":[{"id":"mock-model"}]}).to_string()).await
        }
        ("POST", "/api/chat") => {
            if streaming {
                let mut parts: Vec<String> = STREAM_CHUNKS
                    .iter()
                    .map(|c| json!({"message":{"role":"assistant","content":c},"done":false}).to_string() + "\n")
                    .collect();
                parts.push(json!({"done":true}).to_string() + "\n");
                stream(&mut sock, "application/x-ndjson", parts).await
            } else if wants_tool {
                let body = json!({"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"get_cluster_health","arguments":{}}}]},"done":true});
                respond(&mut sock, "200 OK", "application/json", &body.to_string()).await
            } else {
                let body = json!({"message":{"role":"assistant","content":MARKDOWN_ANSWER},"done":true});
                respond(&mut sock, "200 OK", "application/json", &body.to_string()).await
            }
        }
        ("POST", "/v1/chat/completions") => {
            if streaming {
                let mut parts: Vec<String> = STREAM_CHUNKS
                    .iter()
                    .map(|c| format!("data: {}\n\n", json!({"choices":[{"delta":{"content":c}}]})))
                    .collect();
                parts.push("data: [DONE]\n\n".into());
                stream(&mut sock, "text/event-stream", parts).await
            } else if wants_tool {
                let body = json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_cluster_health","arguments":"{}"}}]},"finish_reason":"tool_calls"}]});
                respond(&mut sock, "200 OK", "application/json", &body.to_string()).await
            } else {
                let body = json!({"choices":[{"message":{"role":"assistant","content":MARKDOWN_ANSWER},"finish_reason":"stop"}]});
                respond(&mut sock, "200 OK", "application/json", &body.to_string()).await
            }
        }
        ("POST", "/v1/messages") => {
            if streaming {
                let mut parts = vec![format!("event: message_start\ndata: {}\n\n", json!({"type":"message_start"}))];
                parts.extend(STREAM_CHUNKS.iter().map(|c| {
                    format!(
                        "event: content_block_delta\ndata: {}\n\n",
                        json!({"type":"content_block_delta","delta":{"type":"text_delta","text":c}})
                    )
                }));
                parts.push(format!("event: message_stop\ndata: {}\n\n", json!({"type":"message_stop"})));
                stream(&mut sock, "text/event-stream", parts).await
            } else if wants_tool {
                let body = json!({"content":[{"type":"tool_use","id":"toolu_1","name":"get_cluster_health","input":{}}],"stop_reason":"tool_use"});
                respond(&mut sock, "200 OK", "application/json", &body.to_string()).await
            } else {
                let body = json!({"content":[{"type":"text","text":MARKDOWN_ANSWER}],"stop_reason":"end_turn"});
                respond(&mut sock, "200 OK", "application/json", &body.to_string()).await
            }
        }
        _ => respond(&mut sock, "404 Not Found", "text/plain", "not found").await,
    }
}
