//! Backend fixes against kind: list pagination (A4), cluster-scoped delete
//! without a namespace (A6), watch payload context/namespace (A3).
mod common;

use common::*;
use kore_core::ResourceKind;
use std::time::Duration;

const MANY: usize = 600;

fn many_configmaps(ns: &str, n: usize) -> String {
    let mut y = String::with_capacity(n * 120);
    for i in 0..n {
        y.push_str(&format!(
            "---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: cm-{i:04}\n  namespace: {ns}\n  labels: {{kore-test: many}}\ndata: {{k: v}}\n"
        ));
    }
    y
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn list_paginates_past_500_and_watch_still_works() {
    require_kind!();
    let core = core();
    let ns = TempNamespace::new("kore-page");
    kubectl_apply(&many_configmaps(&ns.name, MANY));
    block_on(async {
        core.ready().await;
        // +1: every namespace has the kube-root-ca.crt ConfigMap.
        let all = core
            .list_resources_full(ResourceKind::Configmaps, Some(ns.name.clone()), None)
            .await
            .unwrap();
        assert!(!all.truncated);
        let mut got = names(&all.items);
        got.retain(|n| n.starts_with("cm-"));
        got.sort();
        got.dedup();
        assert_eq!(
            got.len(),
            MANY,
            "every ConfigMap listed, not just the first page"
        );

        // Label selectors are kept across pages.
        let sel = core
            .list_resources(
                ResourceKind::Configmaps,
                Some(ns.name.clone()),
                Some("kore-test=many".into()),
            )
            .await
            .unwrap();
        assert_eq!(sel.len(), MANY);

        // All-namespaces list includes them too.
        let every = core
            .list_resources(
                ResourceKind::Configmaps,
                None,
                Some("kore-test=many".into()),
            )
            .await
            .unwrap();
        assert_eq!(every.len(), MANY);

        // The watch's initial sync delivers every object, carries the
        // context/namespace (A3), and later changes still arrive.
        let mut w = core
            .start_watch(ResourceKind::Configmaps, Some(ns.name.clone()), None)
            .await
            .unwrap();
        let mut seen = std::collections::HashSet::new();
        let synced = recv_until(&core, &mut w, Duration::from_secs(30), |ev| {
            assert_eq!(ev.context, CTX);
            assert_eq!(ev.namespace.as_deref(), Some(ns.name.as_str()));
            if let Some(n) = ev.object.pointer("/metadata/name").and_then(|v| v.as_str()) {
                if n.starts_with("cm-") {
                    seen.insert(n.to_string());
                }
            }
            seen.len() == MANY
        })
        .await;
        assert!(synced.is_some(), "watch delivered {} of {MANY}", seen.len());
        let (ok, out) = kubectl(&["-n", &ns.name, "create", "configmap", "after-list"]);
        assert!(ok, "{out}");
        let ev = recv_until(&core, &mut w, Duration::from_secs(20), |ev| {
            ev.object.pointer("/metadata/name").and_then(|v| v.as_str()) == Some("after-list")
        })
        .await;
        assert!(
            ev.is_some(),
            "watch did not see a ConfigMap created after the list"
        );
    });
}

#[test]
#[ignore = "requires kind cluster (KORE_KIND=1)"]
fn delete_namespace_without_namespace_argument() {
    require_kind!();
    let core = core();
    let ns = TempNamespace::new("kore-del");
    block_on(async {
        core.ready().await;
        core.delete_resource(ResourceKind::Namespaces, String::new(), ns.name.clone())
            .await
            .expect("cluster-scoped delete must not require a namespace");
        // Gone or Terminating.
        for _ in 0..60 {
            let (ok, out) = kubectl(&[
                "get",
                "namespace",
                &ns.name,
                "-o",
                "jsonpath={.status.phase}",
            ]);
            if !ok || out.trim() == "Terminating" {
                return;
            }
            sleep(&core, Duration::from_millis(250)).await;
        }
        panic!("namespace {} was not deleted", ns.name);
    });
}
