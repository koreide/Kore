//! `kore-harness mcp`: an MCP server (JSON-RPC 2.0 over stdio) for driving the app
//! through its Layer 2 control socket while developing. It runs the real app in
//! the background against the harness kind clusters, sandboxed in
//! `harness/runs/mcp/`, and never touches OS input.
//!
//! Tools: launch, quit, snapshot, click, hover, type, key, scroll, action, wait,
//! screenshot (returns the image), text (OCR lines with coordinates).

use crate::driver::{socket_path, App, Layer};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use kore_automation::{id_matches, Condition, Request};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const PROTOCOL_VERSION: &str = "2024-11-05";

struct Server {
    root: PathBuf,
    binary: Option<PathBuf>,
    app: Option<App>,
}

pub fn serve(root: &Path, binary: Option<PathBuf>) -> Result<()> {
    let mut server = Server { root: root.to_path_buf(), binary, app: None };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(&mut stdout, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}))?;
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned() else { continue }; // notification
        let method = msg["method"].as_str().unwrap_or_default();
        let reply = match server.handle(method, &msg["params"]) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": format!("{e:#}")}}),
        };
        write_msg(&mut stdout, &reply)?;
    }
    if let Some(app) = server.app.take() {
        let _ = app.quit();
    }
    Ok(())
}

fn write_msg(out: &mut impl Write, v: &Value) -> Result<()> {
    let mut s = serde_json::to_string(v)?;
    s.push('\n');
    out.write_all(s.as_bytes())?;
    out.flush()?;
    Ok(())
}

fn text(s: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": s.into()}]})
}

fn tool_error(s: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": s.into()}], "isError": true})
}

impl Server {
    fn handle(&mut self, method: &str, params: &Value) -> Result<Value> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "kore-harness", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Drives the Kore desktop app (Layer 2: in-app input, background window, no OS input). \
                    Call `launch` first. Target elements by automation id (see `snapshot`); ids ending in `*` match by prefix.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(match self.call(name, &args) {
                    Ok(v) => v,
                    Err(e) => tool_error(format!("{e:#}")),
                })
            }
            other => bail!("method not found: {other}"),
        }
    }

    fn app(&mut self) -> Result<&mut App> {
        self.app.as_mut().ok_or_else(|| anyhow!("app not running: call `launch` first"))
    }

    fn call(&mut self, name: &str, a: &Value) -> Result<Value> {
        match name {
            "launch" => {
                if let Some(app) = self.app.take() {
                    let _ = app.quit();
                }
                let sandbox = self.root.join("harness/runs/mcp");
                if a["fresh"].as_bool().unwrap_or(false) {
                    let _ = std::fs::remove_dir_all(sandbox.join("home"));
                }
                let mut cfg = crate::launch_config(
                    &self.root,
                    self.binary.clone(),
                    sandbox.join("home"),
                    Layer::App,
                    socket_path("mcp"),
                    sandbox.join("app.log"),
                )?;
                if let Some(env) = a["env"].as_object() {
                    cfg.extra_env = env.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect();
                }
                let app = App::launch(&cfg, &sandbox.join("scratch"))?;
                let msg = format!("launched pid {} (log {})", app.pid, sandbox.join("app.log").display());
                self.app = Some(app);
                Ok(text(msg))
            }
            "quit" => {
                if let Some(app) = self.app.take() {
                    app.quit()?;
                }
                Ok(text("quit"))
            }
            "snapshot" => {
                let snap = self.app()?.automation().snapshot()?;
                let filter = a["filter"].as_str();
                let elements: Vec<Value> = snap
                    .elements
                    .iter()
                    .filter(|e| filter.is_none_or(|f| id_matches(f, &e.id)))
                    .map(|e| {
                        let mut v = json!({"id": e.id, "role": e.role, "label": e.label,
                            "bounds": [e.bounds.x.round(), e.bounds.y.round(), e.bounds.w.round(), e.bounds.h.round()]});
                        if e.selected {
                            v["selected"] = json!(true);
                        }
                        if e.disabled {
                            v["disabled"] = json!(true);
                        }
                        v
                    })
                    .collect();
                let out = if a["state_only"].as_bool().unwrap_or(false) {
                    json!({"state": snap.state})
                } else {
                    json!({"state": snap.state, "window": [snap.window.w, snap.window.h], "visible": snap.visible, "elements": elements})
                };
                Ok(text(serde_json::to_string_pretty(&out)?))
            }
            "click" | "hover" => {
                let app = self.app()?;
                let req = match a["id"].as_str() {
                    Some(id) if name == "click" => Request::ClickElement {
                        id: id.into(),
                        count: a["count"].as_u64().unwrap_or(1) as u32,
                        right: a["right"].as_bool().unwrap_or(false),
                        timeout_ms: a["timeout_ms"].as_u64().unwrap_or(5000),
                    },
                    Some(id) => {
                        let snap = app.automation().snapshot()?;
                        let e = snap.elements.iter().find(|e| id_matches(id, &e.id)).ok_or_else(|| anyhow!("no element {id:?}"))?;
                        let (x, y) = e.bounds.center();
                        Request::MouseMove { x, y }
                    }
                    None => {
                        let (x, y) = (num(a, "x")?, num(a, "y")?);
                        if name == "click" {
                            Request::Click { x, y, count: a["count"].as_u64().unwrap_or(1) as u32, right: a["right"].as_bool().unwrap_or(false) }
                        } else {
                            Request::MouseMove { x, y }
                        }
                    }
                };
                app.automation().call(&req)?;
                Ok(text("ok"))
            }
            "type" => {
                let t = a["text"].as_str().context("text")?.to_string();
                self.app()?.automation().call(&Request::Type { text: t })?;
                Ok(text("ok"))
            }
            "key" => {
                let chord = a["chord"].as_str().context("chord")?.to_string();
                self.app()?.automation().call(&Request::Key { chord })?;
                Ok(text("ok"))
            }
            "scroll" => {
                let dy = num(a, "dy")?;
                let req = match a["id"].as_str() {
                    Some(id) => Request::ScrollElement { id: id.into(), dy, timeout_ms: 5000 },
                    None => Request::Scroll { x: num(a, "x")?, y: num(a, "y")?, dy },
                };
                self.app()?.automation().call(&req)?;
                Ok(text("ok"))
            }
            "action" => {
                let name = a["name"].as_str().context("name")?.to_string();
                let args = a.get("args").cloned().unwrap_or(Value::Null);
                let out = self.app()?.automation().call(&Request::Action { name, args })?;
                Ok(text(out.to_string()))
            }
            "wait" => {
                let condition = if let Some(id) = a["gone"].as_str() {
                    Condition::Gone { id: id.into() }
                } else if let Some(id) = a["element"].as_str() {
                    Condition::Element { id: id.into(), label: a["label"].as_str().map(Into::into) }
                } else if let Some(p) = a["pointer"].as_str() {
                    Condition::State { pointer: p.into(), equals: a.get("equals").cloned().unwrap_or(Value::Null) }
                } else {
                    bail!("wait needs `element` (+ optional `label` regex), `gone`, or `pointer` + `equals`");
                };
                let timeout_ms = a["timeout_ms"].as_u64().unwrap_or(10_000);
                self.app()?.automation().call(&Request::Wait { condition, timeout_ms })?;
                Ok(text("ok"))
            }
            "screenshot" => {
                let path = self.root.join("harness/runs/mcp/screenshot.png");
                std::fs::create_dir_all(path.parent().unwrap())?;
                self.app()?.screenshot(&path)?;
                let mut img = image::open(&path)?;
                // Device pixels are 2x; logical size is plenty to look at and half the tokens.
                if !a["full_resolution"].as_bool().unwrap_or(false) && img.width() > 1600 {
                    img = img.resize(img.width() / 2, img.height() / 2, image::imageops::FilterType::Triangle);
                }
                let mut png = Vec::new();
                img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
                let data = base64::engine::general_purpose::STANDARD.encode(&png);
                Ok(json!({"content": [
                    {"type": "image", "data": data, "mimeType": "image/png"},
                    {"type": "text", "text": format!("saved {}", path.display())},
                ]}))
            }
            "text" => {
                let app = self.app()?;
                let lines: Vec<String> = app
                    .read_screen()?
                    .iter()
                    .map(|h| format!("{:>5.0},{:>4.0}  {}", h.x + h.w / 2.0, h.y + h.h / 2.0, h.text))
                    .collect();
                Ok(text(lines.join("\n")))
            }
            other => bail!("unknown tool {other}"),
        }
    }
}

fn num(a: &Value, key: &str) -> Result<f32> {
    a[key].as_f64().map(|v| v as f32).ok_or_else(|| anyhow!("missing number `{key}`"))
}

fn tools() -> Value {
    let obj = |props: Value, required: &[&str]| json!({"type": "object", "properties": props, "required": required});
    json!([
        {"name": "launch", "description": "Launch Kore (release build with --features test-harness) in the background against the harness kind clusters. Sandbox: harness/runs/mcp/home.",
         "inputSchema": obj(json!({"fresh": {"type": "boolean", "description": "Start from an empty sandbox HOME"}, "env": {"type": "object", "description": "Extra env vars"}}), &[])},
        {"name": "quit", "description": "Quit the app.", "inputSchema": obj(json!({}), &[])},
        {"name": "snapshot", "description": "View state JSON plus the automation element registry (id, role, label, bounds in window points).",
         "inputSchema": obj(json!({"filter": {"type": "string", "description": "Only elements whose id matches (suffix * = prefix)"}, "state_only": {"type": "boolean"}}), &[])},
        {"name": "click", "description": "Click an element by id (preferred) or at window point x,y.",
         "inputSchema": obj(json!({"id": {"type": "string"}, "x": {"type": "number"}, "y": {"type": "number"}, "count": {"type": "integer"}, "right": {"type": "boolean"}, "timeout_ms": {"type": "integer"}}), &[])},
        {"name": "hover", "description": "Move the mouse over an element (id) or point.", "inputSchema": obj(json!({"id": {"type": "string"}, "x": {"type": "number"}, "y": {"type": "number"}}), &[])},
        {"name": "type", "description": "Type text into the focused element.", "inputSchema": obj(json!({"text": {"type": "string"}}), &["text"])},
        {"name": "key", "description": "Press a chord, e.g. cmd-k, escape, j, shift-enter, cmd-shift-r.", "inputSchema": obj(json!({"chord": {"type": "string"}}), &["chord"])},
        {"name": "scroll", "description": "Wheel-scroll over an element (id) or point; positive dy scrolls down.",
         "inputSchema": obj(json!({"id": {"type": "string"}, "x": {"type": "number"}, "y": {"type": "number"}, "dy": {"type": "number"}}), &["dy"])},
        {"name": "action", "description": "Run a named app action: navigate {view}, set_kind {kind}, set_namespace {namespace}, toast {message}, minimize.",
         "inputSchema": obj(json!({"name": {"type": "string"}, "args": {"type": "object"}}), &["name"])},
        {"name": "wait", "description": "Wait in-app until an element exists (optionally label regex), an element is gone, or a state JSON pointer equals a value.",
         "inputSchema": obj(json!({"element": {"type": "string"}, "label": {"type": "string"}, "gone": {"type": "string"}, "pointer": {"type": "string"}, "equals": {}, "timeout_ms": {"type": "integer"}}), &[])},
        {"name": "screenshot", "description": "Render the window off-screen (works in the background) and return the PNG.",
         "inputSchema": obj(json!({"full_resolution": {"type": "boolean"}}), &[])},
        {"name": "text", "description": "OCR the window: one line per text box with its center in window points.", "inputSchema": obj(json!({}), &[])},
    ])
}
