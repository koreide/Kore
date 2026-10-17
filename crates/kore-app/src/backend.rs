//! Bridge between GPUI and `kore_core::Core`.
//!
//! `Core` owns its own tokio runtime; every async method returns a future that
//! can be awaited on GPUI's executor. Typical use from a view:
//!
//! ```ignore
//! let core = backend::core(cx);
//! cx.spawn(async move |this, cx| {
//!     let res = core.list_contexts().await;
//!     this.update(cx, |this, cx| { /* apply */ cx.notify() }).ok();
//! }).detach();
//! ```
//!
//! or the shorthand [`run`].

use gpui_kit::*;
use kore_core::Core;
use std::future::Future;
use std::time::Duration;

/// Global holding the backend facade (cheap to clone).
pub struct Backend(pub Core);

impl Global for Backend {}

/// Initialise the `Core` runtime. `KUBECONFIG` is read by Core. PATH
/// enrichment already happened at the top of `main` (before any thread exists).
pub fn init(cx: &mut App) {
    init_with(Core::new().expect("failed to start kore-core runtime"), cx);
}

/// Install an already-built `Core` (tests pass one backed by a fake cluster).
pub fn init_with(core: Core, cx: &mut App) {
    cx.set_global(Backend(core));
    // Cancel watches, log/exec/AI streams and port-forwards on quit (bounded wait).
    cx.on_app_quit(|cx| {
        let core = cx.global::<Backend>().0.clone();
        let timer = cx.background_executor().timer(std::time::Duration::from_secs(2));
        async move {
            use futures::FutureExt;
            futures::select! {
                _ = core.shutdown().fuse() => {}
                _ = timer.fuse() => {}
            }
        }
    })
    .detach();
}

pub fn core(cx: &App) -> Core {
    cx.global::<Backend>().0.clone()
}

/// Run a Core call and apply its result to the view (`then` is skipped if the
/// view was dropped). The returned task must be stored or detached; dropping it
/// cancels the call (and drops any `Subscription` it produced).
pub fn run<V, T, Fut>(
    cx: &mut Context<V>,
    call: impl FnOnce(Core) -> Fut + 'static,
    then: impl FnOnce(&mut V, T, &mut Context<V>) + 'static,
) -> Task<()>
where
    V: 'static,
    T: 'static,
    Fut: Future<Output = T> + 'static,
{
    let core = core(cx);
    cx.spawn(async move |this, cx| {
        let out = call(core).await;
        this.update(cx, |v, cx| then(v, out, cx)).ok();
    })
}

/// A timer future usable on the GPUI executor.
pub fn sleep(cx: &App, d: Duration) -> Task<()> {
    cx.background_executor().timer(d)
}

/// User-facing message for a Core error.
pub fn format_error(e: &kore_core::K8sError) -> String {
    e.to_string()
}
