//! Helpers for the cluster_admin tests (candidates for the shared driver).

use crate::testing::Kore;
use std::time::{Duration, Instant};

pub trait KoreExt {
    /// `wait_until` with a custom timeout (the shared one is 5 s).
    fn wait_until_for(&mut self, what: &str, timeout: Duration, pred: impl FnMut(&mut Kore) -> bool);
    /// Navigate to an app view the way the sidebar does (`sidebar.view.<view>`).
    fn open_view(&mut self, view: &str);
    /// Label of element `id`, if present.
    fn label(&mut self, id: &str) -> Option<String>;
    /// Ids of all elements whose id starts with `prefix`.
    fn ids_with_prefix(&mut self, prefix: &str) -> Vec<String>;
    /// Message of the open confirm dialog.
    fn confirm_message(&mut self) -> Option<String>;
    /// Labels of the visible toasts.
    fn toasts(&mut self) -> Vec<String>;
    /// Advance GPUI's test clock by `d` and render (fires debounce timers such
    /// as the table's 500 ms refresh delay without polling frame by frame).
    fn advance(&mut self, d: Duration);
}

impl KoreExt for Kore {
    fn wait_until_for(&mut self, what: &str, timeout: Duration, mut pred: impl FnMut(&mut Kore) -> bool) {
        let deadline = Instant::now() + timeout;
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

    fn open_view(&mut self, view: &str) {
        self.click(&format!("sidebar.view.{view}"));
        let v = view.to_string();
        self.wait_until(&format!("view {view}"), |k| k.state()["view"] == v.as_str());
    }

    fn label(&mut self, id: &str) -> Option<String> {
        self.element(id).map(|e| e.label)
    }

    fn ids_with_prefix(&mut self, prefix: &str) -> Vec<String> {
        self.elements().into_iter().filter(|e| e.id.starts_with(prefix)).map(|e| e.id).collect()
    }

    fn confirm_message(&mut self) -> Option<String> {
        self.app_state(|s, _| s.confirm.as_ref().map(|c| c.message.clone()))
    }

    fn advance(&mut self, d: Duration) {
        self.cx.executor().advance_clock(d);
        self.frame();
    }

    fn toasts(&mut self) -> Vec<String> {
        self.elements().into_iter().filter(|e| e.id.starts_with("toast.")).map(|e| e.label).collect()
    }
}
