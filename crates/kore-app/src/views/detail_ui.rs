//! Primitives shared by the detail views (spec 02 §0): the `Kbd` chip, the
//! bordered toolbar buttons, the sliding-underline tabs bar, skeletons, the
//! caps label/value grid cell and relative-time formatting.

use crate::automation::Automation;
use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::text::{caps, measure};
use crate::ui::widgets::{kbd, skeleton_bar};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;

/// `Kbd` of the detail views (§0.3): the shared chip plus `ml-1.5`.
/// Inside the `text-sm` detail buttons the `<kbd>` flex item inherits the 20px
/// line height, so it is 26px tall (it sizes the button).
pub fn kbd_chip(text: &str) -> Div {
    kbd(text.to_string()).ml(px(6.)).line_height(px(20.))
}

/// Visual variant of a bordered toolbar button (§0.4).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnStyle {
    /// Standard's colors with no hover: for buttons that set their own `.hover`.
    Plain,
    /// border slate-800, text slate-300; hover bg muted/30 (`hover:border-accent/50` dropped).
    Standard,
    /// `border-accent/50 bg-accent/NN text-accent` as rendered: gray preflight border, accent text.
    Active,
    /// border red-800/50, text red-400; hover border red-600, bg red-500/15.
    Danger,
}

/// A bordered button: `flex items-center gap-1.5 rounded-md border`.
/// Padding/size are set by the caller via `px`/`py`/text size.
pub fn button(id: impl Into<SharedString>, style: BtnStyle, cx: &App) -> Stateful<Div> {
    let id: SharedString = id.into();
    let accent = theme::accent(cx);
    let d = div()
        .id(ElementId::Name(id))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .rounded(px(6.))
        .border_1()
        .cursor_pointer()
        .flex_none();
    match style {
        BtnStyle::Standard => d
            .border_color(slate::S800)
            .text_color(slate::S300)
            .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3))),
        // Standard colors without the built-in hover, for buttons that set their own
        // (GPUI allows one `.hover` per element; a second one panics in debug builds).
        BtnStyle::Plain => d.border_color(slate::S800).text_color(slate::S300),
        BtnStyle::Active => d.border_color(theme::PREFLIGHT_BORDER).text_color(accent),
        BtnStyle::Danger => d
            .border_color(theme::hexa(0x991b1b, 0.5))
            .text_color(red::R400)
            .hover(|s| s.border_color(red::R600).bg(theme::hexa(0xef4444, 0.15))),
    }
}

/// Icon colored like its button text (SVGs do not inherit color).
pub fn btn_icon(name: &str, size: f32, color: Rgba) -> Svg {
    icon(name, size).text_color(color)
}

/// One tab of a [`tabs_bar`].
pub struct TabSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Pinging accent dot after the label (pod Shell tab with a debug container).
    pub dot: bool,
}

/// Tabs bar (§2.3): `flex border-b border-slate-800`; each tab `px-4 py-2.5
/// text-sm`, inactive slate-400 (hover slate-200), active accent with a 2px
/// accent underline that slides from the previously active tab.
pub fn tabs_bar(
    tabs: &[TabSpec],
    active: usize,
    prev: usize,
    window: &mut Window,
    cx: &App,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let accent = theme::accent(cx);
    // Tab x offsets (label width + 32px padding [+ 6px gap + 8px dot]).
    let mut xs = Vec::with_capacity(tabs.len());
    let mut x = 0.0f32;
    for t in tabs {
        let w = measure(window, t.label, theme::SANS, 14., FontWeight::NORMAL) + 32. + if t.dot { 14. } else { 0. };
        xs.push((x, w));
        x += w;
    }
    let (from_x, from_w) = xs.get(prev).copied().unwrap_or((0., 0.));
    let (to_x, to_w) = xs.get(active).copied().unwrap_or((0., 0.));
    let on_select = std::rc::Rc::new(on_select);
    let mut row = div().relative().flex().flex_row().flex_none().border_b_1().border_color(slate::S800);
    for (i, t) in tabs.iter().enumerate() {
        let on_select = on_select.clone();
        let is_active = i == active;
        let group: SharedString = format!("tab-{}", t.id).into();
        row = row.child(
            div()
                .id(ElementId::Name(group.clone()))
                .group(group.clone())
                .px(px(16.))
                .py(px(10.))
                .text_sm_tw()
                .cursor_pointer()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .text_color(if is_active { accent } else { slate::S400 })
                .when(!is_active, |d| d.hover(|s| s.text_color(slate::S200)))
                .child(t.label)
                .when(t.dot, |d| d.child(ping_dot(format!("tab-dot-{}", t.id), accent, 8.)))
                .on_click(move |_, window, cx| on_select(i, window, cx))
                .auto_id(format!("tab.{}", t.id), "tab", t.label)
                .selected(is_active),
        );
    }
    let anim_id: SharedString = format!("tab-underline-{prev}-{active}").into();
    row.child(
        div()
            .absolute()
            .bottom(px(-1.))
            .h(px(2.))
            .rounded_full()
            .bg(accent)
            .left(px(to_x))
            .w(px(to_w))
            .with_animation(
                ElementId::Name(anim_id),
                Animation::new(Duration::from_millis(if prev == active { 1 } else { 280 })).with_easing(spring_ease),
                move |el, t| {
                    let l = from_x + (to_x - from_x) * t;
                    let w = from_w + (to_w - from_w) * t;
                    el.left(px(l)).w(px(w))
                },
            ),
    )
}

/// Approximation of Framer's spring (stiffness 500, damping 35): fast ease-out
/// with a hint of overshoot.
fn spring_ease(t: f32) -> f32 {
    let w = 22.36_f32; // sqrt(500)
    let zeta = 0.78_f32;
    let wd = w * (1. - zeta * zeta).sqrt();
    let tt = t * 0.32; // seconds simulated over the animation
    let v = 1. - (-zeta * w * tt).exp() * ((wd * tt).cos() + zeta * w / wd * (wd * tt).sin());
    if t >= 1. {
        1.
    } else {
        v
    }
}

/// 8px dot with an `animate-ping` halo (Debug/Ports buttons, Shell tab).
pub fn ping_dot(id: impl Into<SharedString>, color: Rgba, size: f32) -> impl IntoElement {
    let id: SharedString = id.into();
    div().relative().size(px(size)).flex_none().child(
        div().absolute().inset_0().with_animation(
            ElementId::Name(id),
            Animation::new(Duration::from_millis(1000)).repeat(),
            move |el, t| {
                let p = (t / 0.75).min(1.0);
                let e = 1.0 - (1.0 - p).powi(3);
                let scale = 1.0 + e;
                let grow = size / 2. * (scale - 1.0);
                el.child(
                    div()
                        .absolute()
                        .top(px(-grow))
                        .left(px(-grow))
                        .size(px(size * scale))
                        .rounded_full()
                        .bg(Rgba { a: 0.75 * (1.0 - e), ..color }),
                )
                .child(div().absolute().inset_0().rounded_full().bg(color))
            },
        ),
    )
}

/// Caps label + mono value cell of the details info grids.
pub fn info_cell(label: &str, value: impl Into<SharedString>) -> impl IntoElement {
    let value: SharedString = value.into();
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .child(div().text_10().line_height(px(20.)).text_color(slate::S500).mb(px(4.)).child(caps(label, 10.)))
        .child(div().font_mono().text_xs_tw().text_color(slate::S100).overflow_hidden().child(crate::ui::mono_text::mono_text(value.clone())))
        .auto_id(format!("info.{}", label.to_lowercase()), "text", value)
}

/// Twelve shimmer bars (logs / YAML skeleton), `p-4 space-y-2.5`.
pub fn bars_skeleton(id: &str, widths: &[f32]) -> Div {
    let mut d = div().p(px(16.)).flex().flex_col().gap(px(10.));
    for (i, w) in widths.iter().enumerate() {
        d = d.child(
            div()
                .opacity(1. - i as f32 * 0.06)
                .child(skeleton_bar(ElementId::Name(format!("{id}-{i}").into()), relative(w / 100.), 12.)),
        );
    }
    d
}

/// DescribeSkeleton (§2.6). `groups`: bar widths (px) of the indented groups
/// after each 16px heading bar (`heads`).
pub fn describe_skeleton(id: &str, heads: &[f32], groups: &[&[f32]]) -> Div {
    let mut d = div().p(px(16.)).flex().flex_col().gap(px(12.));
    for (gi, (h, g)) in heads.iter().zip(groups.iter()).enumerate() {
        d = d.child(
            div()
                .when(gi > 0, |d| d.mt(px(16.)))
                .child(skeleton_bar(ElementId::Name(format!("{id}-h{gi}").into()), px(*h).into(), 16.)),
        );
        let mut grp = div().ml(px(16.)).flex().flex_col().gap(px(8.));
        for (i, w) in g.iter().enumerate() {
            grp = grp.child(skeleton_bar(ElementId::Name(format!("{id}-{gi}-{i}").into()), px(*w).into(), 12.));
        }
        d = d.child(grp);
    }
    d
}

/// `formatTimeAgo` of the events timeline / rollback: `{n}s|m|h|d ago`.
pub fn time_ago(ts: Option<&str>) -> String {
    let Some(ts) = ts.filter(|t| !t.is_empty()) else { return "-".into() };
    let Some(d) = crate::state::resource_item::parse_ts(ts) else { return "NaNd ago".into() };
    let s = (chrono::Utc::now() - d).num_milliseconds().div_euclid(1000);
    if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else if s < 86400 {
        format!("{}h ago", s / 3600)
    } else {
        format!("{}d ago", s / 86400)
    }
}

/// Opacity fade-in used for tab panes (`AnimatePresence` enter).
pub fn fade_in(id: impl Into<SharedString>, el: impl IntoElement) -> impl IntoElement {
    let id: SharedString = id.into();
    div().size_full().child(el).with_animation(
        ElementId::Name(id),
        Animation::new(Duration::from_millis(200)),
        |d, t| d.opacity(t),
    )
}

/// Detail view enter animation (§0.2): opacity 0→1, x +20→0 over ~0.3s.
pub fn enter_anim(id: impl Into<SharedString>, el: Div) -> impl IntoElement {
    let id: SharedString = id.into();
    el.relative().with_animation(
        ElementId::Name(id),
        Animation::new(Duration::from_millis(300)).with_easing(ease_out_quint),
        |d, t| d.opacity(t).left(px(20. * (1. - t))),
    )
}

fn ease_out_quint(t: f32) -> f32 {
    1. - (1. - t).powi(4)
}

/// Copy text to the clipboard (navigator.clipboard.writeText).
pub fn copy_to_clipboard(cx: &mut App, text: String) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// The latest `ResourceItem` for `namespace/name` carried by a watch event
/// (`None` for other objects, deletions and errors).
pub fn watched_item(ev: &kore_core::WatchEventPayload, namespace: &str, name: &str) -> Option<crate::state::resource_item::ResourceItem> {
    if ev.action != "applied" || ev.object["metadata"]["name"].as_str() != Some(name) {
        return None;
    }
    let item = crate::state::resource_item::to_resource_item(&ev.object)?;
    let ns_ok = item.namespace == namespace || (item.namespace.is_empty() && namespace == "default");
    (item.name == name && ns_ok).then_some(item)
}

/// Keep a details view's header data live: watch `kind` in `namespace` and
/// call `apply` with the newest item for `name` and its raw object
/// (coalesced per batch). Keep
/// the returned task in a field; dropping it stops the watch.
pub fn watch_item<V: 'static>(
    cx: &mut Context<V>,
    kind: crate::state::types::ResourceKind,
    namespace: String,
    name: String,
    apply: fn(&mut V, crate::state::resource_item::ResourceItem, &serde_json::Value, &mut Context<V>),
) -> Task<()> {
    let core = crate::backend::core(cx);
    cx.spawn(async move |this, cx| {
        let Ok(mut sub) = core.start_watch(kind, Some(namespace.clone()), None).await else { return };
        while let Some(first) = sub.recv().await {
            let mut batch = vec![first];
            batch.extend(sub.try_recv_batch(4096));
            let latest = batch.iter().rev().find_map(|ev| watched_item(ev, &namespace, &name).map(|i| (i, &ev.object)));
            if let Some((item, obj)) = latest {
                if this.update(cx, |v, cx| apply(v, item, obj, cx)).is_err() {
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{spring_ease, time_ago, watched_item};
    use kore_core::WatchEventPayload;
    use serde_json::json;

    #[test]
    fn watched_item_filters_by_object() {
        let ev = |action: &str, name: &str, phase: &str| WatchEventPayload {
            action: action.into(),
            kind: "pods".into(),
            object: json!({"metadata": {"name": name, "namespace": "ns"}, "status": {"phase": phase}}),
            context: "ctx".into(),
            namespace: Some("ns".into()),
        };
        let got = watched_item(&ev("applied", "p", "Running"), "ns", "p").unwrap();
        assert_eq!(got.status.as_deref(), Some("Running"));
        assert!(watched_item(&ev("applied", "other", "Running"), "ns", "p").is_none());
        assert!(watched_item(&ev("applied", "p", "Running"), "other-ns", "p").is_none());
        assert!(watched_item(&ev("deleted", "p", "Running"), "ns", "p").is_none());
    }

    #[test]
    fn time_ago_formats() {
        assert_eq!(time_ago(None), "-");
        let t = (chrono::Utc::now() - chrono::Duration::seconds(130)).to_rfc3339();
        assert_eq!(time_ago(Some(&t)), "2m ago");
        let t = (chrono::Utc::now() - chrono::Duration::hours(50)).to_rfc3339();
        assert_eq!(time_ago(Some(&t)), "2d ago");
    }

    #[test]
    fn spring_ends_at_one() {
        assert_eq!(spring_ease(1.0), 1.0);
        assert!(spring_ease(0.5) > 0.8);
    }
}
