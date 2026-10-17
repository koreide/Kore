//! Sidebar (spec 01 §4): branding, context/namespace dropdowns, multi-cluster
//! toggle, navigation sections, pinned resources (§9) and footer.

use crate::automation::Automation;
use crate::state::app_state::FAVORITE_CONTEXTS_KEY;
use crate::state::app_state::FAVORITE_NAMESPACES_KEY;
use crate::state::resource_item::ResourceItem;
use crate::state::types::{AppView, KindExt, ResourceKind};
use crate::state::AppState;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::scrollbar;
use crate::ui::searchable_dropdown::{DropdownEvent, SearchableDropdown};
use crate::ui::text::caps;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

pub const WIDTH: f32 = 256.;
pub const SPONSOR_URL: &str = "https://github.com/sponsors/eladbash";

/// Navigation entries in display order.
pub const VIEWS: [(AppView, &str, &str); 7] = [
    (AppView::Chat, "AI Chat", "message-circle"),
    (AppView::Dashboard, "Dashboard", "layout-dashboard"),
    (AppView::Graph, "Graph", "git-branch"),
    (AppView::NetworkPolicies, "Network Policies", "shield"),
    (AppView::Rbac, "RBAC", "shield-check"),
    (AppView::Crds, "Custom Resources", "puzzle"),
    (AppView::Helm, "Helm", "package"),
];

type KindItems = &'static [(ResourceKind, &'static str, &'static str)];

pub const KIND_SECTIONS: [(&str, KindItems); 4] = [
    (
        "Workloads",
        &[
            (ResourceKind::Pods, "Pods", "boxes"),
            (ResourceKind::Deployments, "Deployments", "server"),
            (ResourceKind::Jobs, "Jobs", "timer"),
            (ResourceKind::Cronjobs, "CronJobs", "calendar"),
        ],
    ),
    ("Network", &[(ResourceKind::Services, "Services", "activity"), (ResourceKind::Ingresses, "Ingresses", "globe")]),
    ("Config", &[(ResourceKind::Configmaps, "ConfigMaps", "file-text"), (ResourceKind::Secrets, "Secrets", "lock")]),
    (
        "Cluster",
        &[
            (ResourceKind::Nodes, "Nodes", "cpu"),
            (ResourceKind::Namespaces, "Namespaces", "folder-open"),
            (ResourceKind::Events, "Events", "zap"),
        ],
    ),
];

pub struct Sidebar {
    app: Entity<AppState>,
    context_dd: Entity<SearchableDropdown>,
    namespace_dd: Entity<SearchableDropdown>,
    nav_scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    ind: Indicator,
    _subs: Vec<Subscription>,
}

/// The shared active-item indicator (framer `layoutId="sidebar-active-indicator"`):
/// one 2px bar that slides between items with a spring (stiffness 500, damping 35).
#[derive(Clone, Default)]
struct Indicator {
    /// Active item (window y, height) from the last prepaint.
    measured: std::rc::Rc<std::cell::Cell<Option<(Pixels, Pixels)>>>,
    /// Nav content origin y from the last prepaint.
    origin: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// Animation: from (top, height) → to (top, height), keyed by `seq`.
    from: Option<(f32, f32)>,
    to: Option<(f32, f32)>,
    seq: usize,
}

/// Critically-underdamped spring step response for stiffness 500, damping 35, mass 1.
fn spring(p: f32) -> f32 {
    let t = p * 0.45;
    let w0 = 500f32.sqrt();
    let zeta = 35. / (2. * w0);
    let wd = w0 * (1. - zeta * zeta).sqrt();
    let a = zeta * w0;
    1. - (-a * t).exp() * ((wd * t).cos() + (a / wd) * (wd * t).sin())
}

impl Sidebar {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = app.downgrade();
        let context_dd = cx.new(|cx| {
            SearchableDropdown::new(
                "sidebar.context",
                "Context",
                "Select context...",
                None,
                FAVORITE_CONTEXTS_KEY,
                weak.clone(),
                window,
                cx,
            )
        });
        let namespace_dd = cx.new(|cx| {
            SearchableDropdown::new(
                "sidebar.namespace",
                "Namespaces",
                "Select namespace...",
                Some(("All Namespaces", "*")),
                FAVORITE_NAMESPACES_KEY,
                weak,
                window,
                cx,
            )
        });
        let subs = vec![
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.subscribe(&context_dd, |this, _, ev: &DropdownEvent, cx| {
                if let DropdownEvent::Select(ctx) = ev {
                    let ctx = ctx.clone();
                    this.app.update(cx, |s, cx| s.switch_context(ctx, cx));
                }
            }),
            cx.subscribe(&namespace_dd, |this, _, ev: &DropdownEvent, cx| match ev {
                DropdownEvent::Select(ns) => {
                    let ns = ns.clone();
                    this.app.update(cx, |s, cx| s.set_namespace(ns, cx));
                }
                DropdownEvent::Open => this.app.update(cx, |s, cx| s.refresh_namespaces(cx)),
            }),
        ];
        Sidebar { app, context_dd, namespace_dd, nav_scroll: ScrollHandle::new(), gutter: Default::default(), ind: Indicator::default(), _subs: subs }
    }

    pub fn close_dropdowns(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.context_dd.update(cx, |d, cx| d.close(window, cx));
        self.namespace_dd.update(cx, |d, cx| d.close(window, cx));
    }
}

fn section_heading(label: &str) -> Div {
    div().mb(px(6.)).text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S500).child(caps(label, 10.))
}

fn tagged_section_heading(label: &str) -> impl IntoElement {
    section_heading(label).auto_id(format!("sidebar.section.{}", label.to_lowercase()), "heading", label.to_uppercase())
}

/// One nav row (32px; 36px when the count pill shows). `active` draws the
/// accent text and the 2px left indicator.
fn nav_item(id: SharedString, label: &str, icon_name: &str, active: bool, count: Option<usize>, accent: Rgba, ind: &Indicator) -> Stateful<Div> {
    let measure = ind.measured.clone();
    let text = if active { accent } else { slate::S400 };
    let group: SharedString = format!("g-{id}").into();
    div()
        .id(id)
        .group(group.clone())
        .relative()
        .w_full()
        .px(px(12.))
        .py(px(6.))
        .rounded(px(6.))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .text_sm_tw()
        .text_color(text)
        .cursor_pointer()
        .when(!active, |d| d.hover(|s| s.text_color(slate::S200).bg(theme::hexa(0x1d2939, 0.3))))
        .child(icon(icon_name, 16.).text_color(text).when(!active, |s| s.group_hover(group, |s| s.text_color(slate::S200))))
        .child(div().flex_1().child(label.to_string()))
        .when(active, |d| {
            d.child(
                canvas(
                    move |b, window, _| {
                        let v = Some((b.origin.y, b.size.height));
                        if measure.replace(v) != v {
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
        })
        .when_some(count.filter(|_| active), |d, n| {
            d.child(
                div()
                    .text_10()
                    .font_weight(FontWeight::MEDIUM)
                    .px(px(6.))
                    .py(px(2.))
                    .rounded_full()
                    .text_color(accent)
                    .child(n.to_string())
                    .auto_id("sidebar.count", "text", n.to_string()),
            )
        })
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let app = self.app.read(cx);
        let view = app.view;
        let kind = app.kind;
        let multi = app.multi_cluster;
        let count = app.visible_rows(cx).len();
        let pins = app.pins.clone();
        let contexts = app.contexts.clone();
        let namespaces = app.namespaces.clone();
        let context = app.context.clone();
        let namespace = app.namespace.clone();
        self.context_dd.update(cx, |d, _| {
            d.items = contexts;
            d.selected = context;
        });
        self.namespace_dd.update(cx, |d, _| {
            d.items = namespaces;
            d.selected = Some(namespace);
        });

        let branding = div()
            .flex_none()
            .pt(px(16.))
            .px(px(16.))
            .pb(px(12.))
            .border_b_1()
            .border_color(theme::hexa(0x1e293b, 0.5))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .size(px(32.))
                    .flex_none()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm_tw()
                    .font_weight(FontWeight::BOLD)
                    .text_color(accent)
                    .child("K"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("Kore"))
                    .child(div().text_10().text_color(slate::S500).child("Kubernetes Desktop")),
            );

        let multi_toggle = div()
            .id("multi-cluster")
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(6.))
            .rounded(px(6.))
            .text_xs_tw()
            .border_1()
            .cursor_pointer()
            .map(|d| {
                if multi {
                    d.border_color(theme::PREFLIGHT_BORDER).text_color(accent)
                } else {
                    d.border_color(slate::S800).text_color(slate::S500).hover(|s| s.text_color(slate::S300).border_color(slate::S700))
                }
            })
            .group("multi-cluster")
            .child(icon("layers", 14.).map(|s| {
                if multi {
                    s.text_color(accent)
                } else {
                    s.text_color(slate::S500).group_hover("multi-cluster", |s| s.text_color(slate::S300))
                }
            }))
            .child(div().flex_1().child("Multi-Cluster"))
            .child(div().size(px(8.)).flex_none().rounded_full().bg(if multi { accent } else { slate::S700 }))
            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |m, cx| m.toggle_multi_cluster(cx))))
            .auto_id("sidebar.multi-cluster", "toggle", "Multi-Cluster")
            .selected(multi);

        let dropdowns = div()
            .flex_none()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(self.context_dd.clone())
            .child(self.namespace_dd.clone())
            .child(multi_toggle);

        // Indicator: update the animation target from last frame's measurement.
        let any_active = VIEWS.iter().any(|(v, _, _)| *v == view) || matches!(view, AppView::Table | AppView::Details);
        if !any_active {
            self.ind.measured.set(None);
        }
        if let Some((y, h)) = self.ind.measured.get() {
            let target = (f32::from(y - self.ind.origin.get()), f32::from(h));
            if self.ind.to != Some(target) {
                self.ind.from = self.ind.to.or(Some(target));
                self.ind.to = Some(target);
                self.ind.seq += 1;
            }
        }
        let origin = self.ind.origin.clone();
        let indicator = (any_active && self.ind.to.is_some()).then(|| {
            let (from, to) = (self.ind.from.unwrap_or_default(), self.ind.to.unwrap_or_default());
            div()
                .absolute()
                .left_0()
                .w(px(2.))
                .rounded_full()
                .bg(accent)
                .top(px(to.0 + 2.))
                .h(px(to.1 - 4.))
                .with_animation(("sidebar-indicator", self.ind.seq), Animation::new(std::time::Duration::from_millis(450)), move |el, p| {
                    let k = spring(p);
                    let top = from.0 + (to.0 - from.0) * k;
                    let h = from.1 + (to.1 - from.1) * k;
                    el.top(px(top + 2.)).h(px((h - 4.).max(0.)))
                })
        });
        let mut nav = div()
            .relative()
            .flex()
            .flex_col()
            .child(canvas(
                move |b, window, _| {
                    if origin.replace(b.origin.y) != b.origin.y {
                        window.refresh();
                    }
                },
                |_, _, _, _| {},
            ).absolute().top_0().left_0().size_0());

        nav = nav.child(div().mb(px(12.)).child(section_heading("Views")).child(div().flex().flex_col().gap(px(2.)).children(
            VIEWS.iter().map(|(v, label, ic)| {
                let v = *v;
                nav_item(SharedString::from(format!("nav-{label}")), label, ic, view == v, None, accent, &self.ind)
                    .on_click(cx.listener(move |this, _, _, cx| this.app.update(cx, |m, cx| m.set_view(v, cx))))
                    .auto_id(format!("sidebar.view.{}", v.as_str()), "button", *label)
                    .selected(view == v)
            }),
        )));

        if !pins.is_empty() {
            nav = nav.child(
                div()
                    .mb(px(12.))
                    .child(
                        div()
                            .mb(px(6.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .text_10()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(slate::S500)
                            .child(icon("star", 12.).text_color(slate::S500))
                            .child(caps("Pinned", 10.))
                            .auto_id("sidebar.section.pinned", "heading", "PINNED"),
                    )
                    .child(self.render_pinned(&pins, cx)),
            );
        }

        for (title, items) in KIND_SECTIONS {
            nav = nav.child(div().mb(px(12.)).child(tagged_section_heading(title)).child(div().flex().flex_col().gap(px(2.)).children(
                items.iter().map(|(k, label, ic)| {
                    let k = *k;
                    let active = kind == k && matches!(view, AppView::Table | AppView::Details);
                    nav_item(SharedString::from(format!("kind-{}", k.as_str())), label, ic, active, Some(count), accent, &self.ind)
                        .on_click(cx.listener(move |this, _, _, cx| this.app.update(cx, |m, cx| m.set_kind(k, cx))))
                        .auto_id(format!("sidebar.kind.{}", k.as_str()), "button", *label)
                        .selected(active)
                }),
            )));
        }

        let gutter = self.gutter.overflows();
        let nav_scroll = div()
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_row()
            .child(
                div()
                    .id("sidebar-nav")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.nav_scroll)
                    .px(px(16.))
                    .child(nav.children(indicator)),
            )
            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(scrollbar::vertical(self.nav_scroll.clone(), self.gutter.clone()));

        let settings_active = view == AppView::Settings;
        let footer = div()
            .flex_none()
            .p(px(16.))
            .border_t_1()
            .border_color(theme::hexa(0x1e293b, 0.5))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .text_11()
                    .text_color(slate::S500)
                    .child(icon("command", 12.).text_color(slate::S500))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .child(
                                div()
                                    .px(px(4.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(theme::hexa(0x1e293b, 0.8))
                                    .text_10()
                                    .line_height(px(12.))
                                    .font_mono()
                                    .text_color(slate::S400)
                                    .child(crate::keys::MOD_K),
                            )
                            .child(div().child("\u{00a0}Command Palette")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(4.))
                    .child(
                        div()
                            .id("sponsor")
                            .p(px(6.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .group("sponsor")
                            .hover(|s| s.bg(theme::hexa(0xec4899, 0.1)))
                            .child(icon("heart", 16.).text_color(slate::S500).group_hover("sponsor", |s| s.text_color(theme::misc::PINK400)))
                            .on_click(|_, _, _| crate::platform::open_url(SPONSOR_URL))
                            .auto_id("sidebar.sponsor", "button", "Sponsor Kore"),
                    )
                    .child(
                        div()
                            .id("settings")
                            .p(px(6.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .group("settings-btn")
                            .when(!settings_active, |d| d.hover(|s| s.bg(theme::hexa(0x1d2939, 0.3))))
                            .child(icon("settings", 16.).map(|s| {
                                if settings_active {
                                    s.text_color(accent)
                                } else {
                                    s.text_color(slate::S500).group_hover("settings-btn", |s| s.text_color(slate::S300))
                                }
                            }))
                            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |m, cx| m.set_view(AppView::Settings, cx))))
                            .auto_id("sidebar.settings", "button", "Settings")
                            .selected(settings_active),
                    ),
            );

        div()
            .w(px(WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .glass()
            .child(branding)
            .child(dropdowns)
            .child(nav_scroll)
            .child(footer)
    }
}

impl Sidebar {
    /// PinnedResources (§9): the pinned rows under the section's single
    /// "Pinned" heading, aligned with the other nav items (12px inset).
    fn render_pinned(&self, pins: &[crate::state::pins::Pin], cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().gap(px(2.)).children(pins.iter().enumerate().map(|(ix, p)| {
                let kind = ResourceKind::parse(&p.kind);
                let ic = kind.map(|k| k.icon()).unwrap_or("boxes");
                let (k_open, name_open, ns_open) = (kind, p.name.clone(), p.namespace.clone());
                let (k_un, n_un, ns_un) = (p.kind.clone(), p.name.clone(), p.namespace.clone());
                let group: SharedString = format!("pin-{ix}").into();
                div()
                    .id(("pin", ix))
                    .group(group.clone())
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let item = ResourceItem { name: name_open.clone(), namespace: ns_open.clone(), ..Default::default() };
                        this.app.update(cx, |s, cx| s.open_resource(k_open, item, cx));
                    }))
                    .child(icon(ic, 14.).text_color(slate::S500).group_hover(group.clone(), |s| s.text_color(slate::S400)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_mono()
                            .text_xs_tw()
                            .text_color(slate::S300)
                            .group_hover(group.clone(), |s| s.text_color(slate::S100))
                            .child(p.name.clone()),
                    )
                    .child(div().max_w(px(60.)).truncate().text_10().text_color(slate::S600).child(p.namespace.clone()))
                    .child(
                        div()
                            .id(("unpin", ix))
                            .p(px(2.))
                            .rounded(px(4.))
                            .opacity(0.)
                            .group_hover(group.clone(), |s| s.opacity(1.))
                            .hover(|s| s.bg(theme::hexa(0x334155, 0.5)))
                            .child(icon("x", 12.).text_color(slate::S500))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.app.update(cx, |s, cx| s.toggle_pin(&k_un, &n_un, &ns_un, cx));
                            }))
                            .auto_id(format!("sidebar.pin.unpin.{}/{}", p.namespace, p.name), "button", "Unpin"),
                    )
                    .auto_id(format!("sidebar.pin.{}/{}", p.namespace, p.name), "button", p.name.clone())
        }))
    }
}
