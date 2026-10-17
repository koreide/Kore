# 04 — Backend (kore-core)

Status: reference for `crates/kore-core` as it is now (branch `kore-native-gpui`). kore-core was extracted from the legacy Tauri backend, which is still in git history at `3a66870`.
Paths without a prefix are relative to `crates/kore-core/src/`. App-side paths are given in full (`crates/kore-app/src/...`).

Crate layout:

| File | Role |
|---|---|
| `lib.rs` | `Core` facade: owns the tokio runtime and has one `pub async fn` per operation. Also defines `LogRequest`, `READ_ATTEMPT_TIMEOUT`, `READ_ATTEMPTS` and re-exports the public types |
| `config.rs` | `CoreConfig`: kubeconfig paths, initial context, data/favorites dirs, event retention, worker threads |
| `error.rs` | `K8sError` (thiserror), `ConnectionStatus`, `classify_connection_error` |
| `events.rs` | `Subscription<T>` / `Sink<T>` / `channel()`, plus the stream payload types (`PodLogPayload`, `MultiPodLogLine`, `PodColor`, `MultiPodLogStream`, `ExecExit`, `ExecSession`) |
| `json.rs` | `sort_keys` / `sorted`, for alphabetical key order in describe/YAML/diff output |
| `constants.rs` | Limits and timeouts (list page size 500 with a 50 000-object safety cap and 8 s × 3 per-page timeout, search 200, dashboard 1000, AI 2 MB, YAML 5 MB, multi-cluster 5 s, watch backoff 1 s → 30 s, log tail 200) |
| `tools.rs` | `enrich_path`, `which`, `resolve`, `command`: subprocess resolution for `kubectl`/`helm`/`claude`/`agent` |
| `secrets.rs` | API keys in the OS keychain (`keyring 2`, service `"kore"`) |
| `prefs.rs` | Favorites JSON (`favorites.json`, `{key: [values]}`) |
| `state/mod.rs` | `K8sState`, `ResourceKind`, `WatchEventPayload`, connection/context logic, stream cancellation scopes, event recording |
| `state/*.rs` | One module per feature: `resources`, `watcher`, `logs`, `multi_logs`, `exec`, `port_forward`, `metrics`, `yaml`, `rollback`, `dashboard`, `event_store`, `crd`, `graph`, `helm`, `multi_cluster`, `rbac`, `network_policy`, `debug`, `update`, `ai` |

Dependencies (crate `Cargo.toml`): `kube 0.91` / `kube-runtime 0.91` (rustls, `ws`), `k8s-openapi 0.22 (v1_29)`, `tokio` (multi-thread, process, net, io-util, time, sync, fs), `tokio-util` (codec, rt), `reqwest 0.12` (**rustls-tls**, no default features), `rusqlite 0.31 (bundled)`, `keyring 2`, `serde_yaml 0.9`, `similar 2`, `dirs 5`, `chrono`, `base64`, `rand 0.8`, `uuid`, `tempfile`, `futures`, `anyhow`, `thiserror 1`. kore-core has no UI dependency. The workspace members are `kore-core`, `kore-app`, `kore-automation` and `kore-harness`.

`K8sState` (`state/mod.rs`) is `Clone` (everything is behind an `Arc`) and holds:

- `inner: RwLock<StateInner>` with `client: Option<kube::Client>`, `kubeconfig: Option<Kubeconfig>`, `current_context: Option<String>` and `connection_error: Option<K8sError>`
- `config: Arc<CoreConfig>`
- `scopes: Mutex<StreamScopes>`: cancellation tokens `root`, `watches` (child of root) and `logs` (child of root). See §3.2
- `port_forwards: PortForwardManager`: `HashMap<forward_id, CancellationToken>`. These tokens are **not** children of `root`
- `exec_sessions: ExecManager`: `std::sync::Mutex<HashMap<session_id, {cancel_token, stdin_tx, resize_tx}>>`
- `event_store: OnceLock<Option<EventStore>>` and `event_retention_days: AtomicU32`
- `ready_tx: watch::Sender<bool>`, which is set when `init()` finishes

`ResourceKind` (serde `lowercase`) has 11 variants: pods, deployments, services, nodes, events, configmaps, secrets, ingresses, jobs, cronjobs, namespaces. It has `ALL`, `as_str()` (the lowercase plural) and `is_cluster_scoped()` (nodes and namespaces).

---

## 1. `Core` method table

Conventions:

- `Result<T>` is `Result<T, K8sError>` (`lib.rs`). Errors stay typed. The UI turns them into strings with `backend::format_error`, which is `Display`.
- **Wrapper** is how `Core` runs the call (§3.1):
  - **R** = `run`: wait for init, then spawn on the core runtime.
  - **RR** = `run_read`: like `run`, plus a per-attempt timeout and retries (§3.5).
  - **B** = `blocking`: `spawn_blocking` on the core runtime.
- **Validation** is checked in `Core` before any backend work. It fails with `K8sError::Validation(msg)`.
- "Client" means `K8sState::current_client()`. It fails with `ClientMissing` when there is no client.
- Effect abbreviations:
  - **K**: Kubernetes API call through kube-rs.
  - **SP**: subprocess via `tools::command` (resolved path, enriched PATH, `kill_on_drop(true)`, no console window on Windows).
  - **NET**: non-Kubernetes HTTP.
  - **FS**: filesystem.
  - **SUB**: returns a `Subscription` (§2).

### 1.0 Lifecycle and escape hatches

| Method | Returns | Notes |
|---|---|---|
| `Core::new()` / `Core::with_config(CoreConfig)` | `anyhow::Result<Core>` | Builds the runtime. Spawns `init()` and the hourly prune loop. Does not block |
| `ready()` | `()` | Resolves when `init()` has finished, whether it succeeded or failed |
| `is_ready()` | `bool` | Synchronous |
| `spawn(fut)` | `JoinHandle<T>` | Spawns arbitrary work on the core runtime. The handle can be awaited from any executor |
| `handle()` / `state()` | `&Handle` / `&K8sState` | `K8sState` methods must be polled on the core runtime (use `spawn`) |
| `shutdown()` | `()` | Cancels watches, logs, exec, AI streams and port-forwards (`K8sState::shutdown`) |

### 1.1 Connection / context

| Method | Args | Returns | W | Notes |
|---|---|---|---|---|
| `get_connection_status` | — | `ConnectionStatus {connected, error, error_kind, kubeconfig_path, contexts_available, current_context}` | R | `error_kind` is one of `no_kubeconfig \| auth_failed \| cluster_unreachable \| unknown`. Auth is detected by the substrings "unauthorized/forbidden/401/403" in a `Kube` error. `kubeconfig_path` is the configured paths joined, else `$KUBECONFIG`, else `~/.kube/config` (display only) |
| `retry_connection` | `context: Option<String>` | `ConnectionStatus` | R | Cancels every stream except port-forwards. Re-reads the kubeconfig and builds a client. K `apiserver_version()` probe with a 5 s timeout |
| `list_contexts` | — | `Vec<String>` | R | Uses the cached kubeconfig, otherwise re-reads it. Never errors (returns `[]`) |
| `list_namespaces` | — | `Vec<String>` | R | K list Namespace (no limit) |
| `switch_context` | `name` (non-empty) | `ConnectionStatus` | R | Cancels streams (not port-forwards), re-reads the kubeconfig, builds a client and runs the 5 s probe. Returns Ok even when the probe fails (the UI still follows the switch), but the returned `ConnectionStatus` is the post-switch status (`connected: false`, `error`, `error_kind`, same as a following `get_connection_status`) (Appendix A15). `Err` only if the kubeconfig is unreadable or no client can be built for the context; the previous client then stays |

### 1.2 Resources, watch, logs, exec, port-forward

| Method | Args | Returns | W | Notes |
|---|---|---|---|---|
| `list_resources` | `kind`, `namespace: Option`, `label_selector: Option` | `Vec<Value>` (key-sorted) | R | K list paged with `limit=500` + `continue` tokens until done (Appendix A4). Each page has its own 8 s timeout × 3 attempts; an expired (410) token restarts the list once. Stops at `MAX_LIST_TOTAL = 50 000` objects (warning logged). Listing `Events` also records them into the event store in the background, under the context captured before the list |
| `list_resources_full` | same | `ResourceList {items, truncated}` | R | Same as `list_resources`; `truncated` is true when the 50 000 cap cut the list |
| `start_watch` | `kind`, `namespace`, `label_selector` | `Subscription<WatchEventPayload>` | R | SUB. Independent per call (§2) |
| `fetch_logs` | `LogRequest {namespace, pod, container?, tail_lines?, previous?}` (ns and pod non-empty) | `String` | R | K `GET pods/log` (one shot). Also used for AI context |
| `delete_resource` | `kind`, `namespace`, `name` (non-empty; `namespace` non-empty for namespaced kinds only) | `()` | R | K delete with default propagation. Nodes and namespaces take no namespace (pass `""`; Appendix A6) |
| `describe_pod` | `namespace`, `pod_name` (non-empty) | `Value` (sorted) | RR | K get Pod |
| `describe_resource` | `kind`, `namespace`, `name` (non-empty) | `Value` (sorted) | RR | K get, 11-way match |
| `list_events_for_resource` | `kind: String` (ignored), `namespace`, `name` (both non-empty) | `Vec<Value>` (sorted) | RR | K list Events with fieldSelector `involvedObject.name=…,involvedObject.namespace=…`. The kind is ignored. Also records the events into the store |
| `start_pod_logs_stream` | `namespace`, `pod_name` (non-empty), `container: Option`, `previous: Option<bool>` | `Subscription<PodLogPayload>` | R | SUB. With no container, K gets the pod and streams every container, prefixing lines with `[name] ` |
| `stop_pod_logs_stream` | — | `()` | R | Cancels **every** single-pod and multi-pod log stream (the `logs` scope). Dropping the individual subscription is preferred |
| `get_pod_metrics` | `namespace`, `pod_name` (non-empty) | `Value` (PodMetrics) | RR | K `metrics.k8s.io/v1beta1 pods`. A 404 is rewritten to "Pod '…' metrics not found. Metrics Server may not be available." |
| `start_port_forward` | `namespace`, `pod_name` (non-empty), `local_port: u16`, `pod_port: u16` | `Value {localPort, podPort, status:"active"}` | R | Binds `127.0.0.1:local_port` immediately ("Port N is already in use" on `AddrInUse`). The id is `ns/pod/local/remote`, and a duplicate is an error. For **each accepted connection**: SP `kubectl port-forward pod/P TMP:PORT -n NS --context <Kore's context>` (with `KUBECONFIG` set when paths are configured), sleep 200 ms, connect with ≤30×100 ms retries, two-way copy, then kill kubectl |
| `stop_port_forward` | same four args | `()` | R | Cancels the token. Unknown id → `PortForward("… not found")` |
| `scale_deployment` | `namespace`, `name` (non-empty), `replicas ≥ 0` | `()` | R | K merge-patch `spec.replicas`, field manager `kore` |
| `restart_deployment` | `namespace`, `name` (non-empty) | `()` | R | K merge-patch annotation `kubectl.kubernetes.io/restartedAt`, field manager `kore`, no force |
| `search_resources` | `query` (non-empty), `namespace: Option` | `Vec<Value>` with an added `_kind` | R | 9 parallel `list_resources` calls (pods, deployments, services, configmaps, secrets, jobs, cronjobs, ingresses, namespaces). Case-insensitive substring match on the name, truncated to 200. Per-kind errors are swallowed |
| `exec_into_pod` | `namespace`, `pod_name` (non-empty), `container: Option`, `shell: Option` (default `/bin/sh`) | `ExecSession` | R | SUB. K websocket exec (`AttachParams::interactive_tty`). The session id is `ns/pod/<uuid-v4-shaped>` |
| `send_exec_input` | `session_id`, `data: String` | `()` | R | UTF-8 convenience over `send_exec_bytes` |
| `send_exec_bytes` | `session_id`, `data: Vec<u8>` | `()` | R | Bounded `mpsc` (cap 256) to stdin. Unknown session → `Validation("Exec session … not found")` |
| `resize_exec` | `session_id`, `cols: u16`, `rows: u16` | `()` | R | Sends `TerminalSize` over kube's terminal-size channel |
| `stop_exec` | `session_id` | `()` | R | Cancels the session and removes it. Unknown session → `Validation` |

### 1.3 YAML / rollback

| Method | Args | Returns | W | Notes |
|---|---|---|---|---|
| `get_resource_yaml` | `kind`, `namespace`, `name` | `String` | RR | `describe_resource`, strip `metadata.managedFields`, re-sort keys, then `serde_yaml` |
| `apply_resource_yaml` | `kind`, `namespace`, `name`, `yaml` (≤5 MB) | `"Applied successfully"` | R | K **server-side apply**, `PatchParams::apply("kore").force()`. Nodes and Events → `Validation("YAML apply not supported for …")` |
| `diff_resource_yaml` | `kind`, `namespace`, `name`, `new_yaml` | `Vec<DiffLine {tag: equal\|insert\|delete, value}>` | R | `similar` line diff of the live YAML (as returned by `get_resource_yaml`) against the input |
| `list_deployment_revisions` | `namespace`, `name` | `Vec<DeploymentRevision {revision, name, created, image, change_cause, replicas}>` | RR | K lists **all** ReplicaSets in the namespace, then filters by ownerRef kind+name. The image comes from the first container. Sorted by revision, descending |
| `get_revision_yaml` | `namespace`, `rs_name` | `String` | RR | K get RS, sort keys, YAML |
| `rollback_deployment` | `namespace`, `deployment_name`, `rs_name` | `()` | R | K merge-patch `spec.template` with the RS template **minus the `pod-template-hash` label** (as `kubectl rollout undo` does). Field manager `kore`, no force |

### 1.4 Dashboard / event history / multi-pod logs

| Method | Args | Returns | W | Notes |
|---|---|---|---|---|
| `get_cluster_health` | — | `ClusterHealth {score, pods, nodes, restart_hotlist, pending_pods, recent_warnings}` | R | 3 parallel `list_resources` calls across all namespaces (pods, nodes, events; paginated, so no longer capped at 500), then pure `compute_health`. List errors are logged and become empty lists. `NodeHealth.cpu_usage/memory_usage` are **allocatable** values; no node metrics are fetched |
| `get_cluster_health_multi_cluster` | — | `MultiClusterHealth {clusters: [{context, health}]}` | R | One task per kubeconfig context with a temporary client (limit 1000). Each context has a **5 s timeout**. Failed or timed-out contexts are dropped from the result, with only a log line |
| `query_stored_events` | `since`, `until` (RFC 3339), `namespace: Option` | `Vec<StoredEvent>` (≤500, `last_seen` desc) | R | SQLite query filtered by the **current context**. Errors with "Event store not available" if the DB failed to open |
| `set_event_retention` | `days: u32` | `u64` (pruned rows) | R | Stores the retention and prunes now. The hourly loop uses the stored value |
| `stream_multi_pod_logs` | `namespace`, `label_selector`, `tail_lines: Option` (default 50) | `MultiPodLogStream {pods: Vec<PodColor>, lines: Subscription<MultiPodLogLine>}` | R | SUB. K lists pods by selector; no pods → `Validation`. One follow stream per pod (default container only). Colors cycle through 8 hex values. The pod set is fixed at start |

### 1.5 CRDs / graph

| Method | Args | Returns | W | Notes |
|---|---|---|---|---|
| `list_crds` | — | `Vec<CrdInfo {name, group, version, kind, scope, plural}>` | R | K list CRDs. The version is the **first served** version, not the storage version |
| `list_crd_instances` | `group`, `version`, `plural`, `namespace: Option` | `Vec<Value>` (sorted) | R | K `DynamicObject` list, no limit |
| `get_crd_instance` | `group`, `version`, `plural`, `namespace` (`""` = cluster), `name` | `Value` (sorted) | R | K get |
| `delete_crd_instance` | same | `()` | R | K delete |
| `build_resource_graph` | `namespace: Option` | `ResourceGraph {nodes: [GraphNode {id, name, namespace, kind, status}], edges: [{source, target, relation}]}` | R | 7 parallel K lists (pods, deployments, services, ingresses, jobs, cronjobs, RS; inactive RS are skipped). Edges come from ownerRefs, service selectors and ingress backends. Node ids are `kind/ns/name` |

### 1.6 Helm and multi-cluster

Helm commands are SP `helm`. Every invocation except `helm_available` gets `--kube-context <current>`, plus `KUBECONFIG=<configured paths>` when `CoreConfig.kubeconfig_paths` is set (`apply_kubeconfig_env`). Revisions are deserialized as string-or-int, because `helm history -o json` prints integers.

| Method | Args | Returns | W | Subprocess / notes |
|---|---|---|---|---|
| `helm_available` | — | `bool` | R | `helm version --short` |
| `list_helm_releases` | `namespace: Option` | `Vec<HelmRelease {name, namespace, revision, updated, status, chart, app_version}>` | R | `helm list --output json (--namespace X \| --all-namespaces)` |
| `get_helm_values` | `release`, `namespace` | `String` | R | `helm get values R --namespace N --all` |
| `get_helm_manifest` | `release`, `namespace` | `String` | R | `helm get manifest R --namespace N` |
| `get_helm_history` | `release`, `namespace` | `Vec<HelmRevision>` | R | `helm history R --namespace N --output json` |
| `rollback_helm_release` | `release`, `namespace`, `revision: String` | `String` (stdout) | R | `helm rollback R REV --namespace N` |
| `list_resources_multi_cluster` | `kind`, `namespace`, `label_selector` | `Vec<Value>` with an added `_context` | R | One task per context with a temporary client, no list limit, **5 s per-context timeout**. Failures and timeouts are logged and dropped |

### 1.7 AI (`state/ai.rs`)

`AIConfig {provider: AIProvider, api_key?, model, base_url?}`. `AIProvider` serializes as `openai | anthropic | ollama | claude_cli | cursor_agent`.

| Method | Args | Returns | W | Notes |
|---|---|---|---|---|
| `ai_diagnose` | `AIConfig`, `DiagnoseRequest {kind?, namespace?, name?, prompt?, session_id?}` | `Subscription<AIStreamEvent>` | R | Context is gathered **before** the subscription is returned: describe, events, logs tail 200 and metrics (pods only). The spawned task then streams from the provider. OpenAI and Anthropic use SSE. Ollama uses NDJSON on `{base_url \| http://localhost:11434}/api/chat`. `claude` CLI runs `-p <prompt> --model M --output-format stream-json --include-partial-messages --verbose --no-session-persistence` with `CLAUDE_CODE_ENTRYPOINT`/`CLAUDECODE` removed. `agent` CLI runs `-p --output-format stream-json --stream-partial-output --model M --mode ask --trust --workspace <temp_dir> <prompt>`. The response is capped at 2 MB. **Dropping the subscription cancels the request and kills the CLI child** |
| `ai_chat` | `AIConfig`, `AIChatRequest {messages: [{role, content}], session_id, namespace?}` | `Subscription<AIStreamEvent>` | R | API providers run a tool-calling loop of ≤10 rounds. The tools are `list_resources, get_cluster_health, describe_resource, get_resource_events, get_pod_logs, get_pod_metrics, search_resources, list_namespaces`, executed against `K8sState`. Each tool call sends `Status("Querying …")`. CLI providers make one SP call with pre-gathered cluster context. Ollama's text-tool fallback replays the answer in 80-byte chunks. Cancellable by drop |
| `ai_test_connection_checked` | `AIConfig` | `()` | R | NET. A non-2xx response becomes `Err("HTTP <code> <reason>: …")`. This is what the Settings "Test Connection" uses |
| `ai_test_connection` | `AIConfig` | `bool` | R | NET (OpenAI `GET /v1/models`, Anthropic `POST /v1/messages`, Ollama tags) |
| `list_ollama_models` | `base_url: Option` | `Vec<String>` | R | NET `GET {base}/api/tags` |
| `claude_cli_available` / `list_claude_models` | — | `bool` / `Vec<String>` (hard-coded `opus, sonnet, haiku`) | R | SP `claude --version` |
| `cursor_agent_available` / `list_cursor_agent_models` | — | `bool` / `Vec<String>` | R | SP `agent --version` / `agent models` (ANSI-stripped) |
| `check_providers_availability` | `ollama_base_url: Option` | `HashMap<String,bool>` with keys `claude_cli, cursor_agent, ollama` | R | `tokio::join!` of the three probes. The Ollama probe uses the configured base URL |

Base URLs: OpenAI defaults to `https://api.openai.com` and can be overridden with env `KORE_OPENAI_BASE_URL`. Anthropic defaults to `https://api.anthropic.com` and can be overridden with `KORE_ANTHROPIC_BASE_URL`. Both are normalized: a trailing `/` or `/v1` is stripped. Ollama uses `AIConfig.base_url`.

### 1.8 Keys, debug containers, network policy, RBAC, update, favorites

| Method | Args | Returns | W | Notes |
|---|---|---|---|---|
| `store_api_key` / `get_api_key` / `delete_api_key` | `provider` (+ `key`) | `()` / `Option<String>` / `()` | B | OS keychain: macOS Keychain, Windows Credential Manager, or Linux Secret Service (needs a running secret-service daemon). `NoEntry` maps to `None`. Delete is idempotent |
| `add_debug_container` | `namespace`, `pod_name`, `image`, `target_container: Option`, `command: Option<Vec<String>>` | `String` (`debugger-<8hex>`) | R | K patch of the `ephemeralcontainers` subresource, then polls `get` every 1 s, 30 times, until Running. Errors are mapped to friendly text (e.g. "requires K8s 1.25+") |
| `list_debug_containers` | `namespace`, `pod_name` | `Vec<Value {name, image, targetContainer, running}>` | RR | K get pod |
| `stop_debug_container` | `namespace`, `pod_name`, `container_name` | `()` | R | K **attach** to PID 1, write `\nexit\n`, then poll |
| `build_network_policy_graph` | `namespace: Option` | `NetworkPolicyGraph` | R | 4 parallel K lists (NetworkPolicy, Pod, ReplicaSet, Namespace). Pure static evaluation; nothing is enforced or probed |
| `simulate_network_traffic` | `source_namespace`, `source_pod`, `dest_namespace`, `dest_pod`, `port: Option<i32>`, `protocol: Option` (default TCP) | `TrafficSimulationResult` | R | K gets both pods and the policies, then evaluates statically |
| `rbac_check_permission` | `identity: RbacIdentity` (tagged `kind`: `User{name}` / `Group{name}` / `ServiceAccount{name, namespace}`), `verb`, `resource`, `namespace: Option` | `PermissionCheckResult` | R | The API group is derived via `api_group_for_resource_cmd`. The K snapshot lists Roles, RoleBindings, ClusterRoles, ClusterRoleBindings and ServiceAccounts in parallel; any error fails the call. Evaluated locally, **not** a SubjectAccessReview (aggregation, `resourceNames` and non-resource URLs are approximated) |
| `rbac_build_matrix` | `identity`, `namespace` | `PermissionMatrix` | R | Snapshot |
| `rbac_reverse_lookup` | `role_kind`, `role_name`, `role_namespace: Option` | `ReverseLookupResult` | R | Snapshot |
| `rbac_analyze_forbidden` | `error_message` | `ForbiddenAnalysis` | R | Parses Kubernetes 403 text, then uses the snapshot |
| `rbac_list_identities` | — | `RbacIdentityList` | R | Snapshot |
| `rbac_list_roles` | `namespace: Option` | `Vec<RoleSummary>` | R | Snapshot |
| `rbac_natural_language_query` | `query`, `namespace` | `NaturalLanguageRbacResult` | R | Rule-based keyword parser (**no LLM**), then the snapshot |
| `rbac_who_can` | `verb`, `resource`, `namespace` | `Vec<PermissionCheckResult>` | R | Snapshot, evaluated for every SA and every bound subject |
| `check_for_updates` | — | `UpdateInfo {has_update, current_version, latest_version, release_url, release_notes}` | R | NET `GET https://api.github.com/repos/koreide/Kore/releases/latest` (UA `Kore-Update-Checker`). `has_update` is true only when the tag is a strictly newer `major.minor.patch` (leading `v` and pre-release/build suffixes ignored; unparsable tags fall back to inequality) |
| `perform_update` | — | `String` (tag) | R | **macOS only**: returns an error elsewhere; `kore_core::SELF_UPDATE_SUPPORTED` tells the UI (§4.2) |
| `load_favorites` / `save_favorites` | `key` (+ `values`) | `Vec<String>` / `()` | B | FS `<favorites_dir>/favorites.json` (default `~/.kore`). Save is read-modify-write, written atomically via `favorites.json.tmp` and a rename. No lock |

Public on `K8sState` but not on `Core`: `new()` (default config, init inline), `new_uninit`, `init`, `wait_ready`, `reload_kubeconfig`, `current_client`, `current_context_name`, `event_retention_days`, `ollama_available`, `shutdown`. Also public: `EventStore::{upsert_event, upsert_events, store_events, store_watch_event, query_events, prune, to_stored}`, `tools::{which, resolve, command, enriched_path}`, and `state::ai::{openai_base_url, anthropic_base_url, ollama_base_url, http_error_reason}`.

---

## 2. Streams (`Subscription<T>`)

Every stream is a `Subscription<T>` created by `events::channel(parent_token)`. Its token is a child of one of the `StreamScopes` tokens (§3.2), and every `Subscription` has a process-unique `id: u64`.

| Producer (`Core` method) | Item type | Parent scope | Producer task(s) | Ends when |
|---|---|---|---|---|
| `start_watch` | `WatchEventPayload {action: "applied"\|"deleted"\|"error", kind: "<plural>", object: Value, context: String, namespace: Option<String>}`. `context` is the kube context the watch started in; `namespace` its namespace scope (`None` for all namespaces and cluster-scoped kinds). Both are `#[serde(default)]` (Appendix A3). The watcher's initial list is paginated by kube-runtime (page size 500). For `error`, `object = {message}`. A watcher `Restarted(objs)` becomes N `applied` items. Objects are **not** key-sorted | `watches` | One task running `kube::runtime::watcher`. A stream end or error sends an `error` item (errors only), then reconnects with exponential backoff from 1 s to 30 s plus 0–25 % jitter. An `Events` watch also records events into the store | Drop / `cancel()`, context switch, retry, shutdown. It never stops on its own |
| `start_pod_logs_stream` | `PodLogPayload`, one of: `{logs: "<line>\n", append: true}` (prefixed `[container] ` when streaming several containers), `{logs: "No previous logs — this container has not been restarted.\n"}` (info, single-container `previous` only), or `{error}` | `logs` | One task per container. `follow = !previous`, tail 200 | Drop, `stop_pod_logs_stream`, context switch, shutdown. Each task also ends at EOF or on error. The channel closes when all tasks have ended |
| `stream_multi_pod_logs` → `.lines` | `MultiPodLogLine {pod, color: "#rrggbb", line}` (no trailing newline). Pod-level errors are only logged | `logs` | One task per pod, follow, default container only | Same as pod logs |
| `exec_into_pod` → `ExecSession {session_id, stdout: Subscription<Vec<u8>>, exit: oneshot::Receiver<ExecExit>}` | `stdout`: raw terminal bytes (4 KiB reads). `exit`: `ExecExit {reason: "completed"}` or `{error}`, sent once | `root` | A session task plus stdout and stdin/resize tasks | Drop `stdout`, `stop_exec`, context switch / retry (via `ExecManager::cancel_all` and `root`), shutdown, remote EOF. The `SessionGuard` removes the map entry synchronously when the task ends |
| `ai_diagnose`, `ai_chat` | `AIStreamEvent` (serde tag `type`): `Chunk {content}`, `Done`, `Error {message}`, `Status {message}` | `root` | One task wrapping the provider stream in `select!` on the token | Drop / cancel (CLI children are killed through `kill_on_drop`), context switch, shutdown, provider done/error, or the 2 MB cap |

`Subscription<T>` API (`events.rs`):

- `recv().await -> Option<T>`. It returns `None` once all producers are gone.
- `try_recv()`.
- `try_recv_batch(max) -> Vec<T>`, for per-frame coalescing.
- `cancel()`: cancels while keeping buffered items readable.
- `is_cancelled()`.
- `impl futures::Stream`.
- The channel is a **tokio unbounded mpsc**, so there is no backpressure or overflow policy. Its `recv` only needs a waker, so it works from any executor.

`Sink<T>` (producer side):

- `send(v) -> Result<(), SinkClosed>`. It fails once the token is cancelled or the receiver is dropped.
- `token()`, `is_closed()`, `closed().await`.
- A legacy alias takes an ignored event-name string. New code should call `send`.

Port-forwards are **not** a stream. Their status is the `start_port_forward` result plus log lines. Per-connection kubectl failures are only logged.

The payload types keep the JSON shapes the UI was written against (checked by a unit test in `events.rs`).

---

## 3. Architecture as implemented

### 3.1 `Core` and its runtime

- `Core` is `Clone` (an `Arc<CoreInner>`). `CoreInner` holds `runtime: Mutex<Option<Runtime>>`, `handle: Handle` and `state: K8sState`.
- `with_config` builds a multi-thread tokio runtime with `worker_threads = config.worker_threads.max(1)` (default 4), `enable_all()` and thread name `kore-rt`. It then:
  1. spawns `state.init()`;
  2. spawns an hourly `set_event_retention(event_retention_days())` prune loop (the first tick is skipped; init already pruned once).
- **One async method per operation.** Each `Core` method validates its arguments synchronously. It then calls one of these private helpers:
  - `run(f)`: spawns `async { state.wait_ready().await; f(state).await }` on the core runtime and awaits the `JoinHandle`.
  - `run_read(args, f)`: the same, with a per-attempt timeout and retries (§3.5).
  - `blocking(f)`: `spawn_blocking` on the core runtime.

  A `JoinError` maps to `K8sError::Cancelled` (task cancelled) or to `Other("backend task panicked: …")`.
- Because the work runs on the owned runtime and only a `JoinHandle` is awaited, every `Core` future can be polled from any executor: GPUI, `futures::executor::block_on`, or another tokio runtime. The unit test `core_works_without_tokio_context` and all kind tests rely on this.
- `Drop for CoreInner` calls `Runtime::shutdown_background()`, which never blocks. This makes dropping the last `Core` safe inside an async context (test `dropping_core_inside_async_context_does_not_panic`).
- Blocking work goes to the blocking pool:
  - SQLite (`EventStore::with_db` holds an `Arc<std::sync::Mutex<Connection>>` and uses `spawn_blocking`);
  - keychain;
  - favorites;
  - opening the event store at init.

  - the DMG install step of `perform_update` (`update::install_dmg`: writing the DMG, `hdiutil`, `rm`/`cp`, `xattr`).

  The remaining blocking call inside async code is `Kubeconfig::read*` (small sync file IO).

### 3.2 Stream lifetime and cancellation

- `StreamScopes {root, watches, logs}` lives behind a `std::sync::Mutex`. `watches` and `logs` are children of `root`.
- `cancel_streams()` is called by `switch_context`, `retry_connection` and `shutdown`. It cancels `root`, replaces all three scopes with fresh tokens and calls `ExecManager::cancel_all()`. This ends every watch, log, exec and AI stream.
- `cancel_logs()` (`stop_pod_logs_stream`) cancels only the `logs` scope and replaces it.
- Port-forward tokens are independent. Only `stop_port_forward` and `shutdown` (`PortForwardManager::cancel_all`) stop them, so a forward survives a context switch.
- There are no global single slots. Any number of watches and log streams can run at once; the table, pod details and network-policy views each own their own.
- Producers `select!` on their token and also stop on `SinkClosed` (receiver dropped).

### 3.3 Bridge into GPUI (`crates/kore-app/src/backend.rs`)

- `kore_core::enrich_path()` is the very first statement of `main()`, before the tracing subscriber, GPUI or the Core runtime start any thread (it mutates the process env; Appendix A21).
- `backend::init(cx)` runs inside the GPUI `run` callback in `main.rs`. It:
  1. calls `Core::new()` (default `CoreConfig`, so `KUBECONFIG` / `~/.kube/config` are read by Core) and panics if the runtime cannot start;
  3. stores the result as the GPUI global `Backend(Core)`;
  4. registers `cx.on_app_quit`, which races `core.shutdown()` against a 2 s GPUI timer.
- `backend::core(cx) -> Core` returns a cheap clone.
- `backend::run(cx, |core| async move { core.x().await }, |view, result, cx| …) -> Task<()>`:
  - spawns on the view's GPUI context, awaits the Core future and applies the result through `this.update`;
  - skips the callback if the view was dropped;
  - must be stored or detached, because dropping the task cancels the call and drops any `Subscription` it produced.
- `backend::sleep(cx, d)` is a GPUI timer. Use it instead of `tokio::time` on the UI side.
- `backend::format_error(&K8sError)` is `to_string()`.
- Stream consumption pattern, used by `state/resources.rs`, `views/pod_logs.rs`, `views/multi_pod_logs.rs`, `views/exec_terminal.rs` and `state/ai_stream.rs`:
  1. The view keeps the `Subscription` inside a stored `Task`.
  2. The task loops `while let Some(first) = sub.recv().await`, then calls `try_recv_batch(N)` (N is 4096 for watches, 2048 for logs, 256–4096 for exec and AI).
  3. It makes one `this.update(..)` and one notify per batch.
  4. Replacing or dropping the task cancels the backend stream.

### 3.4 Errors (`error.rs`)

`K8sError` variants and their `Display`:

| Variant | `Display` |
|---|---|
| `Kubeconfig(KubeconfigError)` | "Kubeconfig not found or unreadable" |
| `ClientMissing` | "Client not initialized" |
| `Kube(kube::Error)` | "Kubernetes error: …" |
| `Io(std::io::Error)` | "IO error: …" |
| `Serde(serde_json::Error)` | "Serialization error: …" |
| `Validation(String)` | "Validation error: …" |
| `PortForward(String)` | "Port forward error: …" |
| `Cancelled` | "Operation cancelled" |
| `Other(String)` | Verbatim. Used by the update checker, keychain, favorites, AI and read timeouts. Built with `K8sError::other(..)` |

`K8sError` is not `Clone`, because `kube::Error` is not. `connection_error` is therefore stored by value and surfaced only through `ConnectionStatus` strings.

`classify_connection_error` maps:
- `Kubeconfig` → `no_kubeconfig`;
- a `Kube` error with auth substrings → `auth_failed`;
- any other `Kube` error → `cluster_unreachable`;
- everything else → `unknown`.

### 3.5 Read retries (`Core::run_read`)

- Used for idempotent reads: `describe_pod`, `describe_resource`, `list_events_for_resource`, `get_pod_metrics`, `get_resource_yaml`, `list_deployment_revisions`, `get_revision_yaml`, `list_debug_containers`.
- Each attempt is wrapped in `tokio::time::timeout(READ_ATTEMPT_TIMEOUT = 8 s)`. A timed-out attempt is retried, up to `READ_ATTEMPTS = 3`. After that the call returns `Other("Request timed out after 3 attempts")`.
- Only timeouts are retried. An attempt that returns an error is returned immediately.
- The purpose is to keep a request sent on a stale pooled connection from hanging until TCP gives up and leaving the UI on a skeleton.
- `list_resources` is not wrapped: a paginated list can legitimately take longer than 8 s, so the same timeout/retry (`LIST_PAGE_TIMEOUT`, `LIST_PAGE_ATTEMPTS`) applies to each page inside `state::resources::paginate`.
- Everything else (writes, streams, RBAC, dashboard, CRDs, Helm, AI) uses plain `run`, with no Core-level timeout.

### 3.6 JSON key order (`json.rs`)

- GPUI enables serde_json's `preserve_order` feature, and Cargo unifies features across the workspace, so `Value` objects keep the API server's key order.
- `json::sort_keys(&mut Value)` and `json::sorted(Value)` recursively sort keys, which restores the alphabetical output of a BTreeMap build. They are applied to:
  - list items (`list_resources`, `list_events_for_resource`, CRD instances);
  - describe/get results;
  - `get_resource_yaml` (re-sorted after `managedFields` removal, because `remove` swap-removes under `preserve_order`);
  - `get_revision_yaml`.
- Watch payload objects and `get_pod_metrics` are **not** sorted.

### 3.7 Configuration, paths and subprocesses

- `CoreConfig` (`config.rs`):

  | Field | Default |
  |---|---|
  | `kubeconfig_paths: Vec<PathBuf>` | Empty, meaning `Kubeconfig::read()` from `KUBECONFIG` / `~/.kube/config`. When set, the files are merged in order and also passed to kubectl and Helm as `KUBECONFIG` |
  | `initial_context` | Kubeconfig `current-context`, else the first context |
  | `data_dir` | `dirs::data_dir()/kore`, holding `events.db` |
  | `favorites_dir` | `~/.kore` |
  | `event_retention_days` | 7 |
  | `worker_threads` | 4 |

  kore-app uses `CoreConfig::default()`. Tests inject temp dirs and the harness kubeconfig.
- `tools.rs`:
  - `enriched_path()` appends well-known tool dirs that exist and are not already listed. It uses `std::env::split_paths`/`join_paths`, so the separator is platform-correct.
    - Unix: `/usr/local/bin`, `/opt/homebrew/{bin,sbin}`, `/usr/local/sbin`, `/usr/local/aws-cli`, the Python.framework bin, `/snap/bin`, `/nix/var/nix/profiles/default/bin`, and under `~`: `.local/bin`, `bin`, `.krew/bin`, `google-cloud-sdk/bin`, `.nix-profile/bin`, the global Node-package bin dir, `.rd/bin`, `.docker/bin`.
    - Windows: the `%APPDATA%` Node-package dir, WinGet Links, scoop shims, `.krew\bin`, chocolatey bin.
  - `resolve(name)` checks the override env `KORE_<NAME>` (`KORE_KUBECTL`, `KORE_HELM`, `KORE_CLAUDE`, `KORE_AGENT`) first. It then searches PATH, trying `PATHEXT` extensions plus `.exe/.cmd/.bat` on Windows, and finally falls back to the bare name.
  - `command(name)` sets the child's PATH, `kill_on_drop(true)` and, on Windows, `CREATE_NO_WINDOW`.
- Event store (`state/event_store.rs`): SQLite `events` table keyed by uid, with indexes on `last_seen`, `namespace` and `context`.
  - Written by: listing `Events`, an `Events` watch (applied and restarted items), and `list_events_for_resource`. All three go through `K8sState::record_events`, which runs in the background and never fails the caller.
  - Pruned at init and hourly (`DELETE … WHERE last_seen < now - days`).

---

## 4. App lifecycle and platform specifics

### 4.1 Startup sequence (`crates/kore-app/src/main.rs`)

1. `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` gives Windows release builds no console.
2. `tracing_subscriber::fmt()` logs to stderr with `EnvFilter` from `RUST_LOG`, default `info`. There is no file logging.
3. `gpui_kit::application().with_assets(..).run(|cx| …)`:
   1. `gpui_kit::init`.
   2. `platform::set_app_icon()`, which sets the Dock icon from the embedded `icon.icns` on macOS and is a no-op elsewhere.
   3. `backend::init(cx)` (§3.3: `Core::new`, quit hook; `enrich_path` already ran as the first statement of `main`). `Core::new` does not block. The first frame renders while init runs, and every call waits for `ready`.
   4. `Settings::load()`, `Theme::init(accent)`, `root::init_keymap`.
4. One window:
   - title "Kore", 1400×900 centered, resizable, native decorations, `app_id = "dev.kore.desktop"`;
   - on macOS, `platform::style_window` makes the titlebar transparent over `#0b1221`;
   - no min size and no remembered geometry.
5. In `--features test-harness` builds only, `KORE_CONTROL_SOCKET=<path>` starts the Layer 2 control server (Unix socket) and `KORE_BACKGROUND` skips focusing or activating the window.
6. Closing the last window quits the app. Quit triggers `Core::shutdown()` with a 2 s bound. Port-forward kubectl children and AI CLI children are also covered by `kill_on_drop`.
7. `KORE_FONT_PROBE=1` prints mono-font advances and exits (dev aid).

`platform::open_url(url)` opens links with `open` (macOS), `cmd /C start` (Windows) or `xdg-open` (Linux). It is used for the release, sponsor and `http://localhost:<port>` links.

### 4.2 Update check (`state/update.rs`)

- The app checks on startup (`AppState::check_updates`, `crates/kore-app/src/state/app_state.rs`). The result is cached for 24 h in the app's local storage under key `kore-update-check`. Settings "Check for Updates" forces a check (`check_updates_now`). A banner shows when `has_update` is true.
- The GitHub endpoint is a hard-coded constant and cannot be injected. `is_newer` compares numeric `major.minor.patch`, so a build newer than the latest release is not offered a downgrade.
- `perform_update` is user-triggered from the banner. It:
  1. finds the `.dmg` asset for this architecture only (`_aarch64`, or `_x86_64`/legacy `_x64`); there is no fallback to the other architecture;
  2. downloads it into memory and writes it to a tempdir;
  3. runs `hdiutil attach -nobrowse -quiet -mountpoint`;
  4. deletes `/Applications/<App>.app`, then `cp -R` the new bundle;
  5. runs `hdiutil detach` and `xattr -dr com.apple.quarantine`.

  There is no signature or checksum check and no relaunch. `perform_update` is macOS-only (`SELF_UPDATE_SUPPORTED`); on Linux/Windows the banner button reads "Download" and opens the release page, where the release workflow publishes the tarball and the installer.

### 4.3 Window and bundle identity

- **macOS** (`crates/kore-app/packaging/macos/bundle.sh`):
  - builds `target/release/bundle/macos/kore.app`;
  - product name `kore`, bundle id **`dev.kore.desktop`**, version from `crates/kore-app/Cargo.toml` (0.2.0);
  - `LSMinimumSystemVersion` **14.0**, `NSHighResolutionCapable`, icon `assets/icons/icon.icns`;
  - ad-hoc signed unless `SIGN_IDENTITY` is set.

  `dmg.sh` packages the DMG. The release workflow builds `aarch64` and `x86_64` DMGs.
- **Linux** (`crates/kore-app/packaging/linux/`):
  - `dev.kore.desktop.desktop` has `Exec=kore`, `Icon=dev.kore.desktop` and `StartupWMClass=dev.kore.desktop`, matching the window `app_id`;
  - `install.sh` installs the binary, desktop file and hicolor icons into `PREFIX` (default `~/.local`);
  - `package.sh` writes `kore_<ver>_<arch>.tar.gz`;
  - `deps.sh` and the `Dockerfile` cover build deps plus a headless Xvfb smoke stack (`smoke.sh`).
- **Windows** (`crates/kore-app/packaging/windows/`):
  - `kore.rc` (icon plus VERSIONINFO, ProductName `kore`) is compiled by `crates/kore-app/build.rs` via `embed-resource` when the target OS is Windows;
  - `kore.nsi` is a per-user NSIS installer (`RequestExecutionLevel user`, `$LOCALAPPDATA\Programs\kore`, Start Menu/Desktop shortcuts, uninstaller under `HKCU\…\Uninstall\dev.kore.desktop`);
  - release builds need a Windows host (GPUI's `fxc.exe` shader step).

### 4.4 Platform-specific paths and behavior

| Concern | macOS | Linux | Windows |
|---|---|---|---|
| Events DB (`dirs::data_dir()/kore/events.db`) | `~/Library/Application Support/kore/` | `$XDG_DATA_HOME/kore/` (default `~/.local/share/kore/`) | `%APPDATA%\kore\` (Roaming) |
| App local storage (`crates/kore-app/src/state/storage.rs`) | `<data_dir>/kore/local-storage.json` | same | same |
| Favorites | `~/.kore/favorites.json` on all OSes (inconsistent with the above) | | |
| API keys | Keychain | Secret Service over D-Bus (fails without gnome-keyring/kwallet) | Credential Manager |
| PATH enrichment | Homebrew, `/usr/local`, Python.framework, user bins | `/snap/bin`, Nix, user bins | Node-package dir, WinGet, scoop, chocolatey, krew |
| Tool lookup | PATH | PATH | PATH + `PATHEXT` (`.exe/.cmd/.bat`), `CREATE_NO_WINDOW` |
| `agent --workspace` | `std::env::temp_dir()` | same | same |
| Prompt to `claude`/`agent` | argv | argv (per-arg 128 KiB) | argv. Can exceed the ~32 KiB command-line limit with large cluster context |
| TLS | rustls (kube and reqwest) | rustls, so no OpenSSL runtime dependency for kore-core | rustls |
| Self-update | DMG flow (§4.2) | not supported | not supported |
| Window chrome | transparent titlebar | native | native |

CI (`.github/workflows/ci.yml`) runs clippy (`-D warnings`) and `cargo test -p kore-core -p kore-app` on `macos-latest` only. The release workflow builds macOS, Linux (ubuntu-22.04) and Windows.

---

## 5. Tests

### 5.1 Unit tests (no cluster)

```bash
cargo test -p kore-core -p kore-app
```

kore-core has 32 unit tests, all pure or offline:

| Module | Count | Covers |
|---|---|---|
| `error.rs` | 5 | Display strings, `classify_connection_error` for non-kube variants |
| `events.rs` | 4 | Drop cancels the producer, parent cancel ends the stream, payload JSON shapes, unique ids |
| `tools.rs` | 3 | PATH enrichment keeps existing entries, Windows candidate names, resolve fallback |
| `lib.rs` | 2 | `core_works_without_tokio_context` (offline `Core` from `futures::executor`: `no_kubeconfig`, `ClientMissing`, validation, favorites, event store file created), drop inside an async context |
| `json.rs`, `prefs.rs`, `state/exec.rs`, `state/ai.rs` | 1 each | Key sorting (JSON and YAML), favorites roundtrip, session id shape, `http_error_reason` |
| `state/helm.rs` | 2 | Integer and string revisions |
| `state/event_store.rs` | 2 | Store/query/prune roundtrip, `eventTime`-only events |
| `state/dashboard.rs` | 10 | `compute_health`: healthy/failed/mixed, crashloop, pending reason, hotlist sort and top 10, node Ready, warnings filtered and capped at 20, empty input, clamping |

`crates/kore-core/tests/ai_mock.rs` (1 test, runs by default) starts a local HTTP mock. It checks:
- the `KORE_OPENAI_BASE_URL` / `KORE_ANTHROPIC_BASE_URL` overrides;
- that the Ollama probe honours the configured base URL;
- that AI subscriptions can be consumed from a non-tokio executor.

kore-app has its own unit tests.

### 5.2 Backend tests against kind

```bash
cargo build -p kore-harness && ./target/debug/kore-harness cluster up   # also seeds fixtures
KORE_KIND=1 cargo test -p kore-core -- --include-ignored --test-threads=1
```

- Every test is `#[ignore = "requires kind cluster (KORE_KIND=1)"]` and also returns early unless `KORE_KIND=1` (`require_kind!`).
- `tests/common/mod.rs` builds a `Core` with:
  - `kubeconfig_paths = [harness/.kube/config]`;
  - `initial_context = kind-kore-harness` (the second cluster is `kind-kore-harness-2`);
  - temp `data_dir` and `favorites_dir`.
- Every test drives `Core` from `futures::executor::block_on`. Timers run on the core runtime via `core.spawn`.
- Fixtures are the harness's (`harness/cluster/fixtures`, namespace `kore-demo`; see `harness/README.md`).

| File | Tests |
|---|---|
| `kind_resources.rs` (6) | `connection_and_contexts`, `list_every_kind`, `describe_events_search`, `watch_receives_add_and_delete_with_concurrent_subscriptions`, `yaml_get_diff_apply_and_delete`, `rollback_history_and_restore` |
| `kind_streams.rs` (6) | `logs_fetch_and_stream`, `multi_pod_logs_by_selector`, `exec_echo_resize_exit`, `pod_metrics`, `port_forward_http_get`, `port_forward_uses_selected_context` |
| `kind_backend_fixes.rs` (2) | `list_paginates_past_500_and_watch_still_works` (600 ConfigMaps in a temp namespace: list, label-selected list, all-namespaces list, watch initial sync + a later add, payload `context`/`namespace`), `delete_namespace_without_namespace_argument` |
| `kind_cluster.rs` (9) | `dashboard_health`, `event_store_written_by_list_and_watch_and_pruned`, `crd_discovery_and_instances`, `resource_graph_for_web`, `helm_release_read_only`, `multi_cluster_listing_with_timeout` (adds an unreachable context), `switch_context_cancels_streams`, `rbac_functions`, `network_policy_graph_and_simulation` |

### 5.3 End-to-end harness

Testing has three layers (`CLAUDE.md` "Testing layers"):
- **Layer 1:** headless GPUI tests of the UI against `kore-testkit`'s in-memory API server. `CoreConfig` makes every outside dependency injectable: `connector`, `secrets`, `update_url`, `tool_dir` and `ai_endpoints`.
- **Layers 2 and 3:** `crates/kore-harness`. Layer 2 drives the built app (`target/release/kore` with `--features test-harness`) in the background through its control socket (`KORE_CONTROL_SOCKET`). Layer 3 uses OS-level input and window capture. Both use OCR, run against the same kind clusters with a sandboxed `HOME` and `KUBECONFIG=harness/.kube/config`. Scenarios live in `harness/scenarios/*.yaml`. `kore-harness compare` diffs screenshots against a reference run, and `kore-harness mock-ai` serves deterministic Ollama/OpenAI/Anthropic responses. Usage is in `harness/README.md`; the commands are in `CLAUDE.md`.

### 5.4 Known gaps

- The `auth_failed` / `cluster_unreachable` branches of `classify_connection_error` have no unit test.
- These parsers have no unit tests: RBAC (`check_permission_from_snapshot`, `parse_forbidden_message`), network-policy evaluation, graph building, and the AI SSE/NDJSON/CLI stream-line parsers. They are covered only through kind and harness runs. The graph and network-policy builders are not split into pure functions.
- Debug containers, Helm rollback and `apply` on Secrets have no kind test. The 50 000-object list cap is unit-tested only (`paginate`).
- CLI AI providers (`claude`/`agent`) are not tested. `KORE_CLAUDE` / `KORE_AGENT` can point at a fake script.
- The update checker is untested (the endpoint is not injectable).

---

## Appendix A: Behavioral quirks (preserved or fixed in kore-core)

| # | Quirk | Status in kore-core | Where |
|---|---|---|---|
| A1 | Event store never written | **Fixed.** Events are recorded from Events lists, Events watches and `list_events_for_resource`. Pruned at init and hourly. kore-app passes the Settings "Event Retention" value via `set_event_retention` at startup and on change | `state/mod.rs::record_events`, `state/resources.rs`, `state/watcher.rs`, `lib.rs` |
| A2 | Single global watch / log slot | **Fixed.** One independent `Subscription` per call. `stop_pod_logs_stream` still cancels *all* log streams (single and multi) by design | `state/mod.rs::StreamScopes`, `events.rs` |
| A3 | Watch payload lacks context/ns/id | **Fixed.** `Subscription.id` distinguishes streams, and `WatchEventPayload` carries `context` and `namespace` (additive, `serde(default)`). A context switch still cancels every watch. Events recorded from a watch or list are filed under the context they were read from | `events.rs`, `state/mod.rs`, `state/watcher.rs` |
| A4 | List truncated at 500 without pagination | **Fixed.** `list_resources` follows `continue` tokens (page size 500, per-page timeout/retry, one restart on an expired token) up to a 50 000-object safety cap; `list_resources_full` reports `truncated`. Unit tests `state::resources::tests::paginate_*`, kind test `list_paginates_past_500_and_watch_still_works`, harness `30-cluster-bug-list-pagination` | `state/resources.rs`, `constants.rs`, `lib.rs` |
| A5 | `fetch_logs` argument-shape mismatch | **Moot.** `Core::fetch_logs(LogRequest)` is typed | `lib.rs` |
| A6 | `delete_resource` requires a namespace for cluster-scoped kinds | **Fixed.** `Core::delete_resource` requires only the name for nodes and namespaces. Unit test `delete_cluster_scoped_needs_no_namespace`, kind test `delete_namespace_without_namespace_argument` | `lib.rs`, `state/resources.rs` |
| A7 | Port-forward ignores the selected context; one kubectl per connection | **Context fixed** (`--context <Kore context>` plus `KUBECONFIG` when configured; kind test `port_forward_uses_selected_context`). **Per-connection kubectl preserved**, including the 200 ms delay and no status stream | `state/port_forward.rs` |
| A8 | Exec resize no-op; stdin String-only | **Fixed.** Resize sends `TerminalSize`, `send_exec_bytes` takes raw bytes, and stdout is raw bytes | `state/exec.rs` |
| A9 | Exec session leaks if the client is missing | **Fixed.** The client is fetched before the session is registered, and `SessionGuard` removes the entry synchronously | `state/exec.rs` |
| A10 | AI streams not cancellable; CLI children not `kill_on_drop` | **Fixed.** `select!` on the subscription token, and `tools::command` sets `kill_on_drop(true)`. The diagnose context gathering before the subscription is returned is not cancellable | `state/ai.rs`, `tools.rs` |
| A11 | Multi-cluster calls have no timeout | **Fixed** with a 5 s per-context timeout. Failed or timed-out contexts are **still dropped silently** (log only) | `state/dashboard.rs`, `state/multi_cluster.rs`, `constants.rs` |
| A12 | Dashboard "usage" is really allocatable | **Preserved** | `state/dashboard.rs` |
| A13 | `has_update` is string inequality | **Fixed.** Numeric version comparison (`is_newer`) | `state/update.rs` |
| A14 | Blocking IO in async code | **Mostly fixed.** SQLite, keychain, favorites, the event-store open and the `perform_update` install step (`install_dmg` via `spawn_blocking`; the DMG is now detached on every exit path) run on the blocking pool. **Preserved:** `Kubeconfig::read*` (small sync file reads) | `state/event_store.rs`, `lib.rs`, `state/update.rs` |
| A15 | `switch_context` returns Ok even when unreachable | **Fixed (API).** Still Ok for an unreachable cluster (the UI follows the switch and shows the connection banner), but it now returns the post-switch `ConnectionStatus` with `connected`/`error`/`error_kind`, so a separate `get_connection_status` is optional. Unit test `switch_context_returns_connection_status` | `state/mod.rs`, `lib.rs` |
| A16 | Multi-pod pod list sent separately and never consumed; default container only | **Pod list fixed** (returned synchronously in `MultiPodLogStream.pods`). **Default-container-only preserved** | `state/multi_logs.rs` |
| A17 | Favorites in `~/.kore`, DB in the OS data dir | **Preserved by design** as the defaults (moving them would orphan existing user data), but both are injectable through `CoreConfig`. Favorites writes are now atomic (temp file plus rename) | `config.rs`, `prefs.rs` |
| A18 | Merge patches with `force()` were rejected by kube-rs, so restart and rollback failed | **Fixed.** Field manager `kore` without force. Rollback also drops `pod-template-hash` | `state/resources.rs`, `state/rollback.rs` |
| A19 | `helm history` integer revisions failed to deserialize | **Fixed** (string-or-int) | `state/helm.rs` |
| A20 | Windows PATH handling and `.cmd` shims | **Fixed.** `split_paths`/`join_paths`, `PATHEXT` lookup, `KORE_*` overrides | `tools.rs` |
| A21 | `enrich_path` mutates the process env and should run before threads exist | **Fixed.** It is the first statement of kore-app `main()`, before any thread exists; `backend::init` no longer calls it. Children launched through `tools::command` also get the enriched PATH explicitly | `crates/kore-app/src/main.rs`, `tools.rs` |
