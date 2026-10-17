//! The [`CoreConfig`] of the `Core` whose runtime thread is running.
//!
//! Every `Core` owns its own tokio runtime, and each of its threads (workers and
//! the blocking pool) carries that Core's config. Free functions deep in the
//! backend, such as tool resolution and AI endpoints, read it here instead of
//! process-global env vars. Two Cores in one process, like parallel tests, so
//! never see each other's settings. Off a Core thread this returns `None`, and
//! callers use their defaults.

use crate::config::CoreConfig;
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static CONFIG: RefCell<Option<Arc<CoreConfig>>> = const { RefCell::new(None) };
}

pub(crate) fn install(config: Arc<CoreConfig>) {
    CONFIG.with(|c| *c.borrow_mut() = Some(config));
}

pub(crate) fn get() -> Option<Arc<CoreConfig>> {
    CONFIG.with(|c| c.borrow().clone())
}
