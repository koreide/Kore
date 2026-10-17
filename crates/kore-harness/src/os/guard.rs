//! Layer 3 safety net. A run that takes over the user's mouse and keyboard
//! must always give them back, including on Ctrl-C, panics and hung scenarios.
//!
//! - A per-scenario hard deadline. On expiry the watchdog releases input,
//!   kills the app and exits with status 124.
//! - SIGINT / SIGTERM / SIGHUP. The handler only sets a flag, since posting
//!   events isn't async-signal-safe; the watchdog does the cleanup.
//! - Panics. The hook releases input before unwinding.
//! - Normal end. [`disarm`] releases input.

use super::input;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static ARMED: AtomicBool = AtomicBool::new(false);
static SIGNALLED: AtomicI32 = AtomicI32::new(0);
static TIMEOUT_MS: AtomicU64 = AtomicU64::new(60_000);
static DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
static APP_PID: AtomicI32 = AtomicI32::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

extern "C" fn on_signal(sig: libc::c_int) {
    SIGNALLED.store(sig, Ordering::SeqCst);
}

/// Install the watchdog, signal handlers and panic hook for a Layer 3 run.
pub fn arm(per_scenario: Duration) {
    TIMEOUT_MS.store(per_scenario.as_millis() as u64, Ordering::SeqCst);
    DEADLINE_MS.store(0, Ordering::SeqCst);
    ARMED.store(true, Ordering::SeqCst);
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        unsafe { libc::signal(sig, on_signal as *const () as libc::sighandler_t) };
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        input::release_all();
        prev(info);
    }));
    std::thread::Builder::new()
        .name("layer3-watchdog".into())
        .spawn(|| loop {
            std::thread::sleep(Duration::from_millis(100));
            if !ARMED.load(Ordering::SeqCst) {
                continue;
            }
            let sig = SIGNALLED.load(Ordering::SeqCst);
            if sig != 0 {
                eprintln!("\nlayer 3: signal {sig}: releasing input and stopping the app");
                emergency_stop();
                std::process::exit(128 + sig);
            }
            let deadline = DEADLINE_MS.load(Ordering::SeqCst);
            if deadline != 0 && now_ms() > deadline {
                eprintln!(
                    "\nlayer 3: scenario exceeded its {}s hard timeout: releasing input and stopping the app",
                    TIMEOUT_MS.load(Ordering::SeqCst) / 1000
                );
                emergency_stop();
                std::process::exit(124);
            }
        })
        .expect("spawn watchdog");
}

/// A scenario starts now (resets the hard deadline); `pid` is the app to kill on expiry.
pub fn scenario_started(pid: Option<i32>) {
    APP_PID.store(pid.unwrap_or(0), Ordering::SeqCst);
    DEADLINE_MS.store(now_ms() + TIMEOUT_MS.load(Ordering::SeqCst), Ordering::SeqCst);
}

/// Normal end of the run: stop the watchdog and release input.
pub fn disarm() {
    ARMED.store(false, Ordering::SeqCst);
    DEADLINE_MS.store(0, Ordering::SeqCst);
    input::release_all();
}

fn emergency_stop() {
    input::release_all();
    let pid = APP_PID.load(Ordering::SeqCst);
    // Only a pid we launched, never 0/1 (process group / init).
    if pid > 1 {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}
