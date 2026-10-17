//! AI provider plumbing against a local mock HTTP server (no cluster, no
//! real provider): Deliberate Fix 13 (base-URL env overrides for
//! OpenAI/Anthropic, Ollama probe honours the configured base URL) and
//! AI subscriptions consumed from a non-tokio executor.

use kore_core::{
    AIChatRequest, AIConfig, AIProvider, AIStreamEvent, ChatMessage, Core, CoreConfig,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Minimal blocking HTTP/1.1 mock. Records request paths.
fn start_mock() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let seen = seen2.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    return;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                seen.lock().unwrap().push(format!("{method} {path}"));
                let (status, resp) = match (method.as_str(), path.as_str()) {
                    ("GET", "/api/tags") => ("200 OK", r#"{"models":[{"name":"mock-model:latest"}]}"#.to_string()),
                    ("GET", "/v1/models") => ("200 OK", r#"{"data":[]}"#.to_string()),
                    ("POST", "/v1/chat/completions") => (
                        "200 OK",
                        r#"{"choices":[{"message":{"role":"assistant","content":"OPENAI-MOCK-ANSWER"}}]}"#.to_string(),
                    ),
                    ("POST", "/v1/messages") => (
                        "200 OK",
                        r#"{"content":[{"type":"text","text":"ANTHROPIC-MOCK-ANSWER"}],"stop_reason":"end_turn"}"#.to_string(),
                    ),
                    _ => ("404 Not Found", "{}".to_string()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                    resp.len()
                );
            });
        }
    });
    (port, seen)
}

fn offline_core(dir: &std::path::Path) -> Core {
    Core::with_config(CoreConfig {
        kubeconfig_paths: vec![dir.join("no-kubeconfig")],
        data_dir: Some(dir.join("data")),
        favorites_dir: Some(dir.join("kore")),
        ..Default::default()
    })
    .unwrap()
}

async fn collect(
    core: &Core,
    sub: &mut kore_core::Subscription<AIStreamEvent>,
) -> Vec<AIStreamEvent> {
    let mut out = Vec::new();
    loop {
        let timer = core.spawn(async { tokio::time::sleep(Duration::from_secs(20)).await });
        match futures::future::select(Box::pin(sub.recv()), timer).await {
            futures::future::Either::Left((Some(ev), _)) => {
                let done = matches!(ev, AIStreamEvent::Done | AIStreamEvent::Error { .. });
                out.push(ev);
                if done {
                    return out;
                }
            }
            _ => return out,
        }
    }
}

fn text_of(evs: &[AIStreamEvent]) -> String {
    evs.iter()
        .filter_map(|e| match e {
            AIStreamEvent::Chunk { content } => Some(content.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn ai_providers_use_overridable_base_urls() {
    let (port, seen) = start_mock();
    let base = format!("http://127.0.0.1:{port}");
    // Single test function owns these process-wide env vars.
    std::env::set_var("KORE_OPENAI_BASE_URL", format!("{base}/"));
    std::env::set_var("KORE_ANTHROPIC_BASE_URL", format!("{base}/v1"));

    let dir = tempfile::tempdir().unwrap();
    let core = offline_core(dir.path());
    futures::executor::block_on(async {
        // Ollama probe honours the configured base URL
        let avail = core
            .check_providers_availability(Some(base.clone()))
            .await
            .unwrap();
        assert_eq!(avail.get("ollama"), Some(&true));
        let closed = core
            .check_providers_availability(Some("http://127.0.0.1:1".into()))
            .await
            .unwrap();
        assert_eq!(closed.get("ollama"), Some(&false));
        assert_eq!(
            core.list_ollama_models(Some(base.clone())).await.unwrap(),
            vec!["mock-model:latest"]
        );

        let openai = AIConfig {
            provider: AIProvider::OpenAI,
            api_key: Some("sk-test".into()),
            model: "gpt-test".into(),
            base_url: None,
        };
        assert!(core.ai_test_connection(openai.clone()).await.unwrap());

        let req = AIChatRequest {
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
            session_id: "s1".into(),
            namespace: None,
        };
        let mut sub = core.ai_chat(openai, req.clone()).await.unwrap();
        let evs = collect(&core, &mut sub).await;
        assert_eq!(evs.last(), Some(&AIStreamEvent::Done), "{evs:?}");
        assert_eq!(text_of(&evs), "OPENAI-MOCK-ANSWER");

        let anthropic = AIConfig {
            provider: AIProvider::Anthropic,
            api_key: Some("sk-ant-test".into()),
            model: "claude-test".into(),
            base_url: None,
        };
        let mut sub = core.ai_chat(anthropic, req).await.unwrap();
        let evs = collect(&core, &mut sub).await;
        assert_eq!(text_of(&evs), "ANTHROPIC-MOCK-ANSWER", "{evs:?}");

        // Payload JSON shape is stable
        assert_eq!(
            serde_json::to_value(AIStreamEvent::Chunk {
                content: "x".into()
            })
            .unwrap(),
            serde_json::json!({"type": "chunk", "content": "x"})
        );
        assert_eq!(
            serde_json::to_value(AIStreamEvent::Done).unwrap(),
            serde_json::json!({"type": "done"})
        );
    });

    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.contains(&"POST /v1/chat/completions".to_string()),
        "{seen:?}"
    );
    assert!(seen.contains(&"POST /v1/messages".to_string()), "{seen:?}");
    assert!(seen.contains(&"GET /v1/models".to_string()), "{seen:?}");
}
