//! What the root view exposes to automation: the control server (Layer 2) and
//! the unit-test driver (Layer 1) read the same view state.

use gpui_kit::*;

/// Implemented by the root view so the server can read state and run named actions.
pub trait AutomationHost: 'static {
    fn automation_state(&self, cx: &App) -> serde_json::Value;
    fn automation_action(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<serde_json::Value>
    where
        Self: Sized;
}
