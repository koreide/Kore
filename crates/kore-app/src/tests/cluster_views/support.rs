//! Helpers shared by the cluster_views tests: an Ollama provider pointed at the
//! in-process mock AI server, and small readers over the automation registry.

use crate::state::ai_config::{AiConfig, STORAGE_KEY};
use crate::state::storage;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_testkit::{fixtures, MockAi};

/// Store the AI config the app loads at boot (same key Settings writes).
pub fn seed_ai_config(provider: &str, model: &str, base_url: Option<&str>) {
    storage::set(
        STORAGE_KEY,
        &AiConfig { provider: provider.into(), api_key: None, model: model.into(), base_url: base_url.map(str::to_string) },
    );
}

/// Boot the harness world with AI configured as Ollama -> mock (what
/// `_lib/cluster-ai-ollama-mock.yaml` does through the Settings UI). Core's
/// default AI endpoint is the mock too, so nothing ever reaches localhost:11434.
pub fn boot_with_mock_ai(cx: &mut TestAppContext) -> (Kore, MockAi) {
    let mock = MockAi::start().expect("mock AI");
    seed_ai_config("ollama", "mock-model", Some(&mock.url));
    let (env, c1, c2) = fixtures::harness_env();
    let env = env.ai_url(&mock.url);
    (Kore::boot_env(cx, env, c1, c2), mock)
}

pub trait KoreExt {
    fn label(&mut self, id: &str) -> Option<String>;
    /// Labels of every element whose id starts with `prefix`, in registry order.
    fn labels_with_prefix(&mut self, prefix: &str) -> Vec<(String, String)>;
    fn is_disabled(&mut self, id: &str) -> bool;
    fn is_selected(&mut self, id: &str) -> bool;
    /// Toast messages currently shown (from the automation state).
    fn toasts(&mut self) -> Vec<String>;
    fn wait_toast(&mut self, what: &str, pred: impl FnMut(&str) -> bool);
    /// Replace the text of a focused-by-click text field.
    fn replace_text(&mut self, id: &str, text: &str);
    /// Wait until `id` stops moving (e.g. the AI panel's slide-in), so a click
    /// lands where the registry says it is.
    fn wait_settled(&mut self, id: &str);
}

impl KoreExt for Kore {
    fn label(&mut self, id: &str) -> Option<String> {
        self.element(id).map(|e| e.label)
    }

    fn labels_with_prefix(&mut self, prefix: &str) -> Vec<(String, String)> {
        self.elements().into_iter().filter(|e| e.id.starts_with(prefix)).map(|e| (e.id, e.label)).collect()
    }

    fn is_disabled(&mut self, id: &str) -> bool {
        self.wait_for_element(id).disabled
    }

    fn is_selected(&mut self, id: &str) -> bool {
        self.wait_for_element(id).selected
    }

    fn toasts(&mut self) -> Vec<String> {
        self.state()["toasts"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t["message"].as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    fn wait_toast(&mut self, what: &str, mut pred: impl FnMut(&str) -> bool) {
        self.wait_until(&format!("toast {what}"), |k| k.toasts().iter().any(|t| pred(t)));
    }

    fn wait_settled(&mut self, id: &str) {
        let mut last = None;
        let mut same = 0;
        self.wait_until(&format!("{id} settled"), |k| {
            let b = k.element(id).map(|e| e.bounds);
            if b.is_some() && b == last {
                same += 1;
            } else {
                same = 0;
            }
            last = b;
            same >= 3
        });
    }

    fn replace_text(&mut self, id: &str, text: &str) {
        self.click(id);
        self.keys("cmd-a");
        self.type_text(text);
    }
}
