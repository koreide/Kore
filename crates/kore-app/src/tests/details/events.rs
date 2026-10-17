//! Events tab: live cards, History (SQLite store), the Live / History toggle.

use super::helpers::{running_pod, DetailsExt, NS};
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_core::ResourceKind;
use serde_json::json;

/// A Normal event about pod `pod`, `ago_s` seconds ago.
fn pod_event(pod: &str, reason: &str, message: &str, ago_s: i64) -> serde_json::Value {
    let t = (chrono::Utc::now() - chrono::Duration::seconds(ago_s)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    json!({
        "apiVersion": "v1", "kind": "Event",
        "metadata": {"name": format!("{pod}.{}", reason.to_lowercase()), "namespace": NS, "creationTimestamp": t},
        "involvedObject": {"kind": "Pod", "name": pod, "namespace": NS, "apiVersion": "v1"},
        "reason": reason, "message": message, "type": "Normal", "count": 1,
        "firstTimestamp": t, "lastTimestamp": t, "source": {"component": "kubelet"}
    })
}

fn seed_pod_with_events(k: &Kore, pod: &str) {
    k.cluster.apply(running_pod(pod, &[pod]));
    for (reason, message, ago) in [
        ("Scheduled", format!("Successfully assigned kore-demo/{pod} to kore-harness-control-plane"), 40),
        ("Pulled", "Container image \"busybox:1.36\" already present on machine".to_string(), 30),
        ("Created", format!("Created container: {pod}"), 20),
        ("Started", format!("Container started: {pod}"), 10),
    ] {
        k.cluster.apply(pod_event(pod, reason, &message, ago));
    }
}

/// Card labels ("Reason: message"), newest first.
fn cards(k: &mut Kore) -> Vec<String> {
    k.elements().into_iter().filter(|e| e.id.starts_with("events.card.") && !e.id.ends_with(".time")).map(|e| e.label).collect()
}

/// Scenario 20-details-events: the live Events tab of a fresh pod shows
/// Scheduled, Pulled, Created and Started cards (newest first) with messages
/// and relative times, plus the Live / History toggle; History can be entered.
#[gpui_kit::test]
fn live_events_show_cards_and_toggle(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    seed_pod_with_events(&k, "kore-events-probe");
    k.open_pod("kore-events-probe");
    k.keys("5");
    k.wait_until("4 cards", |k| cards(k).len() == 4);
    let c = cards(&mut k);
    let reasons: Vec<&str> = c.iter().map(|l| l.split(':').next().unwrap()).collect();
    assert_eq!(reasons, ["Started", "Created", "Pulled", "Scheduled"]);
    assert!(c[3].contains("Successfully assigned kore-demo/kore-events-probe"));
    assert!(c[0].contains("Container started"));
    let re = regex::Regex::new(r"^\d+[smh] ago$").unwrap();
    let time = k.label("events.card.0.time").unwrap();
    assert!(re.is_match(&time), "relative time, got {time}");
    assert!(k.element("events.live").unwrap().selected);
    assert!(k.element("events.history").is_some());
    // Only this pod's events were asked for.
    assert!(k.cluster.requests().iter().any(|r| r.path == "/api/v1/namespaces/kore-demo/events"
        && r.query.get("fieldSelector").is_some_and(|f| f.contains("involvedObject.name=kore-events-probe"))));

    k.click("events.history");
    k.wait_until("history mode", |k| k.element("events.history").is_some_and(|e| e.selected));
    k.wait_for_element("events.range.1h");
    assert!(k.element("events.error").is_none());
}

/// Scenario 20-details-fix-events-history (Fix 10): events seen by the Events
/// tab are written to the SQLite store, so History (1h active; 6h / 24h / 7d
/// offered) lists Scheduled / Started for the pod. Regression symptom: nothing
/// is stored and History shows "No events found".
#[gpui_kit::test]
fn history_lists_events_from_the_store(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    seed_pod_with_events(&k, "kore-history-probe");
    k.open_pod("kore-history-probe");
    k.keys("5");
    k.wait_until("live cards", |k| cards(k).len() == 4);
    // The cluster forgets the events: History must come from the store.
    for reason in ["scheduled", "pulled", "created", "started"] {
        k.cluster.delete("Event", Some(NS), &format!("kore-history-probe.{reason}"));
    }
    k.click("events.history");
    k.wait_until("1h selected", |k| k.element("events.range.1h").is_some_and(|e| e.selected));
    for r in ["6h", "24h", "7d"] {
        assert!(k.element(&format!("events.range.{r}")).is_some_and(|e| !e.selected), "{r}");
    }
    k.wait_until("stored cards", |k| {
        let c = cards(k);
        c.iter().any(|l| l.starts_with("Scheduled:")) && c.iter().any(|l| l.starts_with("Started:"))
    });
    assert!(k.element("events.empty").is_none());
    // The store is on disk in the sandbox's data dir.
    assert!(std::fs::read_dir(k.env.data_dir()).unwrap().flatten().any(|f| f.file_name().to_string_lossy().ends_with(".db")));
}

/// Scenario 20-details-bug-events-toggle: the Live / History toggle is always
/// rendered, also in the "No events found" state: Live (empty) shows it,
/// History (empty) shows the ranges and the toggle, and Live can be selected
/// again.
#[gpui_kit::test]
fn empty_events_keep_the_live_history_toggle(cx: &mut TestAppContext) {
    let mut k = Kore::boot(cx);
    k.cluster.apply(json!({"apiVersion": "v1", "kind": "ConfigMap",
        "metadata": {"name": "kore-bug-events-cm", "namespace": NS}, "data": {"k": "v"}}));
    k.open(ResourceKind::Configmaps, "kore-bug-events-cm");
    k.keys("3");
    k.wait_label("events.empty", "No events found", |l| l == "No events found");
    assert!(k.element("events.live").is_some_and(|e| e.selected));
    assert!(k.element("events.history").is_some());
    k.click("events.history");
    k.wait_for_element("events.range.1h");
    k.wait_for_element("events.empty");
    for id in ["events.live", "events.history", "events.range.6h", "events.range.24h", "events.range.7d"] {
        assert!(k.element(id).is_some(), "{id} missing");
    }
    k.click("events.live");
    k.wait_gone("events.range.24h");
    k.wait_for_element("events.empty");
    assert!(k.element("events.live").is_some_and(|e| e.selected) && k.element("events.history").is_some());
    k.keys("escape");
    k.wait_until("table", |k| k.state()["view"] == "table");
}
