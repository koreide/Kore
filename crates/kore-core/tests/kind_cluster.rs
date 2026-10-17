//! Dashboard, event store, CRDs, graph, Helm, multi-cluster, RBAC and
//! network policies against kind.
mod common;

use common::*;
use kore_core::state::rbac::RbacIdentity;
use kore_core::{Core, CoreConfig, ResourceKind};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn dashboard_health() {
    require_kind!();
    let core = core();
    let h = block_on(core.get_cluster_health()).unwrap();
    assert!(h.score < 100, "score {}", h.score);
    assert!(h.pods.total > 0);
    assert!(h.pods.crash_looping >= 1 || h.restart_hotlist.iter().any(|r| r.name == "crashloop"));
    assert!(h.restart_hotlist.iter().any(|r| r.name == "crashloop"));
    assert!(h.pending_pods.iter().any(|p| p.name == "unschedulable"));
    assert!(!h.recent_warnings.is_empty());
    assert!(!h.nodes.is_empty());
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn event_store_written_by_list_and_watch_and_pruned() {
    require_kind!();
    let core = core();
    let now = chrono::Utc::now();
    let since = (now - chrono::Duration::days(30)).to_rfc3339();
    let until = (now + chrono::Duration::hours(1)).to_rfc3339();
    block_on(async {
        // Listing events persists them
        let listed = core
            .list_resources(ResourceKind::Events, Some(DEMO_NS.into()), None)
            .await
            .unwrap();
        assert!(!listed.is_empty());
        let mut stored = Vec::new();
        for _ in 0..20 {
            stored = core
                .query_stored_events(since.clone(), until.clone(), Some(DEMO_NS.into()))
                .await
                .unwrap();
            if !stored.is_empty() {
                break;
            }
            sleep(&core, Duration::from_millis(250)).await;
        }
        assert!(!stored.is_empty(), "list did not persist events");
        assert!(stored
            .iter()
            .all(|e| e.context == CTX && e.namespace == DEMO_NS));
        assert!(stored.iter().any(|e| e.involved_object == "Pod/crashloop"));
    });

    // A fresh store fed only by an Events watch
    let core2 = common::core();
    block_on(async {
        let mut w = core2
            .start_watch(ResourceKind::Events, Some(DEMO_NS.into()), None)
            .await
            .unwrap();
        assert!(
            recv_until(&core2, &mut w, Duration::from_secs(20), |e| e.action
                == "applied")
            .await
            .is_some()
        );
        let mut n = 0;
        for _ in 0..20 {
            n = core2
                .query_stored_events(since.clone(), until.clone(), None)
                .await
                .unwrap()
                .len();
            if n > 0 {
                break;
            }
            sleep(&core2, Duration::from_millis(250)).await;
        }
        assert!(n > 0, "watch did not persist events");
        drop(w);

        // Retention: 0 days prunes everything older than now
        let pruned = core2.set_event_retention(0).await.unwrap();
        assert!(pruned > 0);
        let left = core2
            .query_stored_events(since.clone(), until.clone(), None)
            .await
            .unwrap();
        assert!(left.len() < n);
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn crd_discovery_and_instances() {
    require_kind!();
    let core = core();
    let ns = TempNamespace::new("kore-core-it");
    block_on(async {
        let crds = core.list_crds().await.unwrap();
        let w = crds
            .iter()
            .find(|c| c.name == "widgets.kore.dev")
            .expect("widgets CRD missing");
        assert_eq!(w.group, "kore.dev");
        assert_eq!(w.version, "v1");
        assert_eq!(w.kind, "Widget");
        assert_eq!(w.plural, "widgets");
        assert_eq!(w.scope, "Namespaced");

        let items = core
            .list_crd_instances(
                "kore.dev".into(),
                "v1".into(),
                "widgets".into(),
                Some(DEMO_NS.into()),
            )
            .await
            .unwrap();
        let n = names(&items);
        assert!(n.contains(&"blue-widget".to_string()) && n.contains(&"red-widget".to_string()));
        let all = core
            .list_crd_instances("kore.dev".into(), "v1".into(), "widgets".into(), None)
            .await
            .unwrap();
        assert!(all.len() >= 2);
        let blue = core
            .get_crd_instance(
                "kore.dev".into(),
                "v1".into(),
                "widgets".into(),
                DEMO_NS.into(),
                "blue-widget".into(),
            )
            .await
            .unwrap();
        assert_eq!(blue["kind"], "Widget");
        assert!(blue["spec"]["color"].is_string());
    });
    // Delete on a private instance
    kubectl_apply(&format!(
        "apiVersion: kore.dev/v1\nkind: Widget\nmetadata: {{name: tmp-widget, namespace: {}}}\nspec: {{color: green, size: 1}}\n",
        ns.name
    ));
    block_on(async {
        core.delete_crd_instance(
            "kore.dev".into(),
            "v1".into(),
            "widgets".into(),
            ns.name.clone(),
            "tmp-widget".into(),
        )
        .await
        .unwrap();
        assert!(core
            .get_crd_instance(
                "kore.dev".into(),
                "v1".into(),
                "widgets".into(),
                ns.name.clone(),
                "tmp-widget".into()
            )
            .await
            .is_err());
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn resource_graph_for_web() {
    require_kind!();
    let core = core();
    let g = block_on(core.build_resource_graph(Some(DEMO_NS.into()))).unwrap();
    let dep = "Deployment/kore-demo/web";
    assert!(g.nodes.iter().any(|n| n.id == dep));
    let rs: Vec<&str> = g
        .edges
        .iter()
        .filter(|e| e.source == dep && e.relation == "owns")
        .map(|e| e.target.as_str())
        .collect();
    assert!(!rs.is_empty(), "no Deployment→ReplicaSet edges");
    assert!(
        g.edges
            .iter()
            .any(|e| rs.contains(&e.source.as_str()) && e.target.starts_with("Pod/kore-demo/web-")),
        "no ReplicaSet→Pod edges"
    );
    assert!(g.edges.iter().any(|e| e.source == "Service/kore-demo/web"
        && e.relation == "selects"
        && e.target.starts_with("Pod/kore-demo/web-")));
    assert!(g
        .edges
        .iter()
        .any(|e| e.source == "Ingress/kore-demo/web" && e.relation == "routes"));
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn helm_release_read_only() {
    require_kind!();
    let core = core();
    block_on(async {
        assert!(core.helm_available().await.unwrap(), "helm not on PATH");
        let rels = core.list_helm_releases(Some(DEMO_NS.into())).await.unwrap();
        let r = rels
            .iter()
            .find(|r| r.name == "kore-demo")
            .expect("release missing");
        assert_eq!(r.status, "deployed");
        assert!(r.chart.starts_with("kore-demo-"));
        let all = core.list_helm_releases(None).await.unwrap();
        assert!(all.iter().any(|r| r.name == "kore-demo"));

        // Deliberate Fix 8: integer revisions parse
        let hist = core
            .get_helm_history("kore-demo".into(), DEMO_NS.into())
            .await
            .unwrap();
        assert!(hist.len() >= 2, "{hist:?}");
        assert!(hist.iter().all(|h| h.revision.parse::<u32>().is_ok()));
        assert!(hist.iter().any(|h| h.revision == "1"));

        let values = core
            .get_helm_values("kore-demo".into(), DEMO_NS.into())
            .await
            .unwrap();
        assert!(values.contains("message"), "{values}");
        let manifest = core
            .get_helm_manifest("kore-demo".into(), DEMO_NS.into())
            .await
            .unwrap();
        assert!(manifest.contains("kind:"), "{manifest}");

        assert!(core
            .get_helm_values("no-such-release".into(), DEMO_NS.into())
            .await
            .is_err());
    });
}

fn kubeconfig_with_unreachable_context(dir: &std::path::Path) -> std::path::PathBuf {
    // 10.255.255.1 is non-routable: connects hang until a timeout fires.
    let p = dir.join("unreachable.yaml");
    std::fs::write(
        &p,
        r#"apiVersion: v1
kind: Config
clusters:
- name: blackhole
  cluster: {server: "https://10.255.255.1:6443", insecure-skip-tls-verify: true}
users:
- name: blackhole
  user: {token: nope}
contexts:
- name: blackhole
  context: {cluster: blackhole, user: blackhole}
"#,
    )
    .unwrap();
    p
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn multi_cluster_listing_with_timeout() {
    require_kind!();
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_config(CoreConfig {
        kubeconfig_paths: vec![
            kubeconfig(),
            kubeconfig_with_unreachable_context(dir.path()),
        ],
        initial_context: Some(CTX.into()),
        data_dir: Some(dir.path().join("data")),
        favorites_dir: Some(dir.path().join("kore")),
        ..Default::default()
    })
    .unwrap();
    block_on(async {
        let ctxs = core.list_contexts().await.unwrap();
        assert_eq!(ctxs.len(), 3, "{ctxs:?}");

        // Deliberate Fix 9: one unreachable context costs at most ~5s
        let t = Instant::now();
        let items = core
            .list_resources_multi_cluster(ResourceKind::Deployments, None, None)
            .await
            .unwrap();
        let took = t.elapsed();
        assert!(took < Duration::from_secs(9), "took {took:?}");
        assert!(items
            .iter()
            .any(|i| i["_context"] == CTX && i["metadata"]["name"] == "web"));
        assert!(items
            .iter()
            .any(|i| i["_context"] == CTX2 && i["metadata"]["name"] == "remote-web"));
        assert!(!items.iter().any(|i| i["_context"] == "blackhole"));

        let t = Instant::now();
        let health = core.get_cluster_health_multi_cluster().await.unwrap();
        assert!(t.elapsed() < Duration::from_secs(9));
        let names: Vec<&str> = health.clusters.iter().map(|c| c.context.as_str()).collect();
        assert!(names.contains(&CTX) && names.contains(&CTX2), "{names:?}");
        assert!(!names.contains(&"blackhole"));
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn switch_context_cancels_streams() {
    require_kind!();
    let core = core();
    block_on(async {
        let mut w = core
            .start_watch(ResourceKind::Pods, Some(DEMO_NS.into()), None)
            .await
            .unwrap();
        assert!(recv_until(&core, &mut w, Duration::from_secs(20), |_| true)
            .await
            .is_some());
        let logger = running_pod(DEMO_NS, "app=logger");
        let mut logs = core
            .start_pod_logs_stream(DEMO_NS.into(), logger, None, None)
            .await
            .unwrap();

        let st = core.switch_context(CTX2.into()).await.unwrap();
        assert_eq!(st.current_context.as_deref(), Some(CTX2));
        assert!(st.connected, "{:?}", st.error);
        assert!(w.is_cancelled() && logs.is_cancelled());
        let drained = timeout(
            &core,
            Duration::from_secs(5),
            Box::pin(async {
                while w.recv().await.is_some() {}
                while logs.recv().await.is_some() {}
            }),
        )
        .await;
        assert!(
            drained.is_some(),
            "streams kept running after context switch"
        );

        let st = core.get_connection_status().await.unwrap();
        assert!(st.connected);
        assert_eq!(st.current_context.as_deref(), Some(CTX2));
        let nss = core.list_namespaces().await.unwrap();
        assert!(nss.contains(&"kore-remote".to_string()));

        // New watches work in the new context
        let mut w2 = core
            .start_watch(ResourceKind::Deployments, Some("kore-remote".into()), None)
            .await
            .unwrap();
        let ev = recv_until(&core, &mut w2, Duration::from_secs(20), |_| true)
            .await
            .unwrap();
        assert_eq!(ev.object["metadata"]["name"], "remote-web");
    });
}

fn viewer_bot() -> RbacIdentity {
    RbacIdentity::ServiceAccount {
        name: "viewer-bot".into(),
        namespace: "kore-rbac".into(),
    }
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn rbac_functions() {
    require_kind!();
    let core = core();
    block_on(async {
        let ok = core
            .rbac_check_permission(
                viewer_bot(),
                "list".into(),
                "pods".into(),
                Some(DEMO_NS.into()),
            )
            .await
            .unwrap();
        assert!(ok.allowed, "{}", ok.summary);
        assert!(ok.rule_chain.iter().any(|r| r.role_name == "pod-reader"));
        let denied = core
            .rbac_check_permission(
                viewer_bot(),
                "delete".into(),
                "pods".into(),
                Some(DEMO_NS.into()),
            )
            .await
            .unwrap();
        assert!(!denied.allowed);

        let deployer = core
            .rbac_check_permission(
                RbacIdentity::User {
                    name: "alice@kore.dev".into(),
                },
                "update".into(),
                "deployments".into(),
                Some(DEMO_NS.into()),
            )
            .await
            .unwrap();
        // Whatever kore-deployer grants, alice gets via the ClusterRoleBinding.
        let _ = deployer.allowed;

        let matrix = core
            .rbac_build_matrix(viewer_bot(), Some(DEMO_NS.into()))
            .await
            .unwrap();
        let pods_row = matrix
            .rows
            .iter()
            .find(|r| r.resource == "pods")
            .expect("pods row");
        let get = serde_json::to_value(&pods_row.verbs["get"]).unwrap();
        assert_eq!(get["status"], "allowed");

        let rev = core
            .rbac_reverse_lookup("Role".into(), "pod-reader".into(), Some(DEMO_NS.into()))
            .await
            .unwrap();
        assert!(rev.subjects.iter().any(|s| s.identity == viewer_bot()));

        let ids = core.rbac_list_identities().await.unwrap();
        assert!(ids.service_accounts.iter().any(|s| s.name == "viewer-bot"));
        assert!(ids.users.contains(&"alice@kore.dev".to_string()));
        assert!(ids.groups.contains(&"kore-devs".to_string()));

        let roles = core.rbac_list_roles(Some(DEMO_NS.into())).await.unwrap();
        assert!(roles.iter().any(|r| r.name == "pod-reader"));

        let who = core
            .rbac_who_can("list".into(), "pods".into(), Some(DEMO_NS.into()))
            .await
            .unwrap();
        assert!(who.iter().any(|r| r.identity == viewer_bot() && r.allowed));

        let fa = core
            .rbac_analyze_forbidden(
                r#"pods "x" is forbidden: User "system:serviceaccount:kore-rbac:viewer-bot" cannot delete resource "pods" in API group "" in the namespace "kore-demo""#
                    .into(),
            )
            .await
            .unwrap();
        assert_eq!(fa.verb, "delete");
        assert_eq!(fa.resource, "pods");
        assert_eq!(fa.namespace.as_deref(), Some(DEMO_NS));
        assert_eq!(fa.identity, viewer_bot());

        let nl = core
            .rbac_natural_language_query("can viewer-bot delete pods in kore-demo".into(), None)
            .await
            .unwrap();
        assert_eq!(nl.parsed_verb.as_deref(), Some("delete"));
        assert_eq!(nl.parsed_resource.as_deref(), Some("pods"));
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn network_policy_graph_and_simulation() {
    require_kind!();
    let core = core();
    let logger = running_pod(DEMO_NS, "app=logger");
    let web = running_pod(DEMO_NS, "app=web");
    block_on(async {
        let g = core
            .build_network_policy_graph(Some(DEMO_NS.into()))
            .await
            .unwrap();
        let pols: Vec<&str> = g.policies.iter().map(|p| p.name.as_str()).collect();
        assert!(
            pols.contains(&"default-deny-ingress") && pols.contains(&"allow-logger-to-web"),
            "{pols:?}"
        );
        assert!(!g.groups.is_empty());
        assert!(!g.edges.is_empty());

        let allowed = core
            .simulate_network_traffic(
                DEMO_NS.into(),
                logger.clone(),
                DEMO_NS.into(),
                web.clone(),
                Some(80),
                None,
            )
            .await
            .unwrap();
        assert!(allowed.allowed, "{}", allowed.summary);
        assert!(allowed.ingress_evaluation.isolated);

        let wrong_port = core
            .simulate_network_traffic(
                DEMO_NS.into(),
                logger.clone(),
                DEMO_NS.into(),
                web.clone(),
                Some(8080),
                None,
            )
            .await
            .unwrap();
        assert!(!wrong_port.allowed, "{}", wrong_port.summary);

        let denied = core
            .simulate_network_traffic(
                DEMO_NS.into(),
                "multi-container".into(),
                DEMO_NS.into(),
                web,
                Some(80),
                None,
            )
            .await
            .unwrap();
        assert!(!denied.allowed, "{}", denied.summary);
    });
}
