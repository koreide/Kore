//! Command palette (spec 01 §14, cmdk). Rendered by the root while
//! `AppState::palette_open`; the entity is created on open and dropped on close
//! (so the query resets). Root handles Esc and ⌘K; the backdrop click closes.
//!
//! Filtering/sorting reproduces cmdk 1.x: items with `command_score > 0` are
//! shown, sorted by score within their group, groups ordered by their best
//! score; with an empty query everything shows in DOM order. ↑/↓/Enter move and
//! activate the keyboard selection, which is highlighted like a hovered item.

use crate::automation::Automation;
use crate::backend;
use crate::state::app_state::AiPanelRequest;
use crate::state::resource_item::{to_resource_item, ResourceItem, StatusVariant};
use crate::state::storage;
use crate::state::types::{AppView, KindExt, ResourceKind};
use crate::state::AppState;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::command_score::command_score;
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::scrollbar;
use crate::ui::text::caps;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Backdrop `bg-black/60 backdrop-blur-sm`. GPUI has no backdrop blur, so the
/// overlay is darkened further to make the content behind it as illegible as
/// a blurred backdrop would (otherwise OCR and the eye pick up background text).
pub const BACKDROP_ALPHA: f32 = 0.9;

const RECENT_KEY: &str = "kore-recent-resources";
const SPONSOR_URL: &str = "https://github.com/sponsors/eladbash";

const QUICK_FILTERS: [(&str, &str); 6] = [
    (":pods", "Filter to pods"),
    (":deployments", "Filter to deployments"),
    (":services", "Filter to services"),
    (":running", "Show running resources"),
    (":failed", "Show failed/error resources"),
    (":pending", "Show pending resources"),
];

const RESOURCE_KINDS: [(ResourceKind, &str, &str); 11] = [
    (ResourceKind::Pods, "Pods", "boxes"),
    (ResourceKind::Deployments, "Deployments", "server"),
    (ResourceKind::Services, "Services", "activity"),
    (ResourceKind::Nodes, "Nodes", "cpu"),
    (ResourceKind::Jobs, "Jobs", "timer"),
    (ResourceKind::Cronjobs, "CronJobs", "calendar"),
    (ResourceKind::Configmaps, "ConfigMaps", "file-text"),
    (ResourceKind::Secrets, "Secrets", "lock"),
    (ResourceKind::Ingresses, "Ingresses", "globe"),
    (ResourceKind::Events, "Events", "zap"),
    (ResourceKind::Namespaces, "Namespaces", "folder-open"),
];

const VIEW_COMMANDS: [(AppView, &str, &str); 8] = [
    (AppView::Chat, "AI Chat", "message-circle"),
    (AppView::Dashboard, "Dashboard", "layout-dashboard"),
    (AppView::Graph, "Resource Graph", "git-branch"),
    (AppView::NetworkPolicies, "Network Policies", "shield"),
    (AppView::Crds, "Custom Resources", "puzzle"),
    (AppView::Helm, "Helm Releases", "package"),
    (AppView::Rbac, "RBAC Simulator", "shield-check"),
    (AppView::Settings, "Settings", "settings"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentItem {
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub timestamp: i64,
}

fn get_recent() -> Vec<RecentItem> {
    let mut v: Vec<RecentItem> = storage::get(RECENT_KEY).unwrap_or_default();
    v.truncate(20);
    v
}

fn add_recent(kind: &str, name: &str, namespace: &str) {
    let mut v: Vec<RecentItem> =
        get_recent().into_iter().filter(|r| !(r.kind == kind && r.name == name && r.namespace == namespace)).collect();
    v.insert(
        0,
        RecentItem { kind: kind.into(), name: name.into(), namespace: namespace.into(), timestamp: chrono::Utc::now().timestamp_millis() },
    );
    v.truncate(20);
    storage::set(RECENT_KEY, &v);
}

#[derive(Clone)]
enum Action {
    Resource(ResourceKind, Box<ResourceItem>, bool),
    View(AppView),
    Kind(ResourceKind),
    /// Kind quick filters go to that kind; status quick filters filter the table.
    QuickFilter(Option<ResourceKind>, Option<StatusVariant>),
    AskAi,
    Sponsor,
    Context(String),
}

#[derive(Clone)]
enum ItemBody {
    /// icon, label, optional right-aligned description, icon color
    Simple { icon: &'static str, label: String, desc: Option<&'static str>, icon_color: Rgba, mono: bool },
    Search { name: String, kind: String, namespace: String },
    Context(String),
    Recent { name: String, kind: String },
}

#[derive(Clone)]
struct Item {
    value: String,
    body: ItemBody,
    action: Action,
}

struct Group {
    heading: &'static str,
    items: Vec<Item>,
}

/// One visible entry of the list (after filtering), in display order.
enum Row {
    Heading(&'static str),
    Item(Box<Item>),
}

pub struct CommandPalette {
    app: Entity<AppState>,
    input: Entity<TextInput>,
    query: String,
    results: Vec<ResourceItem>,
    searching: bool,
    search_task: Option<Task<()>>,
    recent: Vec<RecentItem>,
    /// Keyboard selection (cmdk value).
    selected: Option<String>,
    scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    _subs: Vec<Subscription>,
}

impl CommandPalette {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                "Search resources, navigate, switch context...",
                InputStyle { padding: (16., 16., 12., 12.), ..Default::default() },
                window,
                cx,
            )
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let subs = vec![cx.subscribe(&input, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::Change(q) => this.set_query(q.clone(), cx),
            InputEvent::Up => this.move_sel(-1, cx),
            InputEvent::Down => this.move_sel(1, cx),
            InputEvent::Enter { .. } => this.activate_selected(cx),
            _ => {}
        })];
        // Read on every open (the palette is recreated each time), so the Recent
        // list always reflects the latest selections.
        let recent = get_recent();
        CommandPalette {
            app,
            input,
            query: String::new(),
            results: Vec::new(),
            searching: false,
            search_task: None,
            recent,
            selected: None,
            scroll: ScrollHandle::new(),
            gutter: scrollbar::Gutter::default(),
            _subs: subs,
        }
    }

    fn set_query(&mut self, q: String, cx: &mut Context<Self>) {
        self.query = q.clone();
        self.selected = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        // Debounced backend search (300ms, ≥2 chars).
        if q.chars().count() < 2 {
            self.results.clear();
            self.search_task = None;
            self.searching = false;
        } else {
            let ns = self.app.read(cx).namespace.clone();
            let ns = (ns != "*").then_some(ns);
            let timer = backend::sleep(cx, Duration::from_millis(300));
            let core = backend::core(cx);
            self.search_task = Some(cx.spawn(async move |this, cx| {
                timer.await;
                this.update(cx, |this, cx| {
                    this.searching = true;
                    cx.notify();
                })
                .ok();
                let res = core.search_resources(q, ns).await;
                this.update(cx, |this, cx| {
                    this.searching = false;
                    this.results = match res {
                        Ok(v) => v.iter().filter_map(to_resource_item).collect(),
                        Err(_) => Vec::new(),
                    };
                    cx.notify();
                })
                .ok();
            }));
        }
        cx.notify();
    }

    fn groups(&self, cx: &App) -> Vec<Group> {
        let mut groups = Vec::new();
        if !self.results.is_empty() {
            groups.push(Group {
                heading: "Search Results",
                items: self
                    .results
                    .iter()
                    .map(|r| {
                        let kind_s = r.kind_marker.clone().unwrap_or_else(|| "pods".into());
                        let kind = ResourceKind::parse(&kind_s).unwrap_or(ResourceKind::Pods);
                        Item {
                            value: format!("search-{kind_s}-{}", r.name),
                            body: ItemBody::Search { name: r.name.clone(), kind: kind_s, namespace: r.namespace.clone() },
                            action: Action::Resource(kind, Box::new(r.clone()), true),
                        }
                    })
                    .collect(),
            });
        }
        let simple = |icon: &'static str, label: String| ItemBody::Simple { icon, label, desc: None, icon_color: slate::S500, mono: false };
        groups.push(Group {
            heading: "Views",
            items: VIEW_COMMANDS
                .iter()
                .map(|(v, label, ic)| Item { value: format!("view-{label}"), body: simple(ic, label.to_string()), action: Action::View(*v) })
                .collect(),
        });
        groups.push(Group {
            heading: "Resources",
            items: RESOURCE_KINDS
                .iter()
                .map(|(k, label, ic)| Item { value: format!("nav-{label}"), body: simple(ic, format!("Go to {label}")), action: Action::Kind(*k) })
                .collect(),
        });
        if self.query.starts_with(':') {
            let q = self.query.to_lowercase();
            groups.push(Group {
                heading: "Quick Filters",
                items: QUICK_FILTERS
                    .iter()
                    .filter(|(p, _)| p.starts_with(&q))
                    .map(|(p, desc)| {
                        let (kind, status) = quick_filter_target(p);
                        Item {
                            value: format!("filter-{p}"),
                            body: ItemBody::Simple { icon: "filter", label: p.to_string(), desc: Some(desc), icon_color: slate::S500, mono: true },
                            action: Action::QuickFilter(kind, status),
                        }
                    })
                    .collect(),
            });
        }
        groups.push(Group {
            heading: "Actions",
            items: vec![
                Item {
                    value: "action-ask-ai".into(),
                    body: ItemBody::Simple { icon: "sparkles", label: "Ask AI".into(), desc: Some("Troubleshoot with AI"), icon_color: slate::S500, mono: false },
                    action: Action::AskAi,
                },
                Item {
                    value: "action-sponsor".into(),
                    body: ItemBody::Simple {
                        icon: "heart",
                        label: "Sponsor Kore".into(),
                        desc: Some("Support on GitHub Sponsors"),
                        icon_color: theme::misc::PINK400,
                        mono: false,
                    },
                    action: Action::Sponsor,
                },
            ],
        });
        groups.push(Group {
            heading: "Contexts",
            items: self
                .app
                .read(cx)
                .contexts
                .iter()
                .map(|c| Item { value: format!("ctx-{c}"), body: ItemBody::Context(c.clone()), action: Action::Context(c.clone()) })
                .collect(),
        });
        if !self.recent.is_empty() {
            groups.push(Group {
                heading: "Recent",
                items: self
                    .recent
                    .iter()
                    .take(8)
                    .map(|r| {
                        let kind = ResourceKind::parse(&r.kind).unwrap_or(ResourceKind::Pods);
                        Item {
                            value: format!("recent-{}-{}", r.kind, r.name),
                            body: ItemBody::Recent { name: r.name.clone(), kind: r.kind.clone() },
                            action: Action::Resource(
                                kind,
                                Box::new(ResourceItem { name: r.name.clone(), namespace: r.namespace.clone(), ..Default::default() }),
                                false,
                            ),
                        }
                    })
                    .collect(),
            });
        }
        groups
    }

    /// cmdk filtering + sorting into display rows.
    fn rows(&self, cx: &App) -> Vec<Row> {
        let groups = self.groups(cx);
        let q = self.query.trim();
        let mut scored: Vec<(f64, &'static str, Vec<Item>)> = Vec::new();
        for g in groups {
            if q.is_empty() {
                if !g.items.is_empty() {
                    scored.push((0., g.heading, g.items));
                }
                continue;
            }
            let mut items: Vec<(f64, Item)> =
                g.items.into_iter().map(|i| (command_score(&i.value, q), i)).filter(|(s, _)| *s > 0.).collect();
            if items.is_empty() {
                continue;
            }
            items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let best = items[0].0;
            scored.push((best, g.heading, items.into_iter().map(|(_, i)| i).collect()));
        }
        if !q.is_empty() {
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        let mut rows = Vec::new();
        for (_, h, items) in scored {
            rows.push(Row::Heading(h));
            rows.extend(items.into_iter().map(|i| Row::Item(Box::new(i))));
        }
        rows
    }

    fn items(&self, cx: &App) -> Vec<Item> {
        self.rows(cx).into_iter().filter_map(|r| if let Row::Item(i) = r { Some(*i) } else { None }).collect()
    }

    fn current_selection(&self, items: &[Item]) -> Option<usize> {
        if items.is_empty() {
            return None;
        }
        Some(self.selected.as_ref().and_then(|v| items.iter().position(|i| &i.value == v)).unwrap_or(0))
    }

    fn move_sel(&mut self, d: i32, cx: &mut Context<Self>) {
        let items = self.items(cx);
        let Some(cur) = self.current_selection(&items) else { return };
        let next = (cur as i32 + d).clamp(0, items.len() as i32 - 1) as usize;
        self.selected = Some(items[next].value.clone());
        // Scroll the item into view (rows include headings).
        let rows = self.rows(cx);
        if let Some(ix) = rows.iter().position(|r| matches!(r, Row::Item(i) if i.value == items[next].value)) {
            self.scroll.scroll_to_item(ix + 1);
        }
        cx.notify();
    }

    fn activate_selected(&mut self, cx: &mut Context<Self>) {
        let items = self.items(cx);
        if let Some(ix) = self.current_selection(&items) {
            self.activate(items[ix].action.clone(), cx);
        }
    }

    fn activate(&mut self, action: Action, cx: &mut Context<Self>) {
        let app = self.app.clone();
        match action {
            Action::Resource(kind, item, record) => {
                if record {
                    let ns = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
                    add_recent(kind.as_str(), &item.name, &ns);
                }
                app.update(cx, |a, cx| a.open_resource(Some(kind), *item, cx));
            }
            Action::View(v) => app.update(cx, |a, cx| a.set_view(v, cx)),
            Action::Kind(k) | Action::QuickFilter(Some(k), _) => app.update(cx, |a, cx| a.go_to_kind(k, cx)),
            Action::QuickFilter(None, status) => app.update(cx, |a, cx| a.set_status_filter(status, cx)),
            Action::AskAi => app.update(cx, |a, cx| {
                let req = match (&a.selected, a.view == AppView::Details) {
                    (Some(sel), true) => AiPanelRequest {
                        kind: Some(a.kind.as_str().to_string()),
                        namespace: Some(if sel.namespace.is_empty() { "default".into() } else { sel.namespace.clone() }),
                        name: Some(sel.name.clone()),
                    },
                    _ => AiPanelRequest::default(),
                };
                a.open_ai_panel(req, cx)
            }),
            Action::Sponsor => cx.open_url(SPONSOR_URL),
            Action::Context(c) => app.update(cx, |a, cx| a.switch_context(c, cx)),
        }
        app.update(cx, |a, cx| a.set_palette_open(false, cx));
    }

    fn render_item(&self, item: &Item, is_sel: bool, cx: &mut Context<Self>) -> AnyElement {
        let body: AnyElement = match &item.body {
            ItemBody::Simple { icon: ic, label, desc, icon_color, mono } => div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .child(icon(ic, 16.).text_color(*icon_color))
                .child(if *mono { div().font_mono().text_xs_tw().child(label.clone()) } else { div().child(label.clone()) })
                .children(desc.map(|d| div().ml_auto().text_10().text_color(slate::S500).child(d)))
                .into_any_element(),
            ItemBody::Search { name, kind, namespace } => div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .child(icon("search", 16.).text_color(slate::S500))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(div().min_w_0().truncate().font_mono().text_xs_tw().child(name.clone()))
                                .child(pill(kind)),
                        )
                        .when(!namespace.is_empty(), |d| d.child(div().text_10().text_color(slate::S500).child(namespace.clone()))),
                )
                .child(icon("arrow-right", 12.).text_color(slate::S600))
                .into_any_element(),
            ItemBody::Context(c) => div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .child(div().flex_none().size(px(6.)).rounded_full().bg(theme::emerald::E400))
                .child(div().min_w_0().truncate().font_mono().text_xs_tw().child(c.clone()))
                .into_any_element(),
            ItemBody::Recent { name, kind } => div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .child(icon("clock", 16.).text_color(slate::S500))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(div().min_w_0().truncate().font_mono().text_xs_tw().child(name.clone()))
                        .child(pill(kind)),
                )
                .into_any_element(),
        };
        let action = item.action.clone();
        let value = item.value.clone();
        let v2 = value.clone();
        div()
            .id(SharedString::from(format!("palette-item-{value}")))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(6.))
            .cursor_pointer()
            .text_sm_tw()
            .text_color(slate::S200)
            .flex()
            .flex_row()
            .items_center()
            // The keyboard selection is highlighted like a hovered item.
            .when(is_sel, |d| d.bg(theme::hexa(0x1d2939, 0.6)))
            .hover(|s| s.bg(theme::hexa(0x1d2939, 0.6)))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.selected.as_deref() != Some(v2.as_str()) {
                    this.selected = Some(v2.clone());
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.activate(action.clone(), cx)))
            .child(body)
            .auto_id(format!("palette.item.{value}"), "option", value.clone())
            .selected(is_sel)
            .into_any_element()
    }
}

/// Quick filter target: (kind to go to, status variant to filter the table by).
fn quick_filter_target(p: &str) -> (Option<ResourceKind>, Option<StatusVariant>) {
    match p {
        ":pods" => (Some(ResourceKind::Pods), None),
        ":deployments" => (Some(ResourceKind::Deployments), None),
        ":services" => (Some(ResourceKind::Services), None),
        ":running" => (None, Some(StatusVariant::Running)),
        ":failed" => (None, Some(StatusVariant::Failed)),
        ":pending" => (None, Some(StatusVariant::Pending)),
        _ => (None, None),
    }
}

fn pill(kind: &str) -> Div {
    div()
        .flex_none()
        .text_10()
        .bg(slate::S800)
        .text_color(slate::S400)
        .px(px(6.))
        .py(px(2.))
        .rounded_full()
        .child(kind.to_string())
}

impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top = window.viewport_size().height * 0.2;
        let rows = self.rows(cx);
        let any_items = rows.iter().any(|r| matches!(r, Row::Item(_)));
        let sel_value = {
            let items: Vec<&Item> = rows.iter().filter_map(|r| if let Row::Item(i) = r { Some(&**i) } else { None }).collect();
            self.selected
                .as_ref()
                .filter(|v| items.iter().any(|i| &i.value == *v))
                .cloned()
                .or_else(|| items.first().map(|i| i.value.clone()))
        };
        let children: Vec<AnyElement> = rows
            .iter()
            .map(|r| match r {
                // Group headings: small uppercase muted section labels (like the
                // sidebar's), 24px tall (8px top, 12px line, 4px bottom).
                Row::Heading(h) => div()
                    .px(px(12.))
                    .pt(px(8.))
                    .pb(px(4.))
                    .text_10()
                    .line_height(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(slate::S500)
                    .child(caps(h, 10.))
                    .auto_id(format!("palette.group.{h}"), "heading", h.to_string())
                    .into_any_element(),
                Row::Item(i) => {
                    let is_sel = sel_value.as_deref() == Some(i.value.as_str());
                    self.render_item(i, is_sel, cx)
                }
            })
            .collect();
        // Content height (8px list padding + 24px headings + item rows) decides
        // whether the custom scrollbar gutter is shown.
        let content_h: f32 = 8.
            + rows
                .iter()
                .map(|r| match r {
                    Row::Heading(_) => 24.,
                    Row::Item(i) => match &i.body {
                        // Name row is 24px tall (kind pill: 20px line + 4px padding).
                        ItemBody::Search { namespace, .. } if !namespace.is_empty() => 60.,
                        ItemBody::Search { .. } | ItemBody::Recent { .. } => 40.,
                        _ => 36.,
                    },
                })
                .sum::<f32>();
        let _ = self.gutter.overflows();
        let gutter = content_h > 400.5;
        let list = div()
            .relative()
            .flex()
            .flex_row()
            .max_h(px(400.))
            .child(
                div()
                    .id("palette-list")
                    .flex_1()
                    .min_w_0()
                    .max_h(px(400.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px(px(4.))
                    .child(div().h(px(4.)))
                    .when(!any_items, |d| {
                        d.child(
                            div()
                                .px(px(16.))
                                .py(px(24.))
                                .text_sm_tw()
                                .text_color(slate::S500)
                                .text_center()
                                .child(if self.searching { "Searching..." } else { "No results found" })
                                .auto_id("palette.empty", "status", if self.searching { "Searching..." } else { "No results found" }),
                        )
                    })
                    .children(children)
                    .child(div().h(px(4.))),
            )
            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone()));

        let accent = theme::accent(cx);
        let focused = self.input.read(cx).is_focused(window);
        // Deferred: painted after the whole window so no underlying text
        // (e.g. table cells) can end up above the backdrop.
        deferred(
        div()
            .id("palette-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme::hexa(0, BACKDROP_ALPHA))
            .flex()
            .flex_col()
            .items_center()
            .pt(top)
            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |a, cx| a.set_palette_open(false, cx))))
            .child(
                div()
                    .id("palette")
                    .w_full()
                    .max_w(px(576.))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .glass()
                    .bg(crate::theme::GLASS_OVER_DIM)
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .h(px(45.))
                            .border_b_1()
                            .border_color(theme::hexa(0x1e293b, 0.5))
                            .relative()
                            .when(focused, |d| d.child(crate::views::ai_chat::focus_ring(accent)))
                            .child(self.input.clone())
                            .auto_id("palette.input", "textbox", "Search resources, navigate, switch context..."),
                    )
                    .child(list)
                    .children(sel_value.clone().map(|v| div().auto_id("palette.selected", "status", v))),
            )
            .auto_id("palette", "dialog", "Command Palette"),
        )
        .with_priority(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{quick_filter_target, AppView, StatusVariant, QUICK_FILTERS, VIEW_COMMANDS};

    #[test]
    fn quick_filters_have_targets() {
        for (p, _) in QUICK_FILTERS {
            let (k, s) = quick_filter_target(p);
            assert!(k.is_some() != s.is_some(), "{p} must target exactly one of kind/status");
        }
        assert_eq!(quick_filter_target(":failed"), (None, Some(StatusVariant::Failed)));
        assert!(VIEW_COMMANDS.iter().any(|(v, l, _)| *v == AppView::NetworkPolicies && *l == "Network Policies"));
    }
}
