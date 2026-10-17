#!/usr/bin/env bash
# Kore harness cluster lifecycle. Never touches ~/.kube/config: everything goes
# through harness/.kube/config, which is what the app under test is launched with.
#
#   harness/cluster/cluster.sh up      create clusters + seed fixtures (idempotent)
#   harness/cluster/cluster.sh seed    re-apply fixtures only
#   harness/cluster/cluster.sh down    delete clusters
#   harness/cluster/cluster.sh status
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
HARNESS="$(cd "$HERE/.." && pwd)"
export KUBECONFIG="$HARNESS/.kube/config"
PRIMARY=kore-harness
SECONDARY=kore-harness-2
CTX="kind-$PRIMARY"
CTX2="kind-$SECONDARY"
K="kubectl --context $CTX"
METRICS_SERVER_VERSION=v0.7.2

log() { printf '\033[36m[cluster]\033[0m %s\n' "$*"; }

ensure_cluster() {
  local name=$1
  mkdir -p "$(dirname "$KUBECONFIG")"
  if ! kind get clusters 2>/dev/null | grep -qx "$name"; then
    log "creating kind cluster $name"
    kind create cluster --name "$name" --kubeconfig "$KUBECONFIG" --wait 180s
  fi
  kind export kubeconfig --name "$name" --kubeconfig "$KUBECONFIG" >/dev/null
}

install_metrics_server() {
  if ! $K -n kube-system get deploy metrics-server >/dev/null 2>&1; then
    log "installing metrics-server $METRICS_SERVER_VERSION"
    $K apply -f "https://github.com/kubernetes-sigs/metrics-server/releases/download/$METRICS_SERVER_VERSION/components.yaml"
    $K -n kube-system patch deploy metrics-server --type=json \
      -p '[{"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--kubelet-insecure-tls"}]'
  fi
}

seed() {
  log "applying fixtures"
  $K apply -f "$HERE/fixtures/00-namespaces.yaml"
  $K apply -f "$HERE/fixtures/20-crd.yaml"
  $K wait --for=condition=Established crd/widgets.kore.dev --timeout=60s
  for f in "$HERE"/fixtures/*.yaml; do
    case "$(basename "$f")" in 00-*|20-*) continue ;; esac
    $K apply -f "$f"
  done

  # Three revisions of deployment/web for rollback tests. Only bump when we're
  # not already at the last revision, so re-seeding stays idempotent.
  local img
  img=$($K -n kore-demo get deploy web -o jsonpath='{.spec.template.spec.containers[0].image}')
  if [[ "$img" == "nginx:1.25-alpine" ]]; then
    log "creating deployment revisions"
    $K -n kore-demo rollout status deploy/web --timeout=180s
    $K -n kore-demo set image deploy/web nginx=nginx:1.26-alpine
    $K -n kore-demo rollout status deploy/web --timeout=180s
    $K -n kore-demo set image deploy/web nginx=nginx:1.27-alpine
    $K -n kore-demo rollout status deploy/web --timeout=180s
  fi

  # Helm release with two revisions (history + rollback tests). Each rollback scenario adds
  # revisions and Helm keeps only the last 10, so once revision 1 ("Install complete") has been
  # pruned, reinstall to restore the expected history.
  if helm --kube-context "$CTX" -n kore-demo status kore-demo >/dev/null 2>&1 \
    && ! helm --kube-context "$CTX" -n kore-demo history kore-demo 2>/dev/null | grep -q 'Install complete'; then
    log "helm release kore-demo lost its install revision; reinstalling"
    helm --kube-context "$CTX" -n kore-demo uninstall kore-demo --wait
  fi
  if ! helm --kube-context "$CTX" -n kore-demo status kore-demo >/dev/null 2>&1; then
    log "installing helm release kore-demo (2 revisions)"
    helm --kube-context "$CTX" -n kore-demo install kore-demo "$HERE/charts/kore-demo" --wait
    helm --kube-context "$CTX" -n kore-demo upgrade kore-demo "$HERE/charts/kore-demo" \
      --set message=hello-from-helm-v2 --wait
  fi

  # Secondary cluster: a little distinct content for the multi-cluster view.
  kubectl --context "$CTX2" create namespace kore-remote --dry-run=client -o yaml | kubectl --context "$CTX2" apply -f -
  kubectl --context "$CTX2" -n kore-remote create deployment remote-web --image=nginx:1.27-alpine \
    --dry-run=client -o yaml | kubectl --context "$CTX2" apply -f -

  kubectl config use-context "$CTX" >/dev/null
  log "waiting for core workloads"
  $K -n kore-demo rollout status deploy/logger --timeout=180s
  $K -n kore-demo wait --for=condition=Ready pod/multi-container --timeout=180s
  $K -n kube-system rollout status deploy/metrics-server --timeout=180s || log "metrics-server not ready yet (metrics tests may be flaky)"
  log "ready. KUBECONFIG=$KUBECONFIG context=$CTX"
}

status() {
  kind get clusters
  $K get pods -A -o wide | grep -E 'kore-|metrics-server' || true
  helm --kube-context "$CTX" -n kore-demo history kore-demo 2>/dev/null || true
}

case "${1:-up}" in
  up) ensure_cluster "$PRIMARY"; ensure_cluster "$SECONDARY"; install_metrics_server; seed ;;
  seed) seed ;;
  down) kind delete cluster --name "$PRIMARY" --kubeconfig "$KUBECONFIG" || true
        kind delete cluster --name "$SECONDARY" --kubeconfig "$KUBECONFIG" || true ;;
  status) status ;;
  *) echo "usage: $0 up|seed|down|status" >&2; exit 2 ;;
esac
