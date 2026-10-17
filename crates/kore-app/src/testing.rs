//! Layer 1 test support: boot the real Kore UI in a headless GPUI test window,
//! backed by `kore-testkit`'s fake cluster, and drive it the way a user would
//! (keystrokes, clicks on automation ids, typing). No real cluster, keychain,
//! storage file or OS input is involved.
//!
//! Methods not used by every test module are fine: this is a shared driver.
//!
//! Don't `use gpui_kit::*` in a test file: it brings in GPUI's `test` macro,
//! which shadows Rust's `#[test]` ("recursion limit reached"). Import names.
//!
//! ```ignore
//! #[gpui_kit::test]
//! fn crashloop_is_listed(cx: &mut TestAppContext) {
//!     let mut k = Kore::boot(cx);
//!     k.show_pods("kore-demo");
//!     k.wait_for_element("table.row.kore-demo/crashloop");
//! }
//! ```

#![allow(dead_code)]

use crate::automation::registry;
use crate::{backend, root, state, theme};
use gpui_kit::*;
use kore_automation::Element as AElement;
use kore_core::{Core, ResourceKind};
use kore_testkit::{fixtures, FakeCluster, FakeEnv};
use std::time::{Duration, Instant};

/// Upper bound for `wait_*` (they return as soon as the condition holds). Generous
/// because the whole suite runs in parallel, each test with its own Core runtime.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Kore {
    pub cx: VisualTestContext,
    pub view: Entity<root::KoreApp>,
    pub env: FakeEnv,
    /// `kind-kore-harness` (current context).
    pub cluster: FakeCluster,
    /// `kind-kore-harness-2`.
    pub cluster_2: FakeCluster,
    last_frame: Instant,
    /// Used instead of `env.core_config()` (see [`Kore::boot_with_config`]).
    config: Option<kore_core::CoreConfig>,
}

impl Kore {
    /// The harness world (both kind contexts from the snapshot fixtures).
    pub fn boot(cx: &mut TestAppContext) -> Kore {
        let (env, c1, c2) = fixtures::harness_env();
        Self::boot_env(cx, env, c1, c2)
    }

    /// Boot with a custom environment (extra contexts, broken contexts...).
    pub fn boot_env(cx: &mut TestAppContext, env: FakeEnv, cluster: FakeCluster, cluster_2: FakeCluster) -> Kore {
        Self::boot_inner(cx, env, None, cluster, cluster_2)
    }

    /// Boot with an explicit `CoreConfig` (e.g. a kubeconfig path that doesn't
    /// exist). `env` keeps the sandbox alive; restarts reuse `config`.
    pub fn boot_with_config(cx: &mut TestAppContext, env: FakeEnv, config: kore_core::CoreConfig) -> Kore {
        Self::boot_inner(cx, env, Some(config), FakeCluster::new(), FakeCluster::new())
    }

    fn boot_inner(
        cx: &mut TestAppContext,
        env: FakeEnv,
        config: Option<kore_core::CoreConfig>,
        cluster: FakeCluster,
        cluster_2: FakeCluster,
    ) -> Kore {
        // Core runs on its own tokio runtime; GPUI's test executor must be
        // allowed to park while it waits for those results.
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            root::init_keymap(cx);
            registry::enable();
        });
        let core_config = config.clone().unwrap_or_else(|| env.core_config());
        let (window, view) = Self::open(cx, core_config);
        let cx = VisualTestContext::from_window(window, cx);
        let mut k = Kore { cx, view, env, cluster, cluster_2, last_frame: Instant::now(), config };
        k.wait_booted();
        k
    }

    fn open(cx: &mut TestAppContext, config: kore_core::CoreConfig) -> (AnyWindowHandle, Entity<root::KoreApp>) {
        let core = Core::with_config(config).expect("core");
        let settings = state::settings::Settings::load();
        cx.update(|cx| {
            backend::init_with(core, cx);
            theme::Theme::init(cx, settings.accent_color());
            let bounds = Bounds { origin: Point::default(), size: size(px(1400.), px(900.)) };
            let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
            let (window, view) =
                gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| root::KoreApp::new(settings, window, cx)))
                    .expect("open test window");
            (window, view)
        })
    }

    /// Wait until boot finished: connected (main UI) or the connection setup screen.
    fn wait_booted(&mut self) {
        self.wait_until("boot", |k| k.state()["connected"] == true || k.element("connection").is_some());
    }

    /// Quit and relaunch in the same sandbox: a new Core and window; persisted
    /// state (settings, pins, favorites, events DB) carries over, like an app restart.
    pub fn restart(&mut self) {
        self.cx.update(|window, _| window.remove_window());
        self.cx.run_until_parked();
        let config = self.config.clone().unwrap_or_else(|| self.env.core_config());
        let (window, view) = Self::open(&mut self.cx.cx, config);
        self.cx = VisualTestContext::from_window(window, &self.cx.cx);
        self.view = view;
        self.wait_booted();
    }

    /// Render a frame and let GPUI and Core make progress until `pred` holds.
    pub fn wait_until(&mut self, what: &str, mut pred: impl FnMut(&mut Kore) -> bool) {
        let deadline = Instant::now() + DEFAULT_TIMEOUT;
        loop {
            self.frame();
            if pred(self) {
                return;
            }
            if Instant::now() > deadline {
                let ids: Vec<String> = self.elements().into_iter().map(|e| format!("{} {:?}", e.id, e.label)).collect();
                panic!("timed out waiting for {what}\nstate: {}\nelements:\n  {}", self.state(), ids.join("\n  "));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Run pending GPUI work and paint one frame (refreshing the element registry).
    pub fn frame(&mut self) {
        // GPUI's test clock is fake: advance it by the real time since the last
        // frame (at least 5 ms), so debounce timers, polls and animations run at
        // real speed while we poll.
        let elapsed = self.last_frame.elapsed().max(Duration::from_millis(5));
        self.last_frame = Instant::now();
        self.cx.executor().advance_clock(elapsed);
        self.cx.run_until_parked();
        self.cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.cx.run_until_parked();
    }

    /// The automation view state (same JSON the Layer 2 `snapshot` returns).
    pub fn state(&mut self) -> serde_json::Value {
        let view = self.view.clone();
        self.cx.update(|_, cx| crate::automation::AutomationHost::automation_state(view.read(cx), cx))
    }

    /// Run a named automation action (same as Layer 2's `action` request), e.g.
    /// `("navigate", json!({"view": "dashboard"}))`.
    pub fn action(&mut self, name: &str, args: serde_json::Value) -> serde_json::Value {
        let view = self.view.clone();
        let out = self.cx.update(|window, cx| {
            view.update(cx, |v, cx| crate::automation::AutomationHost::automation_action(v, name, &args, window, cx))
        });
        self.frame();
        out.unwrap_or_else(|e| panic!("action {name}: {e:#}"))
    }

    pub fn elements(&mut self) -> Vec<AElement> {
        registry::last_frame()
    }

    /// First element whose id equals `id` (or starts with it when `id` ends with `*`).
    pub fn element(&mut self, id: &str) -> Option<AElement> {
        let els = self.elements();
        match id.strip_suffix('*') {
            Some(prefix) => els.into_iter().find(|e| e.id.starts_with(prefix)),
            None => els.into_iter().find(|e| e.id == id),
        }
    }

    pub fn wait_for_element(&mut self, id: &str) -> AElement {
        self.wait_until(&format!("element {id}"), |k| k.element(id).is_some());
        self.element(id).unwrap()
    }

    pub fn wait_gone(&mut self, id: &str) {
        self.wait_until(&format!("element {id} gone"), |k| k.element(id).is_none());
    }

    /// Wait until element `id` exists and its label satisfies `pred`.
    pub fn wait_label(&mut self, id: &str, what: &str, mut pred: impl FnMut(&str) -> bool) -> String {
        self.wait_until(&format!("{id} label {what}"), |k| k.element(id).is_some_and(|e| pred(&e.label)));
        self.element(id).unwrap().label
    }

    /// Click the center of element `id` (waits for it to exist).
    pub fn click(&mut self, id: &str) {
        let e = self.wait_for_element(id);
        let (x, y) = e.bounds.center();
        self.cx.simulate_click(point(px(x), px(y)), Modifiers::default());
        self.frame();
    }

    /// Move the mouse over element `id`.
    pub fn hover(&mut self, id: &str) {
        let e = self.wait_for_element(id);
        let (x, y) = e.bounds.center();
        self.cx.simulate_mouse_move(point(px(x), px(y)), None, Modifiers::default());
        self.frame();
    }

    /// Space-separated GPUI keystrokes, e.g. `"cmd-k"`, `"j j l"`, `"escape"`.
    pub fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
        self.frame();
    }

    /// Type text into whatever has focus (like the IME/text input path).
    pub fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.frame();
    }

    pub fn app_state<R>(&mut self, f: impl FnOnce(&state::app_state::AppState, &App) -> R) -> R {
        let view = self.view.clone();
        self.cx.update(|_, cx| {
            let app = view.read(cx).app.clone();
            f(app.read(cx), cx)
        })
    }

    pub fn update_app_state(&mut self, f: impl FnOnce(&mut state::app_state::AppState, &mut Context<state::app_state::AppState>)) {
        let view = self.view.clone();
        self.cx.update(|_, cx| {
            let app = view.read(cx).app.clone();
            app.update(cx, f);
        });
        self.frame();
    }

    /// Resource table for `kind` in `namespace`, waiting for the rows to load.
    pub fn show(&mut self, kind: ResourceKind, namespace: &str) {
        let ns = namespace.to_string();
        self.update_app_state(|m, cx| {
            m.set_namespace(ns, cx);
            m.set_kind(kind, cx);
        });
    }
}
