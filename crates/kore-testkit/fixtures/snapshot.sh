#!/usr/bin/env bash
# Regenerate the fake-cluster fixtures from the seeded harness kind clusters
# (`kore-harness cluster up && kore-harness cluster seed`). Read-only against the clusters.
set -euo pipefail
cd "$(dirname "$0")"
export KUBECONFIG=../../../harness/.kube/config
NAMESPACED=pods,deployments,replicasets,statefulsets,daemonsets,jobs,cronjobs,services,configmaps,secrets,ingresses,networkpolicies,serviceaccounts,roles,rolebindings,events,controllerrevisions
CLUSTER=nodes,namespaces,clusterroles,clusterrolebindings,customresourcedefinitions

snap() { # context out-file namespaces...
  local ctx=$1 out=$2; shift 2
  {
    for ns in "$@"; do kubectl --context "$ctx" -n "$ns" get "$NAMESPACED" -o json; done
    kubectl --context "$ctx" get "$CLUSTER" -o json
    for ns in "$@"; do
      kubectl --context "$ctx" -n "$ns" get widgets.kore.dev -o json 2>/dev/null || true
      kubectl --context "$ctx" get --raw "/apis/metrics.k8s.io/v1beta1/namespaces/$ns/pods" 2>/dev/null || true
    done
    kubectl --context "$ctx" get --raw /apis/metrics.k8s.io/v1beta1/nodes 2>/dev/null || true
  } | python3 -c '
import json, sys, datetime
dec = json.JSONDecoder(); text = sys.stdin.read(); i = 0; items = []
while i < len(text):
    while i < len(text) and text[i].isspace(): i += 1
    if i >= len(text): break
    obj, i = dec.raw_decode(text, i)
    for it in obj.get("items", []):
        md = it.setdefault("metadata", {})
        md.pop("managedFields", None)
        ann = md.get("annotations") or {}
        ann.pop("kubectl.kubernetes.io/last-applied-configuration", None)
        if not ann: md.pop("annotations", None)
        if "apiVersion" not in it: it["apiVersion"] = obj["apiVersion"]
        if "kind" not in it: it["kind"] = obj["kind"][:-4]
        for k, v in list((it.get("data") or {}).items()):
            if it["kind"] == "ConfigMap" and "BEGIN CERTIFICATE" in v:
                it["data"][k] = "-----BEGIN CERTIFICATE-----\nFAKE\n-----END CERTIFICATE-----\n"
        items.append(it)
now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
json.dump({"apiVersion": "v1", "kind": "List", "snapshotTime": now, "items": items}, sys.stdout, indent=1, sort_keys=True)
' > "$out"
  echo "$out: $(python3 -c "import json;print(len(json.load(open(\"$out\"))[\"items\"]))") objects"
}

snap kind-kore-harness kore-harness.json kore-demo kore-rbac kube-system
snap kind-kore-harness-2 kore-harness-2.json kore-remote

# Pod logs (last 40 lines per container) for the log fixtures.
python3 - <<'PY'
import json, subprocess
logs = {}
for pod, containers in [("multi-container", ["main", "sidecar"]), ("crashloop", ["crash"])]:
    for c in containers:
        out = subprocess.run(["kubectl", "--context", "kind-kore-harness", "-n", "kore-demo", "logs", pod, "-c", c, "--tail=40"], capture_output=True, text=True).stdout
        logs[f"kore-demo/{pod}/{c}"] = out.splitlines()
pods = json.loads(subprocess.run(["kubectl", "--context", "kind-kore-harness", "-n", "kore-demo", "get", "pods", "-l", "app=logger", "-o", "json"], capture_output=True, text=True).stdout)["items"]
for p in pods:
    name = p["metadata"]["name"]; c = p["spec"]["containers"][0]["name"]
    out = subprocess.run(["kubectl", "--context", "kind-kore-harness", "-n", "kore-demo", "logs", name, "-c", c, "--tail=40"], capture_output=True, text=True).stdout
    logs[f"kore-demo/{name}/{c}"] = out.splitlines()
json.dump(logs, open("logs.json", "w"), indent=1, sort_keys=True)
print("logs.json:", len(logs), "containers")
PY

# Helm CLI output for the fake `helm` tool (see `kore_testkit::fixtures::helm_tool`).
mkdir -p helm
H="helm --kube-context kind-kore-harness"
$H version --short > helm/version.txt
$H list --output json --all-namespaces > helm/list.json
$H history kore-demo --namespace kore-demo --output json > helm/history.json
$H get values kore-demo --namespace kore-demo --all > helm/values.yaml
$H get manifest kore-demo --namespace kore-demo > helm/manifest.yaml
$H get notes kore-demo --namespace kore-demo > helm/notes.txt 2>&1 || true
echo "helm/: $(ls helm | wc -l | tr -d ' ') files"
