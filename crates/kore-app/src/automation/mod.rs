//! In-app automation.
//!
//! - `registry`: stable element ids, always compiled. It records nothing unless enabled.
//! - `frames`: frame timing.
//! - `server`: the Layer 2 control server. It exists only with the
//!   `test-harness` cargo feature; release builds never contain it.
pub mod frames;
#[cfg(any(test, feature = "test-harness"))]
mod host;
pub mod registry;
#[cfg(feature = "test-harness")]
pub mod server;

#[cfg(any(test, feature = "test-harness"))]
pub use host::AutomationHost;
pub use registry::Automation;
