//! Port-forward sidebar. The tunnel itself (kubectl's bridge) is not faked:
//! `kubectl` is a sandbox script that records its argv and exits, so these
//! tests assert the command Core runs, the panel state and the local listener.

use super::helpers::DetailsExt;
use crate::testing::Kore;
use gpui_kit::TestAppContext;
use kore_testkit::{fixtures, FakeEnv};
use std::net::TcpStream;

const WEB_POD: &str = "web-7d86f4f56d-65wlk";

/// A free local port (bound and released).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The harness world plus a fake `kubectl` that records argv (and the
/// KUBECONFIG it got) and exits 1, like a refused port-forward.
fn boot_with_kubectl(cx: &mut TestAppContext, current: &str) -> Kore {
    let (env, c1, c2) = fixtures::harness_env();
    let kc = env.dir.path().join("kubectl-kubeconfig");
    let env: FakeEnv = env
        .tool("kubectl", &format!("printf '%s' \"$KUBECONFIG\" > '{}'\necho 'fake kubectl: no tunnel' >&2\nexit 1", kc.display()))
        .current(current);
    Kore::boot_env(cx, env, c1, c2)
}

fn open_ports(k: &mut Kore) {
    k.open_pod(WEB_POD);
    k.click("pod.ports");
    k.wait_until("ports open", |k| k.element("pod.ports").is_some_and(|e| e.selected));
    // The sidebar slides open (0 -> 320px, 200ms).
    k.wait_until("panel fully open", |k| {
        k.advance(50);
        k.element("view.port-forward").is_some_and(|e| e.bounds.x + e.bounds.w <= 1400.5)
    });
}

fn set_input(k: &mut Kore, id: &str, value: &str) {
    k.click(id);
    k.wait_text_entry();
    k.keys("cmd-a");
    k.type_text(value);
    k.wait_label(id, value, |l| l == value);
}

/// Connect to the forwarded port; Core then spawns `kubectl port-forward`.
/// Returns that kubectl call.
fn kubectl_call_for(k: &mut Kore, port: u16) -> String {
    let _conn = TcpStream::connect(("127.0.0.1", port)).expect("local listener");
    let mut call = String::new();
    k.wait_until("kubectl port-forward call", |k| match k.env.tool_calls().into_iter().find(|c| c.starts_with("kubectl port-forward")) {
        Some(c) => {
            call = c;
            true
        }
        None => false,
    });
    call
}

/// Scenario 20-details-portforward: Ports opens the panel (Pod Port 80 / Local
/// Port 8080, "No active port forwards"); forwarding 80 -> P shows a LIVE card
/// (127.0.0.1:P, <pod>:80, Connected) and toasts; a connection runs
/// `kubectl port-forward pod/<pod> <tmp>:80 -n kore-demo`; the inputs reset; a
/// duplicate local port and an out-of-range pod port are rejected; stopping
/// removes the card and closes the port. (Real traffic through kubectl is Layer 2.)
#[gpui_kit::test]
fn port_forward_start_validate_and_stop(cx: &mut TestAppContext) {
    let mut k = boot_with_kubectl(cx, fixtures::CONTEXT);
    open_ports(&mut k);
    assert_eq!(k.label("pf.podport").as_deref(), Some("80"));
    assert_eq!(k.label("pf.localport").as_deref(), Some("8080"));
    assert!(k.element("pf.empty").is_some());

    let port = free_port();
    set_input(&mut k, "pf.localport", &port.to_string());
    k.click("pf.start");
    k.wait_toast("active", |l| l == format!("Port forward active on :{port}"));
    k.wait_label(&format!("pf.card.{port}"), "LIVE", |l| l == "LIVE");
    assert_eq!(k.label(&format!("pf.link.{port}")), Some(format!("127.0.0.1:{port}")));
    assert_eq!(k.label(&format!("pf.card.{port}.pod")), Some(format!("{WEB_POD}:80")));
    assert_eq!(k.label(&format!("pf.card.{port}.message")).as_deref(), Some("Connected"));
    assert!(k.element("pf.empty").is_none());

    let call = kubectl_call_for(&mut k, port);
    let argv: Vec<&str> = call.split(' ').collect();
    assert_eq!(argv[..3], ["kubectl", "port-forward", &format!("pod/{WEB_POD}")], "{call}");
    assert!(argv[3].ends_with(":80"), "{call}");
    assert_eq!(argv[4..6], ["-n", "kore-demo"], "{call}");

    // Inputs reset after success; a duplicate local port is rejected.
    k.wait_label("pf.localport", "reset", |l| l == "8080");
    assert_eq!(k.label("pf.podport").as_deref(), Some("80"));
    set_input(&mut k, "pf.localport", &port.to_string());
    k.click("pf.start");
    k.wait_label("pf.error", "duplicate", |l| l == format!("Port {port} is already in use by another port forward"));
    // Pod port out of range.
    set_input(&mut k, "pf.podport", "70000");
    k.click("pf.start");
    k.wait_label("pf.error", "pod port", |l| l == "Pod port must be a valid number between 1 and 65535");
    set_input(&mut k, "pf.podport", "80");
    assert!(k.element("pf.error").is_none(), "editing clears the error");

    // Stop
    k.click(&format!("pf.stop.{port}"));
    k.wait_for_element("pf.empty");
    k.wait_gone(&format!("pf.card.{port}"));
    k.wait_until("port closed", |_| TcpStream::connect(("127.0.0.1", port)).is_err());
}

/// Scenario 20-details-fix-portforward-context (Fix 6): port-forward passes
/// `--context` for the context selected in the app. The kubeconfig's current
/// context is kind-kore-harness-2; the user selects kind-kore-harness, and the
/// kubectl call targets it (and Kore's kubeconfig), not kubectl's current context.
#[gpui_kit::test]
fn port_forward_targets_the_selected_context(cx: &mut TestAppContext) {
    let mut k = boot_with_kubectl(cx, fixtures::CONTEXT_2);
    k.wait_until("booted on ctx 2", |k| k.state()["context"] == fixtures::CONTEXT_2);
    k.update_app_state(|m, cx| m.switch_context(fixtures::CONTEXT.into(), cx));
    k.wait_until("ctx 1 connected", |k| k.state()["context"] == fixtures::CONTEXT && k.state()["connected"] == true);
    open_ports(&mut k);
    let port = free_port();
    set_input(&mut k, "pf.localport", &port.to_string());
    k.click("pf.start");
    k.wait_label(&format!("pf.card.{port}"), "LIVE", |l| l == "LIVE");
    let call = kubectl_call_for(&mut k, port);
    assert!(call.ends_with(&format!("-n kore-demo --context {}", fixtures::CONTEXT)), "{call}");
    let kubeconfig = std::fs::read_to_string(k.env.dir.path().join("kubectl-kubeconfig")).unwrap();
    assert_eq!(kubeconfig, k.env.kubeconfig_path().to_string_lossy());
    k.click(&format!("pf.stop.{port}"));
    k.wait_for_element("pf.empty");
}
