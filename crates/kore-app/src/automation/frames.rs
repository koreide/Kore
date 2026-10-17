//! Frame timing: CPU time to build each frame (render + layout + prepaint + paint),
//! measured from the root view's render to a marker painted last. Recorded only
//! while automation is enabled (control server or unit tests).
#![cfg_attr(not(feature = "test-harness"), allow(dead_code))]

use kore_automation::FrameStats;
use std::cell::RefCell;
use std::time::Instant;

const FRAME_BUDGET_MS: f64 = 1000.0 / 60.0;

thread_local! {
    static START: RefCell<Option<Instant>> = const { RefCell::new(None) };
    static SAMPLES: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

pub fn frame_started() {
    if !super::registry::enabled() {
        return;
    }
    START.with(|s| *s.borrow_mut() = Some(Instant::now()));
}

pub fn frame_finished() {
    if let Some(start) = START.with(|s| s.borrow_mut().take()) {
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        SAMPLES.with(|s| {
            let mut s = s.borrow_mut();
            // Bound memory for long sessions.
            if s.len() > 100_000 {
                s.drain(..50_000);
            }
            s.push(ms);
        });
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub fn stats(reset: bool) -> FrameStats {
    SAMPLES.with(|s| {
        let mut s = s.borrow_mut();
        let mut sorted = s.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len();
        let stats = FrameStats {
            frames: n as u64,
            mean_ms: if n == 0 { 0.0 } else { sorted.iter().sum::<f64>() / n as f64 },
            p50_ms: percentile(&sorted, 0.50),
            p95_ms: percentile(&sorted, 0.95),
            p99_ms: percentile(&sorted, 0.99),
            max_ms: sorted.last().copied().unwrap_or(0.0),
            over_budget: sorted.iter().filter(|&&ms| ms > FRAME_BUDGET_MS).count() as u64,
        };
        if reset {
            s.clear();
        }
        stats
    })
}
