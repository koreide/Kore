//! `AppState`: the single source of truth for shell/navigation state and
//! the active cluster context. Views hold an
//! `Entity<AppState>`, read it in `render`, and mutate it via the methods
//! below. Every mutation notifies observers and emits an [`AppEvent`].

use super::pins::{self, Pin};
use super::resource_item::{ResourceItem, StatusVariant};
use super::resources::{auto_first_desc, Query, ResourceStore, SortKey};
use super::settings::Settings;
use super::storage;
use super::types::{AppView, ResourceKind};
use crate::backend;
use gpui_kit::*;
use kore_core::state::helm::HelmRelease;
use kore_core::ConnectionStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

pub const FAVORITE_CONTEXTS_KEY: &str = "kore-favorite-contexts";
pub const FAVORITE_NAMESPACES_KEY: &str = "kore-favorite-namespaces";
const DEFAULT_NAMESPACES: [&str; 4] = ["default", "kube-system", "kube-public", "kube-node-lease"];
const UPDATE_CACHE_KEY: &str = "kore-update-check";

#[derive(Debug, Clone)]
pub enum AppEvent {
    ViewChanged,
    KindChanged,
    ContextChanged,
    NamespaceChanged,
    SelectionChanged,
    SettingsChanged,
    /// Esc pressed with no modal/dropdown/palette/text input focused (Fix 4).
    /// Views that navigate back on Esc subscribe to this.
    Escape,
    /// ⌘R pressed (after the resource list refresh has started).
    Refresh,
    /// Header search must take focus (`/`).
    FocusSearch,
}

/// What a confirm dialog shows and does (spec 01 §10). `on_confirm` runs on
/// "Delete"/confirm; the dialog closes itself afterwards.
pub type ConfirmAction = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct ConfirmRequest {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub variant: ConfirmVariant,
    pub on_confirm: ConfirmAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmVariant {
    Danger,
    Warning,
    Info,
}

/// AI side panel request (the resource to diagnose).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AiPanelRequest {
    pub kind: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
}

/// RBAC impersonation (spec 01 §13.2); set by the RBAC view.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Impersonation {
    /// Displayed label: `ns/name` for ServiceAccounts, else name.
    pub label: String,
    /// The identity object as the RBAC view uses it.
    pub identity: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub has_update: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub release_notes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct UpdateState {
    pub info: Option<UpdateInfo>,
    pub dismissed: bool,
    pub installing: bool,
    pub success: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct UpdateCache {
    timestamp: i64,
    info: Option<UpdateInfo>,
}

pub struct AppState {
    pub settings: Settings,
    /// `None` while booting (spinner).
    pub connection: Option<ConnectionStatus>,
    pub contexts: Vec<String>,
    pub context: Option<String>,
    pub namespaces: Vec<String>,
    /// `"*"` = all namespaces.
    pub namespace: String,
    pub view: AppView,
    pub kind: ResourceKind,
    pub selected: Option<ResourceItem>,
    pub helm_release: Option<HelmRelease>,
    pub search: String,
    pub label_filters: Vec<String>,
    pub show_label_filter: bool,
    /// Palette status quick filter (`:running` / `:failed` / `:pending`):
    /// the table only shows rows whose status badge has this variant.
    pub status_filter: Option<StatusVariant>,
    pub multi_cluster: bool,
    pub sorting: Vec<SortKey>,
    /// Highlighted table row (index into the visible, sorted rows); -1 = none.
    pub selected_row_index: i64,
    pub pins: Vec<Pin>,
    pub palette_open: bool,
    pub shortcuts_open: bool,
    pub ai_panel: Option<AiPanelRequest>,
    pub confirm: Option<ConfirmRequest>,
    /// True once the confirm action was started (until the dialog closes):
    /// Esc / backdrop clicks no longer dismiss it.
    pub confirm_busy: bool,
    /// Ids of open modal-like surfaces (dropdowns, view-owned dialogs) — Fix 4.
    pub modals: BTreeSet<String>,
    pub impersonation: Option<Impersonation>,
    pub update: UpdateState,
    pub retrying: bool,
    pub resources: Entity<ResourceStore>,
    context_seq: u64,
    ns_task: Option<Task<()>>,
    ctx_task: Option<Task<()>>,
}

impl EventEmitter<AppEvent> for AppState {}

impl AppState {
    pub fn new(settings: Settings, cx: &mut Context<Self>) -> Self {
        let resources = cx.new(|_| ResourceStore::new());
        cx.observe(&resources, |_, _, cx| cx.notify()).detach();
        let view = AppView::from_setting(&settings.default_view);
        AppState {
            settings,
            connection: None,
            contexts: Vec::new(),
            context: None,
            namespaces: DEFAULT_NAMESPACES.iter().map(|s| s.to_string()).collect(),
            namespace: "default".into(),
            view,
            kind: ResourceKind::Pods,
            selected: None,
            helm_release: None,
            search: String::new(),
            label_filters: Vec::new(),
            show_label_filter: false,
            status_filter: None,
            multi_cluster: false,
            sorting: Vec::new(),
            selected_row_index: -1,
            pins: pins::load(),
            palette_open: false,
            shortcuts_open: false,
            ai_panel: None,
            confirm: None,
            confirm_busy: false,
            modals: BTreeSet::new(),
            impersonation: None,
            update: UpdateState::default(),
            retrying: false,
            resources,
            context_seq: 0,
            ns_task: None,
            ctx_task: None,
        }
    }

    // ── Boot (spec 01 §1) ───────────────────────────────────────────────

    pub fn boot(&mut self, cx: &mut Context<Self>) {
        let core = backend::core(cx);
        cx.spawn(async move |this, cx| {
            let status = match core.get_connection_status().await {
                Ok(s) => s,
                Err(e) => ConnectionStatus {
                    connected: false,
                    error: Some(e.to_string()),
                    error_kind: Some("unknown".into()),
                    kubeconfig_path: None,
                    contexts_available: Vec::new(),
                    current_context: None,
                },
            };
            let connected = status.connected;
            let current_context = status.current_context.clone();
            this.update(cx, |this, cx| {
                if !status.contexts_available.is_empty() {
                    this.contexts = status.contexts_available.clone();
                }
                this.connection = Some(status);
                cx.notify();
            })
            .ok();
            if !connected {
                return;
            }
            let ctxs = core.list_contexts().await.unwrap_or_default();
            if ctxs.is_empty() {
                this.update(cx, |this, cx| {
                    this.contexts.clear();
                    cx.notify();
                })
                .ok();
                return;
            }
            let favs = core.load_favorites(FAVORITE_CONTEXTS_KEY.into()).await.unwrap_or_default();
            let initial = initial_context(&ctxs, &favs, current_context.as_deref());
            this.update(cx, |this, cx| {
                this.contexts = ctxs;
                this.switch_context_raw(initial, cx);
            })
            .ok();
        })
        .detach();
        self.check_updates(cx);
        self.apply_event_retention(cx);
    }

    /// Hands the Settings "Event Retention Period" to Core, which prunes now and hourly.
    fn apply_event_retention(&self, cx: &mut Context<Self>) {
        let days = self.settings.event_retention_days();
        backend::run(cx, move |core| async move { core.set_event_retention(days).await }, move |_: &mut Self, res, _| {
            if let Err(e) = res {
                tracing::warn!("applying event retention ({days}d) failed: {e}");
            }
        })
        .detach();
    }

    /// `handleContextChange`: switch backend context, then re-read status.
    fn switch_context_raw(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_seq += 1;
        let seq = self.context_seq;
        self.namespaces.clear();
        self.resources.update(cx, |r, cx| r.clear(cx));
        cx.notify();
        let core = backend::core(cx);
        self.ctx_task = Some(cx.spawn(async move |this, cx| {
            let res = core.switch_context(name.clone()).await;
            if this.update(cx, |this, _| this.context_seq != seq).unwrap_or(true) {
                return;
            }
            if let Err(e) = &res {
                tracing::error!("Failed to switch context: {e}");
            } else {
                this.update(cx, |this, cx| {
                    this.context = Some(name.clone());
                    this.on_context_changed(cx);
                })
                .ok();
            }
            if let Ok(status) = core.get_connection_status().await {
                this.update(cx, |this, cx| {
                    if this.context_seq == seq {
                        this.connection = Some(status);
                        cx.notify();
                    }
                })
                .ok();
            }
        }));
    }

    fn on_context_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppEvent::ContextChanged);
        self.load_namespaces(cx);
        self.refresh_resources(cx);
        cx.notify();
    }

    /// Namespaces for the new context + favorite auto-selection (§1 step 5).
    fn load_namespaces(&mut self, cx: &mut Context<Self>) {
        self.namespaces.clear();
        let core = backend::core(cx);
        self.ns_task = Some(cx.spawn(async move |this, cx| {
            let ns = core.list_namespaces().await;
            match ns {
                Ok(ns) if !ns.is_empty() => {
                    let favs: Vec<String> = core
                        .load_favorites(FAVORITE_NAMESPACES_KEY.into())
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|f| f == "*" || ns.contains(f))
                        .collect();
                    this.update(cx, |this, cx| {
                        let configured = this.context.as_ref().and_then(|c| this.settings.default_namespaces.get(c)).map(|s| s.as_str());
                        let next = next_namespace(&ns, &favs, configured, &this.namespace);
                        this.namespaces = ns;
                        if next != this.namespace {
                            this.set_namespace(next, cx);
                        }
                        cx.notify();
                    })
                    .ok();
                }
                other => {
                    if let Err(e) = other {
                        tracing::error!("Failed to load namespaces: {e}");
                    }
                    this.update(cx, |this, cx| {
                        this.namespaces = DEFAULT_NAMESPACES.iter().map(|s| s.to_string()).collect();
                        cx.notify();
                    })
                    .ok();
                }
            }
        }));
    }

    /// Namespace dropdown `onOpen`: re-list, replace if non-empty.
    pub fn refresh_namespaces(&mut self, cx: &mut Context<Self>) {
        let t = backend::run(cx, |core| async move { core.list_namespaces().await }, |this: &mut Self, res, cx| {
            if let Ok(ns) = res {
                if !ns.is_empty() {
                    this.namespaces = ns;
                    cx.notify();
                }
            }
        });
        t.detach();
    }

    /// ConnectionSetup "Retry"/"Connect to <ctx>".
    pub fn retry_connection(&mut self, context: Option<String>, cx: &mut Context<Self>) {
        self.retrying = true;
        cx.notify();
        let core = backend::core(cx);
        cx.spawn(async move |this, cx| {
            let status = core.retry_connection(context).await;
            this.update(cx, |this, cx| {
                this.retrying = false;
                match status {
                    Ok(status) => {
                        let connected = status.connected;
                        if connected {
                            if !status.contexts_available.is_empty() {
                                this.contexts = status.contexts_available.clone();
                            }
                            if let Some(c) = status.current_context.clone() {
                                this.context = Some(c);
                                this.connection = Some(status);
                                this.on_context_changed(cx);
                            } else {
                                this.connection = Some(status);
                            }
                        } else {
                            this.connection = Some(status);
                        }
                    }
                    Err(e) => tracing::error!("retry_connection failed: {e}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ── Navigation (spec 01 §2.2) ───────────────────────────────────────

    /// Sidebar view click / palette view: view=v, selected=None (helm clears release).
    pub fn set_view(&mut self, view: AppView, cx: &mut Context<Self>) {
        self.view = view;
        self.selected = None;
        if view == AppView::Helm {
            self.helm_release = None;
        }
        cx.emit(AppEvent::ViewChanged);
        cx.notify();
    }

    /// Sidebar resource kind click: kind, view=table, clear selection + labels.
    pub fn set_kind(&mut self, kind: ResourceKind, cx: &mut Context<Self>) {
        let changed = kind != self.kind || !self.label_filters.is_empty();
        self.kind = kind;
        self.view = AppView::Table;
        self.selected = None;
        self.label_filters.clear();
        self.show_label_filter = false;
        self.status_filter = None;
        cx.emit(AppEvent::KindChanged);
        cx.emit(AppEvent::ViewChanged);
        if changed {
            self.refresh_resources(cx);
        }
        cx.notify();
    }

    /// Palette "Go to X": kind, view=table, selected=None (labels kept).
    pub fn go_to_kind(&mut self, kind: ResourceKind, cx: &mut Context<Self>) {
        let changed = kind != self.kind;
        self.kind = kind;
        self.view = AppView::Table;
        self.selected = None;
        cx.emit(AppEvent::KindChanged);
        cx.emit(AppEvent::ViewChanged);
        if changed {
            self.refresh_resources(cx);
        }
        cx.notify();
    }

    /// Row select / pinned click / palette resource / dashboard & graph navigate:
    /// `kind` (if given) + selected=row; view=details iff the row is openable.
    pub fn open_resource(&mut self, kind: Option<ResourceKind>, item: ResourceItem, cx: &mut Context<Self>) {
        if let Some(k) = kind {
            if k != self.kind {
                self.kind = k;
                cx.emit(AppEvent::KindChanged);
                self.refresh_resources(cx);
            }
        }
        let openable = !item.name.is_empty()
            && (!item.namespace.is_empty()
                || matches!(self.kind, ResourceKind::Nodes | ResourceKind::Events | ResourceKind::Namespaces));
        self.selected = Some(item);
        if openable {
            self.view = AppView::Details;
            cx.emit(AppEvent::ViewChanged);
        }
        cx.emit(AppEvent::SelectionChanged);
        cx.notify();
    }

    /// Back (`h`, Back buttons): helm-detail → helm; else table.
    pub fn back(&mut self, cx: &mut Context<Self>) {
        if self.view == AppView::HelmDetail {
            self.view = AppView::Helm;
            self.helm_release = None;
        } else {
            self.view = AppView::Table;
            self.selected = None;
        }
        cx.emit(AppEvent::ViewChanged);
        cx.notify();
    }

    pub fn open_helm_release(&mut self, release: HelmRelease, cx: &mut Context<Self>) {
        self.helm_release = Some(release);
        self.view = AppView::HelmDetail;
        cx.emit(AppEvent::ViewChanged);
        cx.notify();
    }

    /// Context switch from sidebar dropdown or palette.
    pub fn switch_context(&mut self, name: String, cx: &mut Context<Self>) {
        self.selected = None;
        self.search.clear();
        self.view = AppView::Table;
        self.label_filters.clear();
        self.status_filter = None;
        cx.emit(AppEvent::ViewChanged);
        self.switch_context_raw(name, cx);
    }

    /// Palette status quick filter: show the table filtered to rows whose
    /// status has `variant` (None clears it; the header chip's × does that).
    pub fn set_status_filter(&mut self, variant: Option<StatusVariant>, cx: &mut Context<Self>) {
        self.status_filter = variant;
        if variant.is_some() && self.view != AppView::Table {
            self.view = AppView::Table;
            self.selected = None;
            cx.emit(AppEvent::ViewChanged);
        }
        cx.notify();
    }

    pub fn set_namespace(&mut self, ns: String, cx: &mut Context<Self>) {
        if ns == self.namespace {
            return;
        }
        self.namespace = ns;
        cx.emit(AppEvent::NamespaceChanged);
        self.refresh_resources(cx);
        cx.notify();
    }

    pub fn set_search(&mut self, s: String, cx: &mut Context<Self>) {
        if s != self.search {
            self.search = s;
            cx.notify();
        }
    }

    pub fn toggle_multi_cluster(&mut self, cx: &mut Context<Self>) {
        self.multi_cluster = !self.multi_cluster;
        self.refresh_resources(cx);
        cx.notify();
    }

    pub fn toggle_label_filter(&mut self, cx: &mut Context<Self>) {
        self.show_label_filter = !self.show_label_filter;
        cx.notify();
    }

    pub fn add_label(&mut self, label: String, cx: &mut Context<Self>) {
        let l = label.trim().to_string();
        if !l.is_empty() && !self.label_filters.contains(&l) {
            self.label_filters.push(l);
            self.refresh_resources(cx);
            cx.notify();
        }
    }

    pub fn remove_label(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx < self.label_filters.len() {
            self.label_filters.remove(idx);
            self.refresh_resources(cx);
            cx.notify();
        }
    }

    pub fn label_selector(&self) -> Option<String> {
        (!self.label_filters.is_empty()).then(|| self.label_filters.join(","))
    }

    /// `refresh()` of the resource list (also ⌘R).
    pub fn refresh_resources(&mut self, cx: &mut Context<Self>) {
        if self.context.is_none() {
            return;
        }
        let q = Query {
            kind: self.kind,
            namespace: self.namespace.clone(),
            label_selector: self.label_selector(),
            multi_cluster: self.multi_cluster,
        };
        self.resources.update(cx, |r, cx| r.refresh(q, cx));
    }

    // ── Table ───────────────────────────────────────────────────────────

    /// Visible (searched + sorted) rows of the table.
    pub fn visible_rows(&self, cx: &App) -> Rc<Vec<ResourceItem>> {
        self.resources.read(cx).visible_filtered(&self.search, &self.sorting, self.status_filter)
    }

    pub fn loading(&self, cx: &App) -> bool {
        self.resources.read(cx).loading
    }

    /// Header click: TanStack toggle (no removal; shift = multi-sort).
    pub fn toggle_sort(&mut self, id: &str, multi: bool, cx: &mut Context<Self>) {
        let first_desc = auto_first_desc(self.resources.read(cx).rows.first(), id);
        let pos = self.sorting.iter().position(|k| k.id == id);
        if multi {
            match pos {
                Some(i) => self.sorting[i].desc = !self.sorting[i].desc,
                None => self.sorting.push(SortKey { id: id.to_string(), desc: first_desc }),
            }
        } else {
            let desc = match pos {
                Some(i) => !self.sorting[i].desc,
                None => first_desc,
            };
            self.sorting = vec![SortKey { id: id.to_string(), desc }];
        }
        cx.notify();
    }

    pub fn set_selected_row_index(&mut self, i: i64, cx: &mut Context<Self>) {
        if i != self.selected_row_index {
            self.selected_row_index = i;
            cx.notify();
        }
    }

    // ── Pins (spec 01 §9) ───────────────────────────────────────────────

    pub fn is_pinned(&self, kind: &str, name: &str, namespace: &str) -> bool {
        self.pins.iter().any(|p| p.kind == kind && p.name == name && p.namespace == namespace)
    }

    pub fn toggle_pin(&mut self, kind: &str, name: &str, namespace: &str, cx: &mut Context<Self>) {
        let ns = if namespace.is_empty() { "default" } else { namespace };
        if let Some(i) =
            self.pins.iter().position(|p| p.kind == kind && p.name == name && p.namespace == ns)
        {
            self.pins.remove(i);
        } else {
            self.pins.insert(0, Pin { kind: kind.into(), name: name.into(), namespace: ns.into() });
            self.pins.truncate(50);
        }
        pins::save(&self.pins);
        cx.notify();
    }

    // ── Overlays / modals ───────────────────────────────────────────────

    pub fn toggle_palette(&mut self, cx: &mut Context<Self>) {
        self.palette_open = !self.palette_open;
        cx.notify();
    }

    pub fn set_palette_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.palette_open = open;
        cx.notify();
    }

    pub fn set_shortcuts_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.shortcuts_open = open;
        cx.notify();
    }

    pub fn open_ai_panel(&mut self, req: AiPanelRequest, cx: &mut Context<Self>) {
        self.ai_panel = Some(req);
        cx.notify();
    }

    pub fn close_ai_panel(&mut self, cx: &mut Context<Self>) {
        self.ai_panel = None;
        cx.notify();
    }

    pub fn confirm(&mut self, req: ConfirmRequest, cx: &mut Context<Self>) {
        self.confirm = Some(req);
        self.confirm_busy = false;
        cx.notify();
    }

    pub fn close_confirm(&mut self, cx: &mut Context<Self>) {
        self.confirm = None;
        self.confirm_busy = false;
        cx.notify();
    }

    /// Esc / backdrop click on the confirm dialog: cancel it, unless its
    /// action is already running. Returns true if a dialog was open.
    pub fn dismiss_confirm(&mut self, cx: &mut Context<Self>) -> bool {
        if self.confirm.is_none() {
            return false;
        }
        if !self.confirm_busy {
            self.close_confirm(cx);
        }
        true
    }

    /// Register/unregister a view-owned modal surface (Fix 4 bookkeeping).
    pub fn set_modal(&mut self, id: &str, open: bool, cx: &mut Context<Self>) {
        let changed = if open { self.modals.insert(id.to_string()) } else { self.modals.remove(id) };
        if changed {
            cx.notify();
        }
    }

    /// True while anything modal is open: palette, overlay, confirm dialog,
    /// AI panel, a dropdown or a view-registered modal.
    pub fn modal_open(&self) -> bool {
        self.palette_open || self.shortcuts_open || self.confirm.is_some() || !self.modals.is_empty()
    }

    /// Row-action Delete → ConfirmDialog → `delete_resource` → toast.
    pub fn request_delete(&mut self, kind: ResourceKind, item: ResourceItem, cx: &mut Context<Self>) {
        use super::types::KindExt;
        let ns = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let name = item.name.clone();
        let this = cx.entity().downgrade();
        self.confirm(
            ConfirmRequest {
                title: format!("Delete {}", kind.singular_label()),
                message: format!("Are you sure you want to delete \"{name}\" in namespace \"{ns}\"?"),
                confirm_label: "Delete".into(),
                cancel_label: "Cancel".into(),
                variant: ConfirmVariant::Danger,
                on_confirm: Rc::new(move |_, cx| {
                    let (ns, name) = (ns.clone(), name.clone());
                    let core = backend::core(cx);
                    let this = this.clone();
                    cx.spawn(async move |cx| {
                        let res = core.delete_resource(kind, ns, name.clone()).await;
                        cx.update(|cx| {
                            match res {
                                Ok(()) => crate::ui::toast::toast(cx, format!("{name} deleted"), crate::ui::toast::ToastKind::Success),
                                Err(e) => crate::ui::toast::toast(cx, backend::format_error(&e), crate::ui::toast::ToastKind::Error),
                            }
                            this.update(cx, |s, cx| s.close_confirm(cx)).ok();
                        });
                    })
                    .detach();
                }),
            },
            cx,
        );
    }

    pub fn set_impersonation(&mut self, imp: Option<Impersonation>, cx: &mut Context<Self>) {
        self.impersonation = imp;
        cx.notify();
    }

    // ── Settings ────────────────────────────────────────────────────────

    /// Mutate + persist settings; applies the accent app-wide.
    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let retention = self.settings.event_retention_days();
        f(&mut self.settings);
        self.settings.save();
        if self.settings.event_retention_days() != retention {
            self.apply_event_retention(cx);
        }
        crate::theme::Theme::set_accent(cx, self.settings.accent_color());
        cx.emit(AppEvent::SettingsChanged);
        cx.refresh_windows();
        cx.notify();
    }

    // ── Update check (§13.5) ────────────────────────────────────────────

    pub fn check_updates(&mut self, cx: &mut Context<Self>) {
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(c) = storage::get::<UpdateCache>(UPDATE_CACHE_KEY) {
            if now - c.timestamp < 24 * 3600 * 1000 && c.info.is_some() {
                self.update.info = c.info;
                cx.notify();
                return;
            }
        }
        self.check_updates_now(cx, |_, _| {});
    }

    /// Unconditional check (Settings "Check for Updates"); `done` gets the
    /// result, or the error message when the check failed.
    pub fn check_updates_now(
        &mut self,
        cx: &mut Context<Self>,
        done: impl FnOnce(Result<&UpdateInfo, String>, &mut Context<Self>) + 'static,
    ) {
        backend::run(cx, |core| async move { core.check_for_updates().await }, move |this: &mut Self, res, cx| {
            match res {
                Ok(i) => {
                    let info = UpdateInfo {
                        has_update: i.has_update,
                        current_version: i.current_version,
                        latest_version: i.latest_version,
                        release_url: i.release_url,
                        release_notes: i.release_notes,
                    };
                    storage::set(
                        UPDATE_CACHE_KEY,
                        &UpdateCache { timestamp: chrono::Utc::now().timestamp_millis(), info: Some(info.clone()) },
                    );
                    this.update.info = Some(info.clone());
                    done(Ok(&info), cx);
                }
                Err(e) => done(Err(e.to_string()), cx),
            }
            cx.notify();
        })
        .detach();
    }

    pub fn perform_update(&mut self, cx: &mut Context<Self>) {
        self.update.installing = true;
        cx.notify();
        backend::run(cx, |core| async move { core.perform_update().await }, |this: &mut Self, res, cx| {
            this.update.installing = false;
            match res {
                Ok(v) => {
                    this.update.success = Some(v);
                    storage::remove(UPDATE_CACHE_KEY);
                }
                Err(e) => this.update.error = Some(e.to_string()),
            }
            cx.notify();
        })
        .detach();
    }

    pub fn dismiss_update(&mut self, cx: &mut Context<Self>) {
        self.update.dismissed = true;
        self.update.error = None;
        cx.notify();
    }

    // ── Automation ──────────────────────────────────────────────────────

    pub fn snapshot_json(&self, cx: &App) -> serde_json::Value {
        let store = self.resources.read(cx);
        serde_json::json!({
            "view": self.view,
            "kind": self.kind,
            "context": self.context,
            "connected": self.connection.as_ref().map(|c| c.connected),
            "namespace": self.namespace,
            "contexts": self.contexts,
            "namespaces": self.namespaces,
            "multiCluster": self.multi_cluster,
            "search": self.search,
            "labelFilters": self.label_filters,
            "showLabelFilter": self.show_label_filter,
            "statusFilter": self.status_filter.map(status_filter_label),
            "sorting": self.sorting.iter().map(|k| serde_json::json!({"id": k.id, "desc": k.desc})).collect::<Vec<_>>(),
            "selectedRowIndex": self.selected_row_index,
            "selected": self.selected,
            "loading": store.loading,
            "rowCount": store.rows.len(),
            "visibleCount": self.visible_rows(cx).len(),
            "pins": self.pins,
            "paletteOpen": self.palette_open,
            "shortcutsOpen": self.shortcuts_open,
            "aiPanel": self.ai_panel,
            "confirmOpen": self.confirm.is_some(),
            "confirmBusy": self.confirm_busy,
            "modals": self.modals,
            "impersonation": self.impersonation,
            "update": self.update,
            "accent": self.settings.accent_color,
        })
    }

    /// One-shot delay helper for views.
    pub fn after(cx: &mut Context<Self>, d: Duration, f: impl FnOnce(&mut Self, &mut Context<Self>) + 'static) {
        let t = backend::sleep(cx, d);
        cx.spawn(async move |this, cx| {
            t.await;
            this.update(cx, f).ok();
        })
        .detach();
    }
}

/// Header chip text of a status quick filter.
pub fn status_filter_label(v: StatusVariant) -> &'static str {
    match v {
        StatusVariant::Running => "Running",
        StatusVariant::Failed => "Failed",
        StatusVariant::Pending => "Pending",
        StatusVariant::Terminating => "Terminating",
        StatusVariant::Default => "Other",
    }
}

/// Initial context at boot (spec 01 §1 step 4): the single favorite context if
/// exactly one favorite exists, else kubeconfig's `current-context`, else the
/// first context.
pub fn initial_context(ctxs: &[String], favorites: &[String], current: Option<&str>) -> String {
    let favs: Vec<&String> = favorites.iter().filter(|f| ctxs.contains(f)).collect();
    if favs.len() == 1 {
        return favs[0].clone();
    }
    if let Some(c) = current.filter(|c| ctxs.iter().any(|x| x == c)) {
        return c.to_string();
    }
    ctxs.first().cloned().unwrap_or_default()
}

/// Namespace after (re)loading a context's namespaces (spec 01 §1 step 5): the
/// Settings "Default Namespace per Context" entry (if it exists in the list or
/// is `*`), else the single favorite namespace, else the current one if still
/// valid, else `default`. `favorites` is already filtered to valid names.
pub fn next_namespace(ns: &[String], favorites: &[String], configured: Option<&str>, current: &str) -> String {
    if let Some(c) = configured.map(str::trim).filter(|c| *c == "*" || ns.iter().any(|n| n == c)) {
        return c.to_string();
    }
    if favorites.len() == 1 {
        return favorites[0].clone();
    }
    if current == "*" || ns.iter().any(|n| n == current) {
        return current.to_string();
    }
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::{initial_context, next_namespace};

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn initial_context_prefers_single_favorite_then_current() {
        let ctxs = v(&["a", "b", "c"]);
        assert_eq!(initial_context(&ctxs, &[], Some("b")), "b");
        assert_eq!(initial_context(&ctxs, &v(&["c"]), Some("b")), "c");
        assert_eq!(initial_context(&ctxs, &v(&["a", "c"]), Some("b")), "b");
        assert_eq!(initial_context(&ctxs, &v(&["gone"]), Some("b")), "b");
        assert_eq!(initial_context(&ctxs, &[], Some("missing")), "a");
        assert_eq!(initial_context(&ctxs, &[], None), "a");
    }

    #[test]
    fn next_namespace_applies_default_per_context() {
        let ns = v(&["default", "kore-demo", "kube-system"]);
        assert_eq!(next_namespace(&ns, &[], Some("kore-demo"), "default"), "kore-demo");
        // Per-context default beats the single favorite.
        assert_eq!(next_namespace(&ns, &v(&["kube-system"]), Some("kore-demo"), "default"), "kore-demo");
        // Missing namespace → ignored.
        assert_eq!(next_namespace(&ns, &[], Some("nope"), "kube-system"), "kube-system");
        assert_eq!(next_namespace(&ns, &[], Some("*"), "default"), "*");
        assert_eq!(next_namespace(&ns, &v(&["kube-system"]), None, "default"), "kube-system");
        assert_eq!(next_namespace(&ns, &[], None, "*"), "*");
        assert_eq!(next_namespace(&ns, &[], None, "gone"), "default");
    }
}
