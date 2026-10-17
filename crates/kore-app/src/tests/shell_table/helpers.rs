//! Shared helpers for the shell_table tests: table cell / header / row
//! readers and a sidebar click that scrolls the nav list first.

use crate::testing::Kore;
use gpui_kit::{point, px, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase};
use kore_core::ResourceKind;
use std::time::Duration;

/// Boot the harness world and show the kore-demo Pods table (rows loaded).
pub fn pods_kore_demo(cx: &mut TestAppContext) -> Kore {
    let mut k = Kore::boot(cx);
    k.show(ResourceKind::Pods, "kore-demo");
    k.settle();
    k.wait_for_element("table.row.kore-demo/crashloop");
    k
}

/// A running pod in kore-demo with the given name (watch/delete probes).
pub fn probe_pod(name: &str) -> serde_json::Value {
    serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": name, "namespace": "kore-demo", "labels": {"run": name}},
        "spec": {"containers": [{"name": name, "image": "busybox:1.36"}]},
        "status": {
            "phase": "Running",
            "containerStatuses": [{"name": name, "ready": true, "restartCount": 0, "state": {"running": {}}}]
        }
    })
}

pub trait TableExt {
    /// Fast-forward the fake clock while the table is loading (the store's
    /// 500 ms + 100 ms load delays), so kind switches take a few frames.
    fn settle(&mut self);
    /// Advance GPUI's fake clock by `ms` (toast auto-dismiss etc.).
    fn advance(&mut self, ms: u64);
    /// Non-empty column header labels, left to right.
    fn headers(&mut self) -> Vec<String>;
    /// Wait until the headers are exactly `want`.
    fn wait_headers(&mut self, want: &[&str]);
    /// Visible row keys (`ns/name`), top to bottom.
    fn row_keys(&mut self) -> Vec<String>;
    /// Row names (labels), top to bottom.
    fn row_names(&mut self) -> Vec<String>;
    /// The rendered text of cell `col` of row `key` (waits for the cell).
    fn cell(&mut self, key: &str, col: &str) -> String;
    /// Every cell label of column `col`, top to bottom.
    fn column(&mut self, col: &str) -> Vec<String>;
    /// First visible row key whose name starts with `prefix`.
    fn row_key_starting(&mut self, prefix: &str) -> String;
    /// Click a sidebar item (`sidebar.kind.<kind>` etc.), scrolling the nav
    /// list until it is inside the window first.
    fn click_sidebar(&mut self, id: &str);
    /// Clear the header search (focus it, select all, delete) and blur it.
    fn clear_search(&mut self);
    /// Blur any text input by clicking the breadcrumb (like the scenarios' click_at).
    fn blur(&mut self);
}

impl TableExt for Kore {
    fn settle(&mut self) {
        self.frame();
        for _ in 0..100 {
            if self.state()["loading"] == false {
                break;
            }
            self.advance(50);
        }
        self.frame();
    }

    fn advance(&mut self, ms: u64) {
        self.cx.executor().advance_clock(Duration::from_millis(ms));
        self.frame();
    }

    fn headers(&mut self) -> Vec<String> {
        let mut hs: Vec<_> = self.elements().into_iter().filter(|e| e.id.starts_with("table.header.") && !e.label.is_empty()).collect();
        hs.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
        hs.into_iter().map(|e| e.label).collect()
    }

    fn wait_headers(&mut self, want: &[&str]) {
        self.wait_until(&format!("headers {want:?}"), |k| k.headers() == want);
    }

    fn row_keys(&mut self) -> Vec<String> {
        let mut rows: Vec<_> = self.elements().into_iter().filter(|e| e.id.starts_with("table.row.")).collect();
        rows.sort_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
        rows.into_iter().map(|e| e.id["table.row.".len()..].to_string()).collect()
    }

    fn row_names(&mut self) -> Vec<String> {
        self.row_keys().into_iter().map(|k| k.rsplit('/').next().unwrap_or_default().to_string()).collect()
    }

    fn cell(&mut self, key: &str, col: &str) -> String {
        self.wait_for_element(&format!("table.cell.{key}.{col}")).label
    }

    fn column(&mut self, col: &str) -> Vec<String> {
        self.row_keys()
            .into_iter()
            .filter_map(|key| self.element(&format!("table.cell.{key}.{col}")).map(|e| e.label))
            .collect()
    }

    fn row_key_starting(&mut self, prefix: &str) -> String {
        self.wait_until(&format!("row starting {prefix}"), |k| k.row_names().iter().any(|n| n.starts_with(prefix)));
        self.row_keys().into_iter().find(|k| k.rsplit('/').next().unwrap_or_default().starts_with(prefix)).unwrap()
    }

    fn click_sidebar(&mut self, id: &str) {
        for _ in 0..20 {
            let e = self.wait_for_element(id);
            // The footer (settings button) sits below the scrolling nav list.
            let limit = self.element("sidebar.settings").map(|s| s.bounds.y - 8.).unwrap_or(840.);
            let (top, bottom) = (e.bounds.y, e.bounds.y + e.bounds.h);
            if top >= 0. && bottom <= limit {
                self.click(id);
                self.settle();
                return;
            }
            let dy = if bottom > limit { bottom - limit + 40. } else { top - 40. };
            self.cx.simulate_event(ScrollWheelEvent {
                position: point(px(100.), px(500.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(-dy))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            });
            self.frame();
        }
        panic!("could not scroll {id} into view");
    }

    fn clear_search(&mut self) {
        self.click("header.search");
        self.keys("cmd-a backspace");
        self.blur();
        self.wait_until("search cleared", |k| k.state()["search"] == "");
    }

    fn blur(&mut self) {
        self.click("header.breadcrumb");
    }
}
