//! In-memory Kubernetes API server, served to kube-rs as a `tower::Service`.
//!
//! It covers what Kore uses:
//! - get, list (with `limit` / `continue` paging), watch, create, replace,
//!   merge patch and delete on any resource whose type is known (built-ins or
//!   registered CRDs);
//! - label and field selectors;
//! - pod logs (static or followed);
//! - injected 403s and latency.
//!
//! Every request is recorded for assertions. Objects are plain JSON, so
//! fixtures can be the same YAML used for the kind cluster.

use bytes::Bytes;
use futures::StreamExt;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, StreamBody};
use kube::client::Body;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

type RespBody = UnsyncBoxBody<Bytes, Infallible>;

/// Where a resource type lives in the API.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceType {
    /// `api/v1` or `apis/<group>/<version>`.
    pub prefix: String,
    pub plural: String,
    pub kind: String,
    pub namespaced: bool,
}

const BUILTINS: &[(&str, &str, &str, bool)] = &[
    ("api/v1", "pods", "Pod", true),
    ("api/v1", "services", "Service", true),
    ("api/v1", "configmaps", "ConfigMap", true),
    ("api/v1", "secrets", "Secret", true),
    ("api/v1", "events", "Event", true),
    ("api/v1", "serviceaccounts", "ServiceAccount", true),
    ("api/v1", "namespaces", "Namespace", false),
    ("api/v1", "nodes", "Node", false),
    ("api/v1", "persistentvolumeclaims", "PersistentVolumeClaim", true),
    ("apis/apps/v1", "deployments", "Deployment", true),
    ("apis/apps/v1", "replicasets", "ReplicaSet", true),
    ("apis/apps/v1", "statefulsets", "StatefulSet", true),
    ("apis/apps/v1", "daemonsets", "DaemonSet", true),
    ("apis/apps/v1", "controllerrevisions", "ControllerRevision", true),
    ("apis/batch/v1", "jobs", "Job", true),
    ("apis/batch/v1", "cronjobs", "CronJob", true),
    ("apis/networking.k8s.io/v1", "ingresses", "Ingress", true),
    ("apis/networking.k8s.io/v1", "networkpolicies", "NetworkPolicy", true),
    ("apis/rbac.authorization.k8s.io/v1", "roles", "Role", true),
    ("apis/rbac.authorization.k8s.io/v1", "rolebindings", "RoleBinding", true),
    ("apis/rbac.authorization.k8s.io/v1", "clusterroles", "ClusterRole", false),
    ("apis/rbac.authorization.k8s.io/v1", "clusterrolebindings", "ClusterRoleBinding", false),
    ("apis/apiextensions.k8s.io/v1", "customresourcedefinitions", "CustomResourceDefinition", false),
    ("apis/metrics.k8s.io/v1beta1", "pods", "PodMetrics", true),
    ("apis/metrics.k8s.io/v1beta1", "nodes", "NodeMetrics", false),
];

/// A watch event as sent on the wire.
#[derive(Debug, Clone)]
struct WatchEvent {
    rtype: ResourceType,
    namespace: Option<String>,
    kind: &'static str,
    object: Value,
}

/// One recorded API call.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub body: Option<Value>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct Forbid {
    verb: String,
    plural: String,
    namespace: Option<String>,
}

type LogKey = (String, String, String);

#[derive(Default)]
struct State {
    types: Vec<ResourceType>,
    objects: BTreeMap<(ResourceType, String, String), Value>,
    resource_version: u64,
    requests: Vec<RecordedRequest>,
    forbid: Vec<Forbid>,
    logs: HashMap<LogKey, Vec<String>>,
    latency: Option<Duration>,
    /// Requests never answer (an unreachable API server).
    hang: bool,
}

/// An in-memory cluster. Cheap to clone (shared state).
#[derive(Clone)]
pub struct FakeCluster {
    state: Arc<Mutex<State>>,
    events: broadcast::Sender<WatchEvent>,
    log_lines: broadcast::Sender<(LogKey, String)>,
}

impl Default for FakeCluster {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeCluster {
    pub fn new() -> Self {
        let mut st = State::default();
        for (prefix, plural, kind, namespaced) in BUILTINS {
            st.types.push(ResourceType {
                prefix: prefix.to_string(),
                plural: plural.to_string(),
                kind: kind.to_string(),
                namespaced: *namespaced,
            });
        }
        let (events, _) = broadcast::channel(4096);
        let (log_lines, _) = broadcast::channel(4096);
        FakeCluster { state: Arc::new(Mutex::new(st)), events, log_lines }
    }

    /// Register a custom resource type (also done automatically when a CRD object is added).
    pub fn register_type(&self, group: &str, version: &str, plural: &str, kind: &str, namespaced: bool) {
        let rt = ResourceType {
            prefix: format!("apis/{group}/{version}"),
            plural: plural.into(),
            kind: kind.into(),
            namespaced,
        };
        let mut st = self.state.lock().unwrap();
        if !st.types.contains(&rt) {
            st.types.push(rt);
        }
    }

    fn type_of(&self, obj: &Value) -> ResourceType {
        let api_version = obj["apiVersion"].as_str().unwrap_or("v1");
        let kind = obj["kind"].as_str().unwrap_or_default();
        let prefix = if api_version.contains('/') { format!("apis/{api_version}") } else { format!("api/{api_version}") };
        let st = self.state.lock().unwrap();
        st.types
            .iter()
            .find(|t| t.prefix == prefix && t.kind == kind)
            .cloned()
            .unwrap_or_else(|| panic!("FakeCluster: unknown type {api_version} {kind}; call register_type first"))
    }

    /// Add (or replace) an object, emitting a watch event. Returns the stored object.
    pub fn apply(&self, mut obj: Value) -> Value {
        if obj["kind"] == "CustomResourceDefinition" {
            let spec = &obj["spec"];
            let group = spec["group"].as_str().unwrap_or_default();
            let plural = spec["names"]["plural"].as_str().unwrap_or_default();
            let kind = spec["names"]["kind"].as_str().unwrap_or_default();
            let namespaced = spec["scope"] != "Cluster";
            for v in spec["versions"].as_array().into_iter().flatten() {
                self.register_type(group, v["name"].as_str().unwrap_or("v1"), plural, kind, namespaced);
            }
        }
        let rt = self.type_of(&obj);
        let ns = if rt.namespaced {
            obj["metadata"]["namespace"].as_str().unwrap_or("default").to_string()
        } else {
            String::new()
        };
        let name = obj["metadata"]["name"].as_str().expect("object needs metadata.name").to_string();
        let mut st = self.state.lock().unwrap();
        st.resource_version += 1;
        let rv = st.resource_version.to_string();
        let key = (rt.clone(), ns.clone(), name.clone());
        let existed = st.objects.contains_key(&key);
        let meta = obj["metadata"].as_object_mut().expect("metadata");
        if rt.namespaced {
            meta.insert("namespace".into(), json!(ns));
        }
        meta.insert("resourceVersion".into(), json!(rv));
        meta.entry("uid").or_insert_with(|| json!(format!("uid-{}-{name}", st.resource_version)));
        meta.entry("creationTimestamp").or_insert_with(|| json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)));
        st.objects.insert(key, obj.clone());
        drop(st);
        let _ = self.events.send(WatchEvent {
            rtype: rt.clone(),
            namespace: rt.namespaced.then_some(ns),
            kind: if existed { "MODIFIED" } else { "ADDED" },
            object: obj.clone(),
        });
        obj
    }

    /// Add every document of a multi-document YAML string.
    pub fn apply_yaml(&self, yaml: &str) {
        for doc in serde_yaml::Deserializer::from_str(yaml) {
            let v: Value = serde::Deserialize::deserialize(doc).expect("fixture YAML");
            if v.is_null() {
                continue;
            }
            if v["kind"] == "List" {
                for item in v["items"].as_array().into_iter().flatten() {
                    self.apply(item.clone());
                }
            } else {
                self.apply(v);
            }
        }
    }

    /// Change an object in place (e.g. a pod's status) and emit MODIFIED.
    pub fn modify(&self, kind: &str, namespace: Option<&str>, name: &str, f: impl FnOnce(&mut Value)) {
        let mut obj = self.get(kind, namespace, name).unwrap_or_else(|| panic!("no {kind} {name}"));
        f(&mut obj);
        self.apply(obj);
    }

    /// Remove an object, emitting DELETED. Returns whether it existed.
    pub fn delete(&self, kind: &str, namespace: Option<&str>, name: &str) -> bool {
        let mut st = self.state.lock().unwrap();
        let Some(key) = st
            .objects
            .keys()
            .find(|(t, ns, n)| t.kind == kind && n == name && (!t.namespaced || Some(ns.as_str()) == namespace))
            .cloned()
        else {
            return false;
        };
        let obj = st.objects.remove(&key).unwrap();
        st.resource_version += 1;
        drop(st);
        let _ = self.events.send(WatchEvent {
            rtype: key.0.clone(),
            namespace: key.0.namespaced.then_some(key.1),
            kind: "DELETED",
            object: obj,
        });
        true
    }

    pub fn get(&self, kind: &str, namespace: Option<&str>, name: &str) -> Option<Value> {
        let st = self.state.lock().unwrap();
        st.objects
            .iter()
            .find(|((t, ns, n), _)| t.kind == kind && n == name && (!t.namespaced || Some(ns.as_str()) == namespace))
            .map(|(_, v)| v.clone())
    }

    pub fn list(&self, kind: &str, namespace: Option<&str>) -> Vec<Value> {
        let st = self.state.lock().unwrap();
        st.objects
            .iter()
            .filter(|((t, ns, _), _)| t.kind == kind && (namespace.is_none() || !t.namespaced || Some(ns.as_str()) == namespace))
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Set a container's log lines. `append_log` streams further lines to followers.
    pub fn set_logs(&self, namespace: &str, pod: &str, container: &str, lines: &[&str]) {
        let mut st = self.state.lock().unwrap();
        st.logs.insert((namespace.into(), pod.into(), container.into()), lines.iter().map(|s| s.to_string()).collect());
    }

    pub fn append_log(&self, namespace: &str, pod: &str, container: &str, line: &str) {
        let key: LogKey = (namespace.into(), pod.into(), container.into());
        self.state.lock().unwrap().logs.entry(key.clone()).or_default().push(line.into());
        let _ = self.log_lines.send((key, line.into()));
    }

    /// Answer `verb` (`get`, `list`, `watch`, `create`, `patch`, `update`, `delete`) on `plural`
    /// (optionally only in `namespace`) with 403 Forbidden.
    pub fn forbid(&self, verb: &str, plural: &str, namespace: Option<&str>) {
        self.state.lock().unwrap().forbid.push(Forbid {
            verb: verb.into(),
            plural: plural.into(),
            namespace: namespace.map(Into::into),
        });
    }

    pub fn allow_all(&self) {
        self.state.lock().unwrap().forbid.clear();
    }

    pub fn set_latency(&self, latency: Option<Duration>) {
        self.state.lock().unwrap().latency = latency;
    }

    /// Never answer any request (simulates a cluster whose connection hangs).
    pub fn set_hang(&self, hang: bool) {
        self.state.lock().unwrap().hang = hang;
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state.lock().unwrap().requests.clone()
    }

    pub fn clear_requests(&self) {
        self.state.lock().unwrap().requests.clear();
    }

    /// A kube-rs client backed by this cluster. Must be called inside a tokio runtime.
    pub fn client(&self, default_namespace: &str) -> kube::Client {
        let this = self.clone();
        let svc = tower::service_fn(move |req: Request<Body>| {
            let this = this.clone();
            async move { Ok::<_, Infallible>(this.handle(req).await) }
        });
        kube::Client::new(svc, default_namespace)
    }

    async fn handle(&self, req: Request<Body>) -> Response<RespBody> {
        let (parts, body) = req.into_parts();
        let bytes = body.collect().await.map(|c| c.to_bytes()).unwrap_or_default();
        let body_json: Option<Value> = if bytes.is_empty() {
            None
        } else {
            serde_json::from_slice(&bytes).ok().or_else(|| serde_yaml::from_slice(&bytes).ok())
        };
        let path = parts.uri.path().to_string();
        let query: BTreeMap<String, String> = parts
            .uri
            .query()
            .map(|q| {
                q.split('&')
                    .map(|kv| {
                        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                        (decode(k), decode(v))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let headers = parts
            .headers
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();
        let (latency, hang) = {
            let mut st = self.state.lock().unwrap();
            st.requests.push(RecordedRequest {
                method: parts.method.to_string(),
                path: path.clone(),
                query: query.clone(),
                body: body_json.clone(),
                headers,
            });
            (st.latency, st.hang)
        };
        if hang {
            futures::future::pending::<()>().await;
        }
        if let Some(d) = latency {
            tokio::time::sleep(d).await;
        }
        self.route(&parts.method, &path, &query, body_json, &parts.headers)
    }

    fn route(
        &self,
        method: &Method,
        path: &str,
        query: &BTreeMap<String, String>,
        body: Option<Value>,
        headers: &http::HeaderMap,
    ) -> Response<RespBody> {
        if path == "/version" {
            return ok(json!({"major": "1", "minor": "29", "gitVersion": "v1.29.0-fake"}));
        }
        let Some((prefix, rest)) = split_prefix(path) else {
            return status(StatusCode::NOT_FOUND, "NotFound", &format!("no route {path}"));
        };
        if rest.is_empty() {
            return self.discovery(&prefix);
        }
        let (namespace, plural, name, sub) = match rest.as_slice() {
            [ns_kw, ns, plural, tail @ ..] if ns_kw == "namespaces" && !ns.is_empty() => {
                (Some(ns.clone()), plural.clone(), tail.first().cloned(), tail.get(1).cloned())
            }
            [plural, tail @ ..] => (None, plural.clone(), tail.first().cloned(), tail.get(1).cloned()),
            [] => unreachable!(),
        };
        let rtype = {
            let st = self.state.lock().unwrap();
            st.types.iter().find(|t| t.prefix == prefix && t.plural == plural).cloned()
        };
        let Some(rtype) = rtype else {
            return status(StatusCode::NOT_FOUND, "NotFound", &format!("the server could not find the requested resource ({plural})"));
        };
        let watch = query.get("watch").is_some_and(|v| v == "true" || v == "1");
        let verb = match (method.as_str(), name.is_some(), watch) {
            ("GET", _, true) => "watch",
            ("GET", true, _) => "get",
            ("GET", false, _) => "list",
            ("POST", _, _) => "create",
            ("PUT", _, _) => "update",
            ("PATCH", _, _) => "patch",
            ("DELETE", _, _) => "delete",
            _ => "unknown",
        };
        if self.forbidden(verb, &plural, namespace.as_deref()) {
            let msg = format!(
                "{plural} is forbidden: User \"fake\" cannot {verb} resource \"{plural}\" in API group \"{}\"{}",
                group_of(&prefix),
                namespace.as_ref().map(|n| format!(" in the namespace \"{n}\"")).unwrap_or_else(|| " at the cluster scope".into())
            );
            return status(StatusCode::FORBIDDEN, "Forbidden", &msg);
        }
        match (sub.as_deref(), verb) {
            (Some("log"), "get") => self.pod_log(namespace.as_deref().unwrap_or("default"), name.as_deref().unwrap_or_default(), query),
            (_, "watch") => self.watch(rtype, namespace, query),
            (_, "list") => self.list_response(&rtype, namespace.as_deref(), query),
            (_, "get") => match self.find(&rtype, namespace.as_deref(), name.as_deref().unwrap()) {
                Some(obj) => match sub.as_deref() {
                    Some("scale") => ok(scale_of(&obj)),
                    _ => ok(obj),
                },
                None => not_found(&rtype, name.as_deref().unwrap()),
            },
            (_, "create") => {
                let Some(mut obj) = body else { return status(StatusCode::BAD_REQUEST, "BadRequest", "missing body") };
                if rtype.namespaced {
                    obj["metadata"]["namespace"] = json!(namespace.clone().unwrap_or_else(|| "default".into()));
                }
                let n = obj["metadata"]["name"].as_str().unwrap_or_default().to_string();
                if self.find(&rtype, namespace.as_deref(), &n).is_some() {
                    return status(StatusCode::CONFLICT, "AlreadyExists", &format!("{} \"{n}\" already exists", rtype.plural));
                }
                created(self.apply(obj))
            }
            (_, "update") => {
                let Some(obj) = body else { return status(StatusCode::BAD_REQUEST, "BadRequest", "missing body") };
                ok(self.apply(obj))
            }
            (_, "patch") => {
                let name = name.unwrap();
                let content_type = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default();
                let Some(patch) = body else { return status(StatusCode::BAD_REQUEST, "BadRequest", "missing body") };
                let current = self.find(&rtype, namespace.as_deref(), &name);
                let mut obj = match current {
                    Some(o) => o,
                    None if content_type.contains("apply-patch") => patch.clone(),
                    None => return not_found(&rtype, &name),
                };
                if content_type.contains("json-patch") {
                    return status(StatusCode::UNSUPPORTED_MEDIA_TYPE, "UnsupportedMediaType", "FakeCluster: JSON patch not supported");
                }
                if sub.as_deref() == Some("scale") {
                    obj["spec"]["replicas"] = patch["spec"]["replicas"].clone();
                } else {
                    merge(&mut obj, &patch);
                }
                if rtype.namespaced {
                    obj["metadata"]["namespace"] = json!(namespace.clone().unwrap_or_else(|| "default".into()));
                }
                let stored = self.apply(obj);
                if sub.as_deref() == Some("scale") { ok(scale_of(&stored)) } else { ok(stored) }
            }
            (_, "delete") => {
                let name = name.unwrap();
                match self.find(&rtype, namespace.as_deref(), &name) {
                    Some(obj) => {
                        self.delete(&rtype.kind, namespace.as_deref(), &name);
                        ok(obj)
                    }
                    None => not_found(&rtype, &name),
                }
            }
            _ => status(StatusCode::METHOD_NOT_ALLOWED, "MethodNotAllowed", verb),
        }
    }

    fn forbidden(&self, verb: &str, plural: &str, namespace: Option<&str>) -> bool {
        let st = self.state.lock().unwrap();
        st.forbid.iter().any(|f| {
            (f.verb == "*" || f.verb == verb)
                && (f.plural == "*" || f.plural == plural)
                && (f.namespace.is_none() || f.namespace.as_deref() == namespace)
        })
    }

    fn find(&self, rtype: &ResourceType, namespace: Option<&str>, name: &str) -> Option<Value> {
        let ns = if rtype.namespaced { namespace.unwrap_or("default").to_string() } else { String::new() };
        self.state.lock().unwrap().objects.get(&(rtype.clone(), ns, name.to_string())).cloned()
    }

    fn matching(&self, rtype: &ResourceType, namespace: Option<&str>, query: &BTreeMap<String, String>) -> Vec<Value> {
        let st = self.state.lock().unwrap();
        st.objects
            .iter()
            .filter(|((t, ns, _), _)| t == rtype && (namespace.is_none() || Some(ns.as_str()) == namespace))
            .map(|(_, v)| v.clone())
            .filter(|o| query.get("labelSelector").is_none_or(|s| label_selector_matches(s, &o["metadata"]["labels"])))
            .filter(|o| query.get("fieldSelector").is_none_or(|s| field_selector_matches(s, o)))
            .collect()
    }

    fn list_response(&self, rtype: &ResourceType, namespace: Option<&str>, query: &BTreeMap<String, String>) -> Response<RespBody> {
        let items = self.matching(rtype, namespace, query);
        let start: usize = query.get("continue").and_then(|c| c.parse().ok()).unwrap_or(0);
        let limit: usize = query.get("limit").and_then(|l| l.parse().ok()).filter(|&l| l > 0).unwrap_or(usize::MAX);
        let end = start.saturating_add(limit).min(items.len());
        let page: Vec<Value> = items[start.min(items.len())..end].to_vec();
        let rv = self.state.lock().unwrap().resource_version.to_string();
        let mut metadata = json!({"resourceVersion": rv});
        if end < items.len() {
            metadata["continue"] = json!(end.to_string());
            metadata["remainingItemCount"] = json!(items.len() - end);
        }
        let list_kind = format!("{}List", rtype.kind);
        ok(json!({
            "apiVersion": api_version_of(&rtype.prefix),
            "kind": list_kind,
            "metadata": metadata,
            "items": page,
        }))
    }

    fn watch(&self, rtype: ResourceType, namespace: Option<String>, query: &BTreeMap<String, String>) -> Response<RespBody> {
        let label = query.get("labelSelector").cloned();
        let field = query.get("fieldSelector").cloned();
        let rx = self.events.subscribe();
        let stream = tokio_stream_from(rx).filter_map(move |ev| {
            let keep = ev.rtype == rtype
                && (namespace.is_none() || ev.namespace == namespace)
                && label.as_ref().is_none_or(|s| label_selector_matches(s, &ev.object["metadata"]["labels"]))
                && field.as_ref().is_none_or(|s| field_selector_matches(s, &ev.object));
            futures::future::ready(keep.then(|| {
                let mut line = serde_json::to_vec(&json!({"type": ev.kind, "object": ev.object})).unwrap();
                line.push(b'\n');
                Ok::<_, Infallible>(http_body::Frame::data(Bytes::from(line)))
            }))
        });
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(StreamBody::new(stream).boxed_unsync())
            .unwrap()
    }

    fn pod_log(&self, namespace: &str, pod: &str, query: &BTreeMap<String, String>) -> Response<RespBody> {
        let container = query.get("container").cloned().unwrap_or_else(|| {
            self.find(&self.pod_type(), Some(namespace), pod)
                .and_then(|p| p["spec"]["containers"][0]["name"].as_str().map(String::from))
                .unwrap_or_default()
        });
        let key: LogKey = (namespace.into(), pod.into(), container);
        let mut lines = self.state.lock().unwrap().logs.get(&key).cloned().unwrap_or_default();
        if let Some(tail) = query.get("tailLines").and_then(|t| t.parse::<usize>().ok()) {
            let skip = lines.len().saturating_sub(tail);
            lines.drain(..skip);
        }
        let mut text = String::new();
        for l in &lines {
            text.push_str(l);
            text.push('\n');
        }
        let follow = query.get("follow").is_some_and(|v| v == "true");
        if !follow {
            return Response::builder().status(StatusCode::OK).body(Full::new(Bytes::from(text)).boxed_unsync()).unwrap();
        }
        let initial = futures::stream::once(futures::future::ready(Ok::<_, Infallible>(http_body::Frame::data(Bytes::from(text)))));
        let rx = self.log_lines.subscribe();
        let live = tokio_stream_from(rx).filter_map(move |(k, line)| {
            futures::future::ready((k == key).then(|| Ok(http_body::Frame::data(Bytes::from(format!("{line}\n"))))))
        });
        Response::builder().status(StatusCode::OK).body(StreamBody::new(initial.chain(live)).boxed_unsync()).unwrap()
    }

    fn pod_type(&self) -> ResourceType {
        self.state.lock().unwrap().types.iter().find(|t| t.kind == "Pod").cloned().unwrap()
    }

    fn discovery(&self, prefix: &str) -> Response<RespBody> {
        let st = self.state.lock().unwrap();
        if prefix == "api" {
            return ok(json!({"kind": "APIVersions", "versions": ["v1"]}));
        }
        if prefix == "apis" {
            let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for t in &st.types {
                if let Some(gv) = t.prefix.strip_prefix("apis/") {
                    let (g, v) = gv.split_once('/').unwrap();
                    let vs = groups.entry(g.to_string()).or_default();
                    if !vs.contains(&v.to_string()) {
                        vs.push(v.to_string());
                    }
                }
            }
            let groups: Vec<Value> = groups
                .into_iter()
                .map(|(g, vs)| {
                    let versions: Vec<Value> = vs.iter().map(|v| json!({"groupVersion": format!("{g}/{v}"), "version": v})).collect();
                    json!({"name": g, "versions": versions, "preferredVersion": versions[0]})
                })
                .collect();
            return ok(json!({"kind": "APIGroupList", "apiVersion": "v1", "groups": groups}));
        }
        let resources: Vec<Value> = st
            .types
            .iter()
            .filter(|t| t.prefix == prefix)
            .map(|t| {
                json!({
                    "name": t.plural, "singularName": t.kind.to_lowercase(), "namespaced": t.namespaced, "kind": t.kind,
                    "verbs": ["get", "list", "watch", "create", "update", "patch", "delete"],
                })
            })
            .collect();
        ok(json!({"kind": "APIResourceList", "apiVersion": "v1", "groupVersion": api_version_of(prefix), "resources": resources}))
    }
}

fn tokio_stream_from<T: Clone + Send + 'static>(rx: broadcast::Receiver<T>) -> impl futures::Stream<Item = T> + Send {
    futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(v) => return Some((v, rx)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

fn split_prefix(path: &str) -> Option<(String, Vec<String>)> {
    let segs: Vec<String> = path.trim_matches('/').split('/').map(decode).collect();
    match segs.first().map(String::as_str) {
        Some("api") if segs.len() == 1 => Some(("api".into(), vec![])),
        Some("api") => Some((format!("api/{}", segs[1]), segs[2..].to_vec())),
        Some("apis") if segs.len() == 1 => Some(("apis".into(), vec![])),
        Some("apis") if segs.len() >= 3 => Some((format!("apis/{}/{}", segs[1], segs[2]), segs[3..].to_vec())),
        _ => None,
    }
}

fn group_of(prefix: &str) -> String {
    prefix.strip_prefix("apis/").and_then(|gv| gv.split('/').next()).unwrap_or("").to_string()
}

fn api_version_of(prefix: &str) -> String {
    prefix.strip_prefix("apis/").or_else(|| prefix.strip_prefix("api/")).unwrap_or(prefix).to_string()
}

fn decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// JSON merge patch (RFC 7386); `null` deletes a key.
pub fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

fn scale_of(obj: &Value) -> Value {
    json!({
        "apiVersion": "autoscaling/v1", "kind": "Scale",
        "metadata": {"name": obj["metadata"]["name"], "namespace": obj["metadata"]["namespace"]},
        "spec": {"replicas": obj["spec"]["replicas"]},
        "status": {"replicas": obj["status"]["replicas"]},
    })
}

/// Equality- and set-based label selectors: `a=b`, `a==b`, `a!=b`, `a`, `!a`, `a in (x,y)`, `a notin (x)`.
pub fn label_selector_matches(selector: &str, labels: &Value) -> bool {
    split_top_level(selector).iter().all(|req| {
        let req = req.trim();
        if req.is_empty() {
            return true;
        }
        let get = |k: &str| labels.get(k.trim()).and_then(Value::as_str).map(str::to_string);
        if let Some((k, vals)) = req.split_once(" notin ") {
            let set = parse_set(vals);
            return get(k).is_none_or(|v| !set.contains(&v));
        }
        if let Some((k, vals)) = req.split_once(" in ") {
            let set = parse_set(vals);
            return get(k).is_some_and(|v| set.contains(&v));
        }
        if let Some((k, v)) = req.split_once("!=") {
            return get(k).as_deref() != Some(v.trim());
        }
        if let Some((k, v)) = req.split_once("==").or_else(|| req.split_once('=')) {
            return get(k).as_deref() == Some(v.trim());
        }
        if let Some(k) = req.strip_prefix('!') {
            return get(k).is_none();
        }
        get(req).is_some()
    })
}

fn split_top_level(s: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(String::new());
                continue;
            }
            _ => {}
        }
        out.last_mut().unwrap().push(c);
    }
    out
}

fn parse_set(s: &str) -> Vec<String> {
    s.trim().trim_start_matches('(').trim_end_matches(')').split(',').map(|v| v.trim().to_string()).collect()
}

/// Field selectors on dotted paths: `metadata.name=x,involvedObject.uid!=y`.
pub fn field_selector_matches(selector: &str, obj: &Value) -> bool {
    selector.split(',').all(|req| {
        let (path, want, negate) = match req.split_once("!=") {
            Some((p, v)) => (p, v, true),
            None => match req.split_once("==").or_else(|| req.split_once('=')) {
                Some((p, v)) => (p, v, false),
                None => return true,
            },
        };
        let mut cur = obj;
        for seg in path.trim().split('.') {
            cur = &cur[seg];
        }
        let have = match cur {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        (have == want.trim()) != negate
    })
}

fn ok(v: Value) -> Response<RespBody> {
    json_response(StatusCode::OK, &v)
}

fn created(v: Value) -> Response<RespBody> {
    json_response(StatusCode::CREATED, &v)
}

fn json_response(code: StatusCode, v: &Value) -> Response<RespBody> {
    Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(serde_json::to_vec(v).unwrap())).boxed_unsync())
        .unwrap()
}

fn status(code: StatusCode, reason: &str, message: &str) -> Response<RespBody> {
    json_response(
        code,
        &json!({"kind": "Status", "apiVersion": "v1", "status": "Failure", "message": message, "reason": reason, "code": code.as_u16()}),
    )
}

fn not_found(rtype: &ResourceType, name: &str) -> Response<RespBody> {
    status(StatusCode::NOT_FOUND, "NotFound", &format!("{} \"{name}\" not found", rtype.plural))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors() {
        let l = json!({"app": "web", "tier": "fe"});
        assert!(label_selector_matches("app=web", &l));
        assert!(label_selector_matches("app=web,tier", &l));
        assert!(!label_selector_matches("app!=web", &l));
        assert!(label_selector_matches("app in (web,api),!missing", &l));
        assert!(!label_selector_matches("tier notin (fe)", &l));
        let o = json!({"metadata": {"name": "x"}, "involvedObject": {"kind": "Pod"}});
        assert!(field_selector_matches("metadata.name=x,involvedObject.kind=Pod", &o));
        assert!(!field_selector_matches("metadata.name!=x", &o));
    }

    #[test]
    fn decode_percent() {
        assert_eq!(decode("app%3Dweb%2Ctier"), "app=web,tier");
    }
}
