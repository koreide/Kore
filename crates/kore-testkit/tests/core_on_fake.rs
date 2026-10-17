//! kore-core end to end against the fake cluster (no real cluster, no network).

use kore_core::{Core, ResourceKind};
use kore_testkit::fixtures::{harness_env, CONTEXT};
use serde_json::json;
use std::time::Duration;

fn block<F: std::future::Future>(f: F) -> F::Output {
    futures::executor::block_on(f)
}

#[test]
fn lists_and_watches_pods() {
    let (env, cluster, _) = harness_env();
    let core = Core::with_config(env.core_config()).unwrap();
    block(core.ready());
    let status = block(core.get_connection_status()).unwrap();
    assert!(status.connected, "{status:?}");
    assert_eq!(status.current_context.as_deref(), Some(CONTEXT));

    let pods = block(core.list_resources(ResourceKind::Pods, Some("kore-demo".into()), None)).unwrap();
    assert!(pods.iter().any(|p| p["name"] == "crashloop" || p["metadata"]["name"] == "crashloop"), "{:?}", pods.first());

    let mut sub = block(core.start_watch(ResourceKind::Pods, Some("kore-demo".into()), None)).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    cluster.apply(json!({
        "apiVersion": "v1", "kind": "Pod",
        "metadata": {"name": "watch-probe", "namespace": "kore-demo"},
        "spec": {"containers": [{"name": "c", "image": "busybox"}]},
        "status": {"phase": "Pending"},
    }));
    let ev = block(async {
        loop {
            let ev = sub.recv().await.expect("watch ended");
            if ev.object.to_string().contains("watch-probe") {
                return ev;
            }
        }
    });
    assert_eq!(ev.context, CONTEXT);
}

#[test]
fn list_pages_through_continue_tokens() {
    let (env, cluster, _) = harness_env();
    for i in 0..1203 {
        cluster.apply(json!({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": format!("cm-{i:04}"), "namespace": "paging"}}));
    }
    let core = Core::with_config(env.core_config()).unwrap();
    let list = block(core.list_resources_full(ResourceKind::Configmaps, Some("paging".into()), None)).unwrap();
    assert_eq!(list.items.len(), 1203);
    assert!(!list.truncated);
    let pages = cluster.requests().iter().filter(|r| r.path.ends_with("/namespaces/paging/configmaps")).count();
    assert!(pages >= 3, "expected paged requests, got {pages}");
}

#[test]
fn tools_and_endpoints_are_sandboxed() {
    let (env, _, _) = harness_env();
    let env = env.tool("helm", "echo '[]'");
    let core = Core::with_config(env.core_config()).unwrap();
    block(core.ready());
    let releases = block(core.list_helm_releases(None)).unwrap();
    assert!(releases.is_empty());
    assert!(env.tool_calls().iter().any(|c| c.starts_with("helm list")), "{:?}", env.tool_calls());
    // No real claude/agent CLI and no real Ollama are reachable from a sandboxed Core.
    let providers = block(core.check_providers_availability(None)).unwrap();
    assert!(providers.values().all(|v| !v), "{providers:?}");
}

#[test]
fn fake_helm_replays_the_seeded_release() {
    let (env, _, _) = harness_env();
    let env = kore_testkit::fixtures::with_helm(env);
    let core = Core::with_config(env.core_config()).unwrap();
    let releases = block(core.list_helm_releases(None)).unwrap();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].name, "kore-demo");
}
