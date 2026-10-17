//! Network Policies view (spec 03): stats, policy panel, filter, suggested
//! questions, the AI chat with tool status and local traffic simulation.

use super::support::{boot_with_mock_ai, KoreExt};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_testkit::mock_ai::MARKDOWN_ANSWER;
use serde_json::json;

const DENY: &str = "np.policy.default-deny-ingress";
const ALLOW: &str = "np.policy.allow-logger-to-web";

fn open_netpol(k: &mut Kore) {
    k.action("set_namespace", json!({"namespace": "kore-demo"}));
    k.click("sidebar.view.network-policies");
    k.wait_until("netpol view", |k| k.state()["view"] == "network-policies");
    k.wait_label("np.stats.policies", "2 policies", |l| l == "2 policies");
}

fn ask(k: &mut Kore, q: &str) {
    k.wait_until("np input enabled", |k| k.element("np.input").is_some() && k.element("np.tool-status").is_none());
    // The input ignores typing until the AI provider is known to be configured
    // (an async availability probe), so retry until the text lands.
    for attempt in 0..20 {
        k.click("np.input");
        k.type_text(q);
        if k.element("np.send").is_some_and(|e| !e.disabled) {
            break;
        }
        assert!(attempt < 19, "typed question never reached the netpol input");
        k.keys("cmd-a backspace");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    k.keys("enter");
}

/// Scenario 30-cluster-netpol: stats bar, both policy cards with their
/// namespace / types / selector / rule counts, suggested questions, the policy
/// filter, card selection and the panel collapse toggle.
#[gpui_kit::test]
fn netpol_stats_cards_filter_and_collapse(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_netpol(&mut k);
    let re = regex::Regex::new(r"^\d+ workloads$").unwrap();
    assert!(re.is_match(&k.label("np.stats.workloads").unwrap()));
    let re = regex::Regex::new(r"^\d+ edges$").unwrap();
    assert!(re.is_match(&k.label("np.stats.edges").unwrap()));

    assert_eq!(k.label(&format!("{DENY}.ns")).as_deref(), Some("ns: kore-demo"));
    assert_eq!(k.label(&format!("{DENY}.types")).as_deref(), Some("Ingress"));
    assert_eq!(k.label(&format!("{DENY}.selector")).as_deref(), Some("all pods"));
    assert_eq!(k.label(&format!("{ALLOW}.ns")).as_deref(), Some("ns: kore-demo"));
    assert_eq!(k.label(&format!("{ALLOW}.types")).as_deref(), Some("Ingress, Egress"));
    assert_eq!(k.label(&format!("{ALLOW}.selector")).as_deref(), Some("app=web"));
    assert_eq!(k.label(&format!("{ALLOW}.rules")).as_deref(), Some("1 ingress rule, 1 egress rule"));

    let questions: Vec<String> = k.labels_with_prefix("np.question.").into_iter().map(|(_, l)| l).collect();
    for q in ["Which pods are isolated and why?", "Summarize the network security posture"] {
        assert!(questions.iter().any(|x| x == q), "{q:?} in {questions:?}");
    }
    assert!(k.element("banner.ai-config").is_none(), "AI configured");

    // Filter.
    k.click("np.filter");
    k.type_text("deny");
    k.wait_gone(ALLOW);
    k.wait_for_element(DENY);
    // Select a card (and toggle it off again).
    assert!(!k.is_selected(DENY));
    k.click(DENY);
    assert!(k.is_selected(DENY));
    k.click(DENY);
    assert!(!k.is_selected(DENY));

    // Collapse / expand the policy panel.
    assert_eq!(k.label("np.panel.toggle").as_deref(), Some("Collapse policies"));
    k.click("np.panel.toggle");
    k.wait_gone("np.filter");
    assert!(k.element(DENY).is_none());
    assert_eq!(k.label("np.panel.toggle").as_deref(), Some("Expand policies"));
    k.click("np.panel.toggle");
    k.wait_for_element("np.filter");
    k.wait_for_element(DENY);
}

/// Scenario 30-cluster-bug-netpol-status (quirk 10.16, fixed): a tool call
/// ("health" -> get_cluster_health) replaces "Thinking..." with "Querying get
/// cluster health..." until the answer renders.
#[gpui_kit::test]
fn netpol_chat_shows_tool_status(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_netpol(&mut k);
    ask(&mut k, "how is the cluster health");
    k.wait_label("np-msg-0.text", "user question", |l| l == "how is the cluster health");
    k.wait_label("np.tool-status", "Querying...", |l| l == "Querying get cluster health...");
    k.wait_label("np-msg-1.text", "answer", |l| l == MARKDOWN_ANSWER);
    k.wait_gone("np.tool-status");
}

/// Scenario 30-cluster-netpol-simulate: "can logger talk to web" attaches a
/// locally computed DENIED simulation (no egress policy on logger; web is
/// ingress-isolated by default-deny-ingress) with expandable evaluations, and
/// the assistant reply follows; "can web talk to logger" is denied both ways.
#[gpui_kit::test]
fn netpol_chat_simulates_traffic(cx: &mut TestAppContext) {
    let (mut k, _mock) = boot_with_mock_ai(cx);
    open_netpol(&mut k);
    ask(&mut k, "can logger talk to web");
    k.wait_label("np.sim.0", "DENIED", |l| l == "DENIED");
    assert_eq!(k.label("np.sim.0.route").as_deref(), Some("logger → web"));
    let summary = k.label("np.sim.0.summary").unwrap();
    assert!(summary.starts_with("Traffic DENIED from kore-demo/logger-"), "{summary:?}");
    assert_eq!(k.label("np.sim.0.egress").as_deref(), Some("Egress: No policy"));
    assert_eq!(k.label("np.sim.0.ingress").as_deref(), Some("Ingress: Isolated"));
    let details = k.label("np.sim.0.details").unwrap();
    assert!(regex::Regex::new(r"^Policy details \(\d+ evaluations?\)$").unwrap().is_match(&details), "{details:?}");
    k.wait_label("np-msg-1.text", "assistant reply", |l| l == MARKDOWN_ANSWER);

    assert!(k.element("np.sim.0.eval.0").is_none(), "collapsed");
    k.click("np.sim.0.details");
    k.wait_for_element("np.sim.0.eval.0");
    let evals: Vec<String> = k.labels_with_prefix("np.sim.0.eval.").into_iter().map(|(_, l)| l).collect();
    assert!(evals.iter().any(|e| e.starts_with("ingress default-deny-ingress:")), "{evals:?}");
    assert!(evals.iter().any(|e| e.starts_with("ingress allow-logger-to-web:")), "{evals:?}");

    ask(&mut k, "can web talk to logger");
    k.wait_label("np.sim.2", "DENIED", |l| l == "DENIED");
    assert_eq!(k.label("np.sim.2.route").as_deref(), Some("web → logger"));
    let summary = k.label("np.sim.2.summary").unwrap();
    assert!(summary.starts_with("Traffic DENIED from kore-demo/web-"), "{summary:?}");
    assert!(summary.contains("denied and ingress denied"), "{summary:?}");
    assert_eq!(k.label("np.sim.2.egress").as_deref(), Some("Egress: Isolated"));
    k.wait_label("np-msg-3.text", "assistant reply", |l| l == MARKDOWN_ANSWER);
}
