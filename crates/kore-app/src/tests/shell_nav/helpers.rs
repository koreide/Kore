//! Shell/navigation helpers on top of the shared driver (`crate::testing::Kore`).

use crate::testing::Kore;
use gpui_kit::*;
use kore_core::{CoreConfig, ResourceKind};
use kore_testkit::FakeEnv;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Window height used by the driver (`Kore::open`).
const WINDOW_H: f32 = 900.;

pub trait ShellExt {
    /// `shell-pods-kore-demo`: Pods table in kore-demo with the seeded rows.
    fn pods_kore_demo(&mut self);
    /// `shell-open-settings`: the sidebar gear opens Settings.
    fn open_settings(&mut self);
    /// Cmd+K and wait for the palette input.
    fn open_palette(&mut self);
    /// Current routed view (`state.view`).
    fn view(&mut self) -> String;
    /// Ids starting with `prefix`, in on-screen (top to bottom) order.
    fn ids_in_order(&mut self, prefix: &str) -> Vec<String>;
    /// Labels of the elements whose id starts with `prefix`, top to bottom.
    fn labels_in_order(&mut self, prefix: &str) -> Vec<String>;
    /// Scroll the scroll area under `area` until element `id` is fully inside the window.
    fn scroll_into_view(&mut self, area: &str, id: &str);
    /// Open the sidebar namespace dropdown (closing other panels first).
    fn open_ns_dropdown(&mut self);
    /// Pick `ns` in the namespace dropdown by typing and clicking it.
    fn select_namespace(&mut self, ns: &str);
    /// Dispatch one raw keystroke (exact `key` / modifiers, no parsing).
    fn raw_key(&mut self, key: &str, modifiers: Modifiers);
    /// `wait_until`, but fast-forwarding GPUI's fake clock 100ms per poll, so the
    /// app's own debounces (resource refresh 500ms + 100ms, palette search 300ms)
    /// elapse quickly. The driver's `frame` only advances 5ms per (much slower) frame.
    fn wait_ff(&mut self, what: &str, pred: impl FnMut(&mut Kore) -> bool);
    /// `wait_for_element` with [`ShellExt::wait_ff`] pacing (table rows, search results).
    fn wait_el_ff(&mut self, id: &str);
}

impl ShellExt for Kore {
    fn pods_kore_demo(&mut self) {
        self.show(ResourceKind::Pods, "kore-demo");
        self.wait_el_ff("table.row.kore-demo/multi-container");
    }

    fn open_settings(&mut self) {
        self.click("sidebar.settings");
        self.wait_for_element("view.settings");
        self.wait_until("settings view", |k| k.state()["view"] == "settings");
    }

    fn open_palette(&mut self) {
        self.keys("cmd-k");
        self.wait_for_element("palette.input");
    }

    fn view(&mut self) -> String {
        self.state()["view"].as_str().unwrap_or_default().to_string()
    }

    fn ids_in_order(&mut self, prefix: &str) -> Vec<String> {
        let mut els: Vec<_> = self.elements().into_iter().filter(|e| e.id.starts_with(prefix)).collect();
        els.sort_by(|a, b| a.bounds.y.partial_cmp(&b.bounds.y).unwrap());
        els.into_iter().map(|e| e.id).collect()
    }

    fn labels_in_order(&mut self, prefix: &str) -> Vec<String> {
        let mut els: Vec<_> = self.elements().into_iter().filter(|e| e.id.starts_with(prefix)).collect();
        els.sort_by(|a, b| a.bounds.y.partial_cmp(&b.bounds.y).unwrap());
        els.into_iter().map(|e| e.label).collect()
    }

    fn scroll_into_view(&mut self, area: &str, id: &str) {
        for _ in 0..40 {
            let e = self.wait_for_element(id);
            let top = e.bounds.y;
            let bottom = e.bounds.y + e.bounds.h;
            if top >= 0. && bottom <= WINDOW_H - 8. {
                return;
            }
            let a = self.wait_for_element(area);
            let (x, y) = a.bounds.center();
            let dy = if bottom > WINDOW_H { -300. } else { 300. };
            self.cx.simulate_event(ScrollWheelEvent {
                position: point(px(x), px(y)),
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            });
            self.frame();
        }
        panic!("could not scroll {id} into view");
    }

    fn open_ns_dropdown(&mut self) {
        if self.element("sidebar.namespace.panel").is_some() {
            return;
        }
        self.click("sidebar.namespace.trigger");
        self.wait_for_element("sidebar.namespace.panel");
    }

    fn select_namespace(&mut self, ns: &str) {
        self.open_ns_dropdown();
        self.type_text(ns);
        self.click(&format!("sidebar.namespace.item.{ns}"));
        self.wait_gone("sidebar.namespace.panel");
        let want = ns.to_string();
        self.wait_until(&format!("namespace {ns}"), |k| k.state()["namespace"] == want.as_str());
    }

    fn wait_ff(&mut self, what: &str, mut pred: impl FnMut(&mut Kore) -> bool) {
        self.wait_until(what, |k| {
            if pred(k) {
                return true;
            }
            k.cx.executor().advance_clock(Duration::from_millis(100));
            false
        });
    }

    fn wait_el_ff(&mut self, id: &str) {
        self.wait_ff(&format!("element {id}"), |k| k.element(id).is_some());
    }

    fn raw_key(&mut self, key: &str, modifiers: Modifiers) {
        let ks = Keystroke { modifiers, key: key.to_string(), key_char: None };
        self.cx.update(|window, cx| {
            window.dispatch_keystroke(ks, cx);
        });
        self.frame();
    }
}

/// Boot the real UI with an explicit `CoreConfig` (e.g. a kubeconfig path that
/// doesn't exist). `env` keeps the sandbox alive; the clusters are unused.
pub fn boot_with_config(cx: &mut TestAppContext, env: FakeEnv, config: CoreConfig) -> Kore {
    Kore::boot_with_config(cx, env, config)
}

/// A one-route HTTP server answering every request with `body()` (JSON), like
/// GitHub's "latest release" endpoint. Lives until the test thread ends.
pub struct JsonServer {
    pub url: String,
    pub body: Arc<Mutex<String>>,
    pub hits: Arc<Mutex<usize>>,
}

impl JsonServer {
    pub fn start(body: &str) -> JsonServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/releases/latest", listener.local_addr().unwrap());
        let body = Arc::new(Mutex::new(body.to_string()));
        let hits = Arc::new(Mutex::new(0));
        let (b, h) = (body.clone(), hits.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                *h.lock().unwrap() += 1;
                let body = b.lock().unwrap().clone();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        JsonServer { url, body, hits }
    }

    pub fn set_body(&self, body: &str) {
        *self.body.lock().unwrap() = body.to_string();
    }

    pub fn hits(&self) -> usize {
        *self.hits.lock().unwrap()
    }
}

/// GitHub release JSON with `tag`.
pub fn release_json(tag: &str) -> String {
    serde_json::json!({
        "tag_name": tag,
        "html_url": format!("https://example.invalid/releases/{tag}"),
        "body": "notes",
        "assets": [],
    })
    .to_string()
}
