//! Resource list + live watch (`useResourceWatch`, spec 01 §7.1), sorting
//! (§7.4), search filtering and restart history (§7.10, Fix 14).

use super::resource_item::{parse_ts, status_variant, to_resource_item, ResourceItem, StatusVariant};
use crate::backend;
use gpui_kit::*;
use kore_core::{ResourceKind, WatchEventPayload};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Parameters a refresh depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub kind: ResourceKind,
    /// `"*"` = all namespaces.
    pub namespace: String,
    pub label_selector: Option<String>,
    pub multi_cluster: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    pub id: String,
    pub desc: bool,
}

pub struct ResourceStore {
    pub rows: Vec<ResourceItem>,
    pub loading: bool,
    /// Bumped on every change of `rows`.
    pub version: u64,
    pub query: Option<Query>,
    generation: u64,
    loading_task: Option<Task<()>>,
    watch_task: Option<Task<()>>,
    restart_history: HashMap<String, RestartEntry>,
    cache: RefCell<Option<VisibleCache>>,
}

struct VisibleCache {
    version: u64,
    search: String,
    sorting: Vec<SortKey>,
    status: Option<StatusVariant>,
    rows: Rc<Vec<ResourceItem>>,
}

pub enum ResourceStoreEvent {
    Changed,
}

impl EventEmitter<ResourceStoreEvent> for ResourceStore {}

#[derive(Default)]
struct RestartEntry {
    timestamps: Vec<Instant>,
    count: i64,
}

const MAX_POINTS: usize = 20;
const MAX_AGE: Duration = Duration::from_secs(3600);

impl ResourceStore {
    pub fn new() -> Self {
        ResourceStore {
            rows: Vec::new(),
            loading: false,
            version: 0,
            query: None,
            generation: 0,
            loading_task: None,
            watch_task: None,
            restart_history: HashMap::new(),
            cache: RefCell::new(None),
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.version += 1;
        cx.emit(ResourceStoreEvent::Changed);
        cx.notify();
    }

    /// Clear the list and stop the watch (context switch in progress).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.loading_task = None;
        self.watch_task = None;
        self.rows.clear();
        self.changed(cx);
    }

    /// `refresh()`: loading, clear, 500ms, list, 100ms, watch, done.
    pub fn refresh(&mut self, query: Query, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.watch_task = None;
        self.loading = true;
        self.rows.clear();
        self.query = Some(query.clone());
        self.changed(cx);

        let core = backend::core(cx);
        let delay = backend::sleep(cx, Duration::from_millis(500));
        let delay2 = backend::sleep(cx, Duration::from_millis(100));
        self.loading_task = Some(cx.spawn(async move |this, cx| {
            delay.await;
            let ns = (query.namespace != "*").then(|| query.namespace.clone());
            let res = if query.multi_cluster {
                core.list_resources_multi_cluster(query.kind, ns.clone(), query.label_selector.clone()).await
            } else {
                core.list_resources(query.kind, ns.clone(), query.label_selector.clone()).await
            };
            let ok = this
                .update(cx, |this, cx| {
                    if this.generation != generation {
                        return false;
                    }
                    match res {
                        Ok(items) => {
                            this.rows = items.iter().filter_map(to_resource_item).collect();
                            this.record_restarts_all();
                        }
                        Err(e) => tracing::error!("Failed to refresh resources: {e}"),
                    }
                    this.changed(cx);
                    true
                })
                .unwrap_or(false);
            if !ok {
                return;
            }
            if !query.multi_cluster {
                delay2.await;
                match core.start_watch(query.kind, ns, query.label_selector.clone()).await {
                    Ok(sub) => {
                        this.update(cx, |this, cx| {
                            if this.generation == generation {
                                this.start_watch_loop(sub, query.kind, cx);
                            }
                        })
                        .ok();
                    }
                    Err(e) => tracing::error!("Failed to start watch: {e}"),
                }
            }
            this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.loading = false;
                    this.changed(cx);
                }
            })
            .ok();
        }));
    }

    fn start_watch_loop(
        &mut self,
        mut sub: kore_core::Subscription<WatchEventPayload>,
        kind: ResourceKind,
        cx: &mut Context<Self>,
    ) {
        self.watch_task = Some(cx.spawn(async move |this, cx| {
            while let Some(first) = sub.recv().await {
                // Coalesce everything already queued into one UI update.
                let mut batch = vec![first];
                batch.extend(sub.try_recv_batch(4096));
                let alive = this
                    .update(cx, |this, cx| {
                        this.apply_events(batch, kind);
                        this.changed(cx);
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        }));
    }

    fn apply_events(&mut self, batch: Vec<WatchEventPayload>, kind: ResourceKind) {
        for ev in batch {
            if ev.kind != kind.as_str() {
                continue;
            }
            let Some(item) = to_resource_item(&ev.object) else { continue };
            let pos = self.rows.iter().position(|r| r.name == item.name && r.namespace == item.namespace);
            if ev.action == "deleted" {
                if let Some(i) = pos {
                    self.rows.remove(i);
                }
            } else {
                if kind == ResourceKind::Pods {
                    let prev = pos.and_then(|i| self.rows[i].restarts);
                    self.record_restart(&item, prev);
                }
                match pos {
                    Some(i) => self.rows[i] = item,
                    None => self.rows.push(item),
                }
            }
        }
    }

    // ── Restart history (Fix 14: actually recorded) ─────────────────────

    fn record_restarts_all(&mut self) {
        if self.query.as_ref().map(|q| q.kind) != Some(ResourceKind::Pods) {
            return;
        }
        let rows = std::mem::take(&mut self.rows);
        for r in &rows {
            let key = format!("{}/{}", r.namespace, r.name);
            let prev = self.restart_history.get(&key).map(|e| e.count);
            self.record_restart(r, prev);
        }
        self.rows = rows;
    }

    /// Records a data point whenever a pod's restart count is first seen
    /// non-zero or increases.
    fn record_restart(&mut self, item: &ResourceItem, prev: Option<i64>) {
        let Some(count) = item.restarts else { return };
        let key = format!("{}/{}", item.namespace, item.name);
        let entry = self.restart_history.entry(key).or_default();
        let known = if entry.timestamps.is_empty() { prev } else { Some(entry.count) };
        // First sight of a restarted pod and every observed increase are recorded.
        let should = match known {
            None => count > 0,
            Some(p) => count > p,
        };
        entry.count = count;
        if !should {
            return;
        }
        let now = Instant::now();
        entry.timestamps.retain(|t| now.duration_since(*t) < MAX_AGE);
        entry.timestamps.push(now);
        if entry.timestamps.len() > MAX_POINTS {
            let drop = entry.timestamps.len() - MAX_POINTS;
            entry.timestamps.drain(..drop);
        }
    }

    /// `getHistory(ns, pod)` → per-point counts (oldest first).
    pub fn restart_history(&self, namespace: &str, pod: &str) -> Vec<i64> {
        let Some(e) = self.restart_history.get(&format!("{namespace}/{pod}")) else { return Vec::new() };
        let now = Instant::now();
        let ts: Vec<_> = e.timestamps.iter().filter(|t| now.duration_since(**t) < MAX_AGE).collect();
        let len = ts.len() as i64;
        (0..len).map(|i| (e.count - (len - 1 - i)).max(0)).collect()
    }

    // ── Filtering + sorting ─────────────────────────────────────────────

    /// Rows after the header search filter (name / namespace / status), in
    /// the current sort order. Cached per (version, search, sorting).
    pub fn visible(&self, search: &str, sorting: &[SortKey]) -> Rc<Vec<ResourceItem>> {
        self.visible_filtered(search, sorting, None)
    }

    /// [`visible`](Self::visible) plus the palette status quick filter
    /// (`:running` / `:failed` / `:pending`): only rows whose status badge has
    /// that variant (spec 01 §7.7, §14).
    pub fn visible_filtered(&self, search: &str, sorting: &[SortKey], status: Option<StatusVariant>) -> Rc<Vec<ResourceItem>> {
        if let Some(c) = self.cache.borrow().as_ref() {
            if c.version == self.version && c.search == search && c.sorting == sorting && c.status == status {
                return c.rows.clone();
            }
        }
        let lower = search.to_lowercase();
        let mut rows: Vec<ResourceItem> = self
            .rows
            .iter()
            .filter(|r| {
                search.is_empty()
                    || r.name.to_lowercase().contains(&lower)
                    || (!r.namespace.is_empty() && r.namespace.to_lowercase().contains(&lower))
                    || r.status.as_ref().map(|s| s.to_lowercase().contains(&lower)).unwrap_or(false)
            })
            .filter(|r| status.is_none_or(|v| r.status.is_some() && status_variant(r.status.as_deref()) == v))
            .cloned()
            .collect();
        if !sorting.is_empty() {
            // Comparator per column chosen from the first (unsorted) row, like TanStack "auto".
            let fns: Vec<(SortFn, bool, &str)> = sorting
                .iter()
                .map(|k| (auto_sort_fn(self.rows.first(), &k.id), k.desc, k.id.as_str()))
                .collect();
            rows.sort_by(|a, b| {
                for (f, desc, id) in &fns {
                    let o = compare(*f, id, a, b, *desc);
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                Ordering::Equal
            });
        }
        let rows = Rc::new(rows);
        *self.cache.borrow_mut() = Some(VisibleCache {
            version: self.version,
            search: search.to_string(),
            sorting: sorting.to_vec(),
            status,
            rows: rows.clone(),
        });
        rows
    }
}

impl Default for ResourceStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Raw cell value used for sorting.
#[derive(Debug, Clone)]
pub enum SortValue {
    Str(String),
    Num(i64),
    Undefined,
}

pub fn sort_value(item: &ResourceItem, id: &str) -> SortValue {
    let s = |v: &Option<String>| v.clone().map(SortValue::Str).unwrap_or(SortValue::Undefined);
    let n = |v: &Option<i64>| v.map(SortValue::Num).unwrap_or(SortValue::Undefined);
    match id {
        "name" => SortValue::Str(item.name.clone()),
        "namespace" => SortValue::Str(item.namespace.clone()),
        "status" => s(&item.status),
        "ready" => s(&item.ready),
        "restarts" => n(&item.restarts),
        "age" => s(&item.age),
        "upToDate" => n(&item.up_to_date),
        "available" => n(&item.available),
        "type" => s(&item.type_),
        "clusterIp" => s(&item.cluster_ip),
        "externalIp" => s(&item.external_ip),
        "ports" => s(&item.ports),
        "roles" => s(&item.roles),
        "version" => s(&item.version),
        "lastSeen" => s(&item.last_seen),
        "eventType" => s(&item.event_type),
        "reason" => s(&item.reason),
        "involvedObject" => s(&item.involved_object),
        "message" => s(&item.message),
        "count" => n(&item.count),
        "dataKeys" => n(&item.data_keys),
        "ingressClass" => s(&item.ingress_class),
        "hosts" => s(&item.hosts),
        "secretType" => s(&item.secret_type),
        "ingressPorts" => s(&item.ingress_ports),
        "completions" => s(&item.completions),
        "duration" => s(&item.duration),
        "schedule" => s(&item.schedule),
        "lastSchedule" => s(&item.last_schedule),
        "active" => n(&item.active),
        "_context" => s(&item.context),
        _ => SortValue::Undefined,
    }
}

/// TanStack auto direction: first click is ascending for string columns,
/// descending otherwise (decided from the first row).
pub fn auto_first_desc(first: Option<&ResourceItem>, id: &str) -> bool {
    !matches!(first.map(|r| sort_value(r, id)), Some(SortValue::Str(_)))
}

#[derive(Debug, Clone, Copy)]
enum SortFn {
    Age,
    Text,
    Alphanumeric,
    Basic,
}

fn auto_sort_fn(first: Option<&ResourceItem>, id: &str) -> SortFn {
    if id == "age" || id == "lastSeen" {
        return SortFn::Age;
    }
    match first.map(|r| sort_value(r, id)) {
        Some(SortValue::Str(s)) => {
            if s.chars().any(|c| c.is_ascii_digit()) {
                SortFn::Alphanumeric
            } else {
                SortFn::Text
            }
        }
        _ => SortFn::Basic,
    }
}

fn compare(f: SortFn, id: &str, a: &ResourceItem, b: &ResourceItem, desc: bool) -> Ordering {
    let (va, vb) = (sort_value(a, id), sort_value(b, id));
    // Undefined values always sort last.
    match (&va, &vb) {
        (SortValue::Undefined, SortValue::Undefined) => return Ordering::Equal,
        (SortValue::Undefined, _) => return Ordering::Greater,
        (_, SortValue::Undefined) => return Ordering::Less,
        _ => {}
    }
    let o = match f {
        SortFn::Age => {
            let ta = str_of(&va).and_then(|s| parse_ts(&s));
            let tb = str_of(&vb).and_then(|s| parse_ts(&s));
            match (ta, tb) {
                (Some(a), Some(b)) => a.cmp(&b),
                (None, Some(_)) => return Ordering::Greater,
                (Some(_), None) => return Ordering::Less,
                (None, None) => Ordering::Equal,
            }
        }
        SortFn::Text => to_text(&va).to_lowercase().cmp(&to_text(&vb).to_lowercase()),
        SortFn::Alphanumeric => natural_cmp(&to_text(&va).to_lowercase(), &to_text(&vb).to_lowercase()),
        SortFn::Basic => match (&va, &vb) {
            (SortValue::Num(x), SortValue::Num(y)) => x.cmp(y),
            _ => to_text(&va).cmp(&to_text(&vb)),
        },
    };
    if desc {
        o.reverse()
    } else {
        o
    }
}

fn str_of(v: &SortValue) -> Option<String> {
    match v {
        SortValue::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn to_text(v: &SortValue) -> String {
    match v {
        SortValue::Str(s) => s.clone(),
        SortValue::Num(n) => n.to_string(),
        SortValue::Undefined => String::new(),
    }
}

/// Natural ("alphanumeric") comparison: digit runs compare numerically.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let d = c.is_ascii_digit();
            match out.last_mut() {
                Some((ld, buf)) if *ld == d => buf.push(c),
                _ => out.push((d, c.to_string())),
            }
        }
        out
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let o = match (x.0, y.0) {
            (true, true) => {
                let (nx, ny) = (x.1.parse::<u128>().unwrap_or(0), y.1.parse::<u128>().unwrap_or(0));
                nx.cmp(&ny)
            }
            _ => x.1.cmp(&y.1),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
}

#[cfg(test)]
mod tests {
    use super::{auto_first_desc, natural_cmp, ResourceStore, SortKey};
    use crate::state::resource_item::ResourceItem;
    use std::cmp::Ordering;

    #[test]
    fn natural() {
        assert_eq!(natural_cmp("web-2", "web-10"), Ordering::Less);
        assert_eq!(natural_cmp("a", "b"), Ordering::Less);
    }

    fn item(name: &str, restarts: Option<i64>) -> ResourceItem {
        ResourceItem { name: name.into(), restarts, ..Default::default() }
    }

    #[test]
    fn sort_and_filter() {
        let mut st = ResourceStore::new();
        st.rows = vec![item("b", Some(1)), item("a", None), item("c", Some(5))];
        st.version = 1;
        let v = st.visible("", &[SortKey { id: "name".into(), desc: false }]);
        assert_eq!(v.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        let v = st.visible("", &[SortKey { id: "restarts".into(), desc: true }]);
        assert_eq!(v.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["c", "b", "a"]);
        let v = st.visible("C", &[]);
        assert_eq!(v.len(), 1);
        assert!(auto_first_desc(st.rows.first(), "restarts"));
        assert!(!auto_first_desc(st.rows.first(), "name"));
    }

    #[test]
    fn status_quick_filter() {
        use crate::state::resource_item::StatusVariant;
        let st_item = |name: &str, status: Option<&str>| ResourceItem { name: name.into(), status: status.map(String::from), ..Default::default() };
        let mut st = ResourceStore::new();
        st.rows = vec![
            st_item("run", Some("Running")),
            st_item("done", Some("Completed")),
            st_item("crash", Some("CrashLoopBackOff")),
            st_item("err", Some("Error")),
            st_item("pend", Some("Pending")),
            st_item("init", Some("Init:0/1")),
            st_item("none", None),
        ];
        st.version = 1;
        let names = |v: std::rc::Rc<Vec<ResourceItem>>| v.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(st.visible_filtered("", &[], Some(StatusVariant::Running))), ["run", "done"]);
        assert_eq!(names(st.visible_filtered("", &[], Some(StatusVariant::Failed))), ["crash", "err"]);
        assert_eq!(names(st.visible_filtered("", &[], Some(StatusVariant::Pending))), ["pend", "init"]);
        assert_eq!(names(st.visible_filtered("cr", &[], Some(StatusVariant::Failed))), ["crash"]);
        assert_eq!(st.visible_filtered("", &[], None).len(), 7);
    }
}
