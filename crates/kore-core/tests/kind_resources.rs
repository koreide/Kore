//! Resources, watch, describe, YAML, rollback, scale/restart against kind.
mod common;

use common::*;
use kore_core::ResourceKind;
use std::time::Duration;

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn connection_and_contexts() {
    require_kind!();
    let core = core();
    block_on(async {
        core.ready().await;
        let st = core.get_connection_status().await.unwrap();
        assert!(st.connected, "not connected: {:?}", st.error);
        assert_eq!(st.current_context.as_deref(), Some(CTX));
        assert!(st.contexts_available.contains(&CTX2.to_string()));
        let ctxs = core.list_contexts().await.unwrap();
        assert!(ctxs.contains(&CTX.to_string()) && ctxs.contains(&CTX2.to_string()));
        let nss = core.list_namespaces().await.unwrap();
        assert!(nss.contains(&DEMO_NS.to_string()));
        // retry_connection keeps us connected
        let st = core.retry_connection(Some(CTX.into())).await.unwrap();
        assert!(st.connected);
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn list_every_kind() {
    require_kind!();
    let core = core();
    let ns = Some(DEMO_NS.to_string());
    block_on(async {
        let expect: &[(ResourceKind, Option<&str>)] = &[
            (ResourceKind::Pods, Some("multi-container")),
            (ResourceKind::Deployments, Some("web")),
            (ResourceKind::Services, Some("web")),
            (ResourceKind::Nodes, None),
            (ResourceKind::Events, None),
            (ResourceKind::Configmaps, Some("web-config")),
            (ResourceKind::Secrets, Some("web-secret")),
            (ResourceKind::Ingresses, Some("web")),
            (ResourceKind::Jobs, Some("one-shot")),
            (ResourceKind::Cronjobs, Some("ticker")),
            (ResourceKind::Namespaces, Some(DEMO_NS)),
        ];
        for (kind, name) in expect {
            let ns = if kind.is_cluster_scoped() {
                None
            } else {
                ns.clone()
            };
            let items = core.list_resources(*kind, ns, None).await.unwrap();
            assert!(!items.is_empty(), "{kind:?}: empty list");
            if let Some(n) = name {
                assert!(
                    names(&items).contains(&n.to_string()),
                    "{kind:?}: {n} missing"
                );
            }
        }
        // Label selector filters
        let loggers = core
            .list_resources(ResourceKind::Pods, ns.clone(), Some("app=logger".into()))
            .await
            .unwrap();
        assert_eq!(loggers.len(), 2);
        assert!(names(&loggers).iter().all(|n| n.starts_with("logger-")));
        // All namespaces
        let all = core
            .list_resources(ResourceKind::Pods, None, None)
            .await
            .unwrap();
        assert!(all.len() > loggers.len());
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn describe_events_search() {
    require_kind!();
    let core = core();
    block_on(async {
        let d = core
            .describe_resource(ResourceKind::Deployments, DEMO_NS.into(), "web".into())
            .await
            .unwrap();
        assert_eq!(d["metadata"]["name"], "web");
        assert_eq!(d["spec"]["replicas"], 3);
        let p = core
            .describe_pod(DEMO_NS.into(), "multi-container".into())
            .await
            .unwrap();
        assert_eq!(p["spec"]["containers"].as_array().unwrap().len(), 2);
        let evs = core
            .list_events_for_resource("pods".into(), DEMO_NS.into(), "crashloop".into())
            .await
            .unwrap();
        assert!(!evs.is_empty(), "crashloop pod should have events");
        let hits = core
            .search_resources("LOGGER".into(), Some(DEMO_NS.into()))
            .await
            .unwrap();
        assert!(hits.iter().any(|h| h["_kind"] == "deployments"));
        assert!(hits.iter().any(|h| h["_kind"] == "pods"));
        // Validation errors
        assert!(core.search_resources(String::new(), None).await.is_err());
        assert!(core
            .describe_resource(ResourceKind::Pods, DEMO_NS.into(), String::new())
            .await
            .is_err());
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn watch_receives_add_and_delete_with_concurrent_subscriptions() {
    require_kind!();
    let core = core();
    let ns = TempNamespace::new("kore-core-it");
    block_on(async {
        let mut w1 = core
            .start_watch(ResourceKind::Configmaps, Some(ns.name.clone()), None)
            .await
            .unwrap();
        let mut w2 = core
            .start_watch(ResourceKind::Configmaps, Some(ns.name.clone()), None)
            .await
            .unwrap();
        assert_ne!(w1.id, w2.id);
        // Initial list: kube-root-ca.crt arrives as "applied"
        let first = recv_until(&core, &mut w1, Duration::from_secs(20), |e| {
            e.action == "applied"
        })
        .await;
        assert!(first.is_some(), "no initial applied event");

        let (ok, out) = kubectl(&[
            "-n",
            &ns.name,
            "create",
            "configmap",
            "watched",
            "--from-literal=a=1",
        ]);
        assert!(ok, "{out}");
        for w in [&mut w1, &mut w2] {
            let ev = recv_until(&core, w, Duration::from_secs(20), |e| {
                e.action == "applied" && e.object["metadata"]["name"] == "watched"
            })
            .await;
            assert!(ev.is_some(), "missing applied event for created configmap");
            assert_eq!(ev.unwrap().kind, "configmaps");
        }

        // Dropping one subscription must not affect the other.
        drop(w2);
        let (ok, out) = kubectl(&["-n", &ns.name, "delete", "configmap", "watched"]);
        assert!(ok, "{out}");
        let ev = recv_until(&core, &mut w1, Duration::from_secs(20), |e| {
            e.action == "deleted" && e.object["metadata"]["name"] == "watched"
        })
        .await;
        assert!(ev.is_some(), "missing deleted event");

        // Explicit cancel ends the stream.
        w1.cancel();
        let end = timeout(
            &core,
            Duration::from_secs(5),
            Box::pin(async { while w1.recv().await.is_some() {} }),
        )
        .await;
        assert!(end.is_some(), "stream did not end after cancel");
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn yaml_get_diff_apply_and_delete() {
    require_kind!();
    let core = core();
    let ns = TempNamespace::new("kore-core-it");
    let (ok, out) = kubectl(&[
        "-n",
        &ns.name,
        "create",
        "configmap",
        "editable",
        "--from-literal=color=blue",
    ]);
    assert!(ok, "{out}");
    block_on(async {
        let yaml = core
            .get_resource_yaml(ResourceKind::Configmaps, ns.name.clone(), "editable".into())
            .await
            .unwrap();
        assert!(!yaml.contains("managedFields"));
        assert!(yaml.contains("color: blue"));

        let edited = yaml.replace("color: blue", "color: green");
        let diff = core
            .diff_resource_yaml(
                ResourceKind::Configmaps,
                ns.name.clone(),
                "editable".into(),
                edited.clone(),
            )
            .await
            .unwrap();
        assert!(diff
            .iter()
            .any(|l| l.tag == "delete" && l.value.contains("color: blue")));
        assert!(diff
            .iter()
            .any(|l| l.tag == "insert" && l.value.contains("color: green")));
        assert!(diff.iter().any(|l| l.tag == "equal"));

        // Apply a minimal manifest (SSA)
        let manifest = format!(
            "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: editable\n  namespace: {}\ndata:\n  color: green\n",
            ns.name
        );
        let msg = core
            .apply_resource_yaml(
                ResourceKind::Configmaps,
                ns.name.clone(),
                "editable".into(),
                manifest,
            )
            .await
            .unwrap();
        assert_eq!(msg, "Applied successfully");
        let live = core
            .describe_resource(ResourceKind::Configmaps, ns.name.clone(), "editable".into())
            .await
            .unwrap();
        assert_eq!(live["data"]["color"], "green");
        let fm = live["metadata"]["managedFields"].as_array().unwrap();
        assert!(fm.iter().any(|f| f["manager"] == "kore"));

        // Unsupported kinds
        assert!(core
            .apply_resource_yaml(
                ResourceKind::Nodes,
                String::new(),
                "x".into(),
                "a: 1".into()
            )
            .await
            .is_err());

        // Secret round-trip keeps base64 data
        let secret = core
            .get_resource_yaml(ResourceKind::Secrets, DEMO_NS.into(), "web-secret".into())
            .await
            .unwrap();
        assert!(secret.contains("API_TOKEN"));

        // delete_resource
        core.delete_resource(ResourceKind::Configmaps, ns.name.clone(), "editable".into())
            .await
            .unwrap();
        let (_, out) = kubectl(&["-n", &ns.name, "get", "configmap", "editable"]);
        assert!(out.contains("NotFound"), "{out}");
    });
}

fn rollout_deployment_yaml(ns: &str) -> String {
    format!(
        r#"apiVersion: apps/v1
kind: Deployment
metadata: {{name: rb, namespace: {ns}}}
spec:
  replicas: 0
  selector: {{matchLabels: {{app: rb}}}}
  template:
    metadata: {{labels: {{app: rb}}}}
    spec:
      containers: [{{name: c, image: "busybox:1.36", command: ["sleep", "3600"]}}]
"#
    )
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn rollback_history_and_restore() {
    require_kind!();
    let core = core();
    block_on(async {
        // Read-only: the fixture deployment has >= 3 revisions with nginx images.
        let revs = core
            .list_deployment_revisions(DEMO_NS.into(), "web".into())
            .await
            .unwrap();
        assert!(revs.len() >= 3, "web has {} revisions", revs.len());
        assert!(
            revs.windows(2).all(|w| w[0].revision > w[1].revision),
            "not sorted desc"
        );
        assert!(revs.iter().all(|r| r.image.starts_with("nginx:")));
        let y = core
            .get_revision_yaml(DEMO_NS.into(), revs[0].name.clone())
            .await
            .unwrap();
        assert!(y.contains("kind: ReplicaSet") || y.contains("ReplicaSet"));
    });

    // Mutating rollback on a private deployment (replicas 0, no pods).
    let ns = TempNamespace::new("kore-core-it");
    kubectl_apply(&rollout_deployment_yaml(&ns.name));
    for tag in ["1.36.0", "1.36.1"] {
        let (ok, out) = kubectl(&[
            "-n",
            &ns.name,
            "set",
            "image",
            "deploy/rb",
            &format!("c=busybox:{tag}"),
        ]);
        assert!(ok, "{out}");
        std::thread::sleep(Duration::from_secs(1));
    }
    block_on(async {
        let mut revs = Vec::new();
        for _ in 0..30 {
            revs = core
                .list_deployment_revisions(ns.name.clone(), "rb".into())
                .await
                .unwrap();
            if revs.len() == 3 {
                break;
            }
            sleep(&core, Duration::from_millis(500)).await;
        }
        assert_eq!(revs.len(), 3, "{revs:?}");
        assert_eq!(revs[0].image, "busybox:1.36.1");
        assert_eq!(revs[2].image, "busybox:1.36");

        // Roll back to revision 1
        core.rollback_deployment(ns.name.clone(), "rb".into(), revs[2].name.clone())
            .await
            .unwrap();
        let d = core
            .describe_resource(ResourceKind::Deployments, ns.name.clone(), "rb".into())
            .await
            .unwrap();
        assert_eq!(
            d["spec"]["template"]["spec"]["containers"][0]["image"],
            "busybox:1.36"
        );

        // Restore (roll forward to the newest template)
        core.rollback_deployment(ns.name.clone(), "rb".into(), revs[0].name.clone())
            .await
            .unwrap();
        let d = core
            .describe_resource(ResourceKind::Deployments, ns.name.clone(), "rb".into())
            .await
            .unwrap();
        assert_eq!(
            d["spec"]["template"]["spec"]["containers"][0]["image"],
            "busybox:1.36.1"
        );

        // scale + restart on the same private deployment
        core.scale_deployment(ns.name.clone(), "rb".into(), 0)
            .await
            .unwrap();
        assert!(core
            .scale_deployment(ns.name.clone(), "rb".into(), -1)
            .await
            .is_err());
        core.restart_deployment(ns.name.clone(), "rb".into())
            .await
            .unwrap();
        let d = core
            .describe_resource(ResourceKind::Deployments, ns.name.clone(), "rb".into())
            .await
            .unwrap();
        assert!(d["spec"]["template"]["metadata"]["annotations"]
            ["kubectl.kubernetes.io/restartedAt"]
            .is_string());
    });
}
