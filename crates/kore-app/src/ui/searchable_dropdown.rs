//! SearchableDropdown (spec 01 §5) with Fix 3: ↑/↓/Enter/Esc work while the
//! search input has focus.

use crate::automation::Automation;
use crate::backend;
use crate::state::AppState;
use crate::theme::{self, misc, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::scrollbar;
use crate::ui::text::tracked;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

pub enum DropdownEvent {
    Select(String),
    Open,
}

impl EventEmitter<DropdownEvent> for SearchableDropdown {}

pub struct SearchableDropdown {
    /// Automation id prefix, e.g. `sidebar.context`.
    id: SharedString,
    label: SharedString,
    placeholder: SharedString,
    all_option: Option<(SharedString, String)>,
    storage_key: String,
    pub items: Vec<String>,
    pub selected: Option<String>,
    favorites: BTreeSet<String>,
    open: bool,
    query: String,
    highlighted: usize,
    input: Entity<TextInput>,
    trigger_bounds: Rc<Cell<Bounds<Pixels>>>,
    list_scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    app: WeakEntity<AppState>,
    _subs: Vec<Subscription>,
}

impl SearchableDropdown {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        label: &str,
        placeholder: &str,
        all_option: Option<(&str, &str)>,
        storage_key: &str,
        app: WeakEntity<AppState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                "Search...",
                InputStyle {
                    text_color: slate::S100,
                    placeholder_color: theme::PREFLIGHT_PLACEHOLDER,
                    font_size: 14.,
                    line_height: 20.,
                    padding: (32., 32., 6., 6.),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change(q) => {
                this.query = q.clone();
                this.highlighted = 0;
                this.update_ghost(cx);
                cx.notify();
            }
            InputEvent::Tab => {
                if let Some(s) = this.suggestion() {
                    this.query = s.clone();
                    this.input.update(cx, |i, cx| i.set_text(s, cx));
                    this.highlighted = 0;
                    this.update_ghost(cx);
                    cx.notify();
                }
            }
            // Fix 3: keyboard navigation from inside the search input.
            InputEvent::Down => {
                let n = this.sorted().len();
                if n > 0 {
                    this.highlighted = (this.highlighted + 1).min(n - 1);
                    this.list_scroll.scroll_to_item(this.highlighted);
                    cx.notify();
                }
            }
            InputEvent::Up => {
                this.highlighted = this.highlighted.saturating_sub(1);
                this.list_scroll.scroll_to_item(this.highlighted);
                cx.notify();
            }
            InputEvent::Enter { .. } => {
                let items = this.sorted();
                if !items.is_empty() {
                    let i = this.highlighted.min(items.len() - 1);
                    this.select(items[i].clone(), window, cx);
                }
            }
            InputEvent::Escape => this.close(window, cx),
            _ => {}
        });
        let mut this = SearchableDropdown {
            id: id.to_string().into(),
            label: label.to_string().into(),
            placeholder: placeholder.to_string().into(),
            all_option: all_option.map(|(l, v)| (SharedString::from(l.to_string()), v.to_string())),
            storage_key: storage_key.to_string(),
            items: Vec::new(),
            selected: None,
            favorites: BTreeSet::new(),
            open: false,
            query: String::new(),
            highlighted: 0,
            input,
            trigger_bounds: Rc::new(Cell::new(Bounds::default())),
            list_scroll: ScrollHandle::new(),
            gutter: scrollbar::Gutter::default(),
            app,
            _subs: vec![sub],
        };
        this.load_favorites(cx);
        this
    }

    fn load_favorites(&mut self, cx: &mut Context<Self>) {
        let key = self.storage_key.clone();
        backend::run(cx, move |core| async move { core.load_favorites(key).await }, |this: &mut Self, res, cx| {
            match res {
                Ok(v) if !v.is_empty() => {
                    this.favorites = v.into_iter().collect();
                    cx.notify();
                }
                Ok(_) => {}
                Err(e) => tracing::error!("Failed to load favorites: {e}"),
            }
        })
        .detach();
    }

    fn toggle_favorite(&mut self, item: &str, cx: &mut Context<Self>) {
        if !self.favorites.remove(item) {
            self.favorites.insert(item.to_string());
        }
        let key = self.storage_key.clone();
        let values: Vec<String> = self.favorites.iter().cloned().collect();
        let core = backend::core(cx);
        cx.background_executor()
            .spawn(async move {
                if let Err(e) = core.save_favorites(key, values).await {
                    tracing::error!("Failed to save favorites: {e}");
                }
            })
            .detach();
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = open;
        let id = self.id.to_string();
        if let Some(app) = self.app.upgrade() {
            // Deferred: we may be inside an AppState update already.
            cx.defer(move |cx| app.update(cx, |s, cx| s.set_modal(&format!("dropdown:{id}"), open, cx)));
        }
        if open {
            self.highlighted = 0;
            self.list_scroll.set_offset(point(px(0.), px(0.)));
            let input = self.input.clone();
            input.update(cx, |i, cx| i.focus(window, cx));
            cx.emit(DropdownEvent::Open);
        } else {
            self.query.clear();
            self.highlighted = 0;
            self.input.update(cx, |i, cx| {
                i.set_text("", cx);
                i.set_ghost(None, cx);
            });
            if self.input.read(cx).is_focused(window) {
                window.blur(cx);
            }
        }
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.set_open(false, window, cx);
        }
    }

    fn select(&mut self, item: String, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DropdownEvent::Select(item));
        self.set_open(false, window, cx);
    }

    /// Filtered + sorted values (favorites, then prefix matches, then A→Z).
    fn sorted(&self) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        if let Some((_, v)) = &self.all_option {
            all.push(v.clone());
        }
        all.extend(self.items.iter().cloned());
        let lower = self.query.to_lowercase();
        if !lower.is_empty() {
            all.retain(|i| i.to_lowercase().contains(&lower));
        }
        all.sort_by(|a, b| {
            let (fa, fb) = (self.favorites.contains(a), self.favorites.contains(b));
            if fa != fb {
                return fb.cmp(&fa);
            }
            if !lower.is_empty() {
                let (sa, sb) = (a.to_lowercase().starts_with(&lower), b.to_lowercase().starts_with(&lower));
                if sa != sb {
                    return sb.cmp(&sa);
                }
            }
            locale_compare(a, b)
        });
        all
    }

    fn suggestion(&self) -> Option<String> {
        if self.query.is_empty() {
            return None;
        }
        let first = self.sorted().into_iter().next()?;
        let (lq, lf) = (self.query.to_lowercase(), first.to_lowercase());
        (lf.starts_with(&lq) && lf != lq).then_some(first)
    }

    fn update_ghost(&mut self, cx: &mut Context<Self>) {
        let ghost = self.suggestion().map(|s| (s[self.query.len().min(s.len())..].to_string(), slate::S600));
        self.input.update(cx, |i, cx| i.set_ghost(ghost, cx));
    }

    fn display_label(&self, item: &str) -> String {
        match &self.all_option {
            Some((label, v)) if v == item => label.to_string(),
            _ => item.to_string(),
        }
    }
}

/// `String.prototype.localeCompare` approximation (case-insensitive first).
pub fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

/// Star icon: filled (favorite) or outline.
pub fn star(filled: bool, size: f32) -> Svg {
    if filled {
        svg().path("kore/star-fill.svg").size(px(size)).flex_none()
    } else {
        icon("star", size)
    }
}

impl Render for SearchableDropdown {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let has_sel = self.selected.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
        let display: SharedString = if has_sel {
            // The all-option shows its label ("All Namespaces"), not its raw value `*`.
            self.display_label(self.selected.as_deref().unwrap_or_default()).into()
        } else {
            self.placeholder.clone()
        };
        let sel_fav = has_sel && self.favorites.contains(self.selected.as_deref().unwrap_or(""));
        let open = self.open;
        let tb = self.trigger_bounds.clone();
        let id = self.id.clone();

        let trigger = div()
            .id("dd-trigger")
            .relative()
            .w_full()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(4.))
            .border_1()
            .text_base_tw()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .cursor_pointer()
            .map(|d| {
                if has_sel {
                    d.border_color(accent).text_color(accent)
                } else {
                    d.border_color(slate::S800).hover(|s| s.border_color(slate::S700))
                }
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .flex_1()
                    .min_w_0()
                    .when(sel_fav, |d| d.child(star(true, 12.).text_color(misc::YELLOW500)))
                    .child(div().truncate().child(display.clone())),
            )
            .child(
                icon("chevron-down", 16.)
                    .text_color(if has_sel { accent } else { slate::S100 })
                    .when(open, |s| s.with_transformation(Transformation::rotate(percentage(0.5)))),
            )
            .child(
                canvas(move |b, _, _| tb.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full()
                    .top_0()
                    .left_0(),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                let next = !this.open;
                this.set_open(next, window, cx);
            }))
            .auto_id(format!("{id}.trigger"), "combobox", display)
            .selected(open);

        let panel = open.then(|| self.render_panel(window, cx));

        div()
            .relative()
            .w_full()
            .child(
                div()
                    .mb(px(8.))
                    .text_xs_tw()
                    .text_color(slate::S400)
                    .child(tracked(self.label.to_uppercase(), 12., 0.025)),
            )
            .child(trigger)
            .children(panel)
    }
}

impl SearchableDropdown {
    fn render_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let items = self.sorted();
        if self.highlighted >= items.len() && !items.is_empty() {
            self.highlighted = items.len() - 1;
        }
        let input_focused = self.input.read(cx).is_focused(window);
        let tb = self.trigger_bounds.clone();
        let id = self.id.clone();
        let q = self.query.clone();

        let search = div()
            .p(px(8.))
            .border_b_1()
            .border_color(slate::S800)
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(34.))
                    .bg(theme::hexa(0x0f172a, 0.5))
                    .border_1()
                    .border_color(if input_focused { accent } else { slate::S700 })
                    .rounded(px(4.))
                    .when(input_focused, |d| {
                        d.shadow(vec![BoxShadow {
                            color: theme::Theme::get(cx).focus_ring().into(),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(0.),
                            spread_radius: px(2.),
                            inset: false,
                        }])
                    })
                    .child(self.input.clone())
                    .child(
                        div()
                            .absolute()
                            .left(px(8.))
                            .top_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .child(icon("search", 16.).text_color(slate::S400)),
                    ),
            );

        let rows: Vec<AnyElement> = if items.is_empty() {
            vec![div().px(px(12.)).py(px(8.)).text_sm_tw().text_color(slate::S500).child("No items found").into_any_element()]
        } else {
            items
                .iter()
                .enumerate()
                .map(|(ix, item)| {
                    let fav = self.favorites.contains(item);
                    let is_sel = self.selected.as_deref() == Some(item.as_str());
                    let hl = ix == self.highlighted;
                    let label = self.display_label(item);
                    let color = if hl || is_sel { accent } else { slate::S200 };
                    let item_sel = item.clone();
                    let item_fav = item.clone();
                    div()
                        .id(("dd-row", ix))
                        .w_full()
                        .px(px(12.))
                        .py(px(8.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .text_base_tw()
                        .text_color(color)
                        .cursor_pointer()
                        .when(!hl && !is_sel, |d| d.hover(|s| s.bg(theme::hexa(0x1e293b, 0.5))))
                        .on_hover(cx.listener(move |this, h: &bool, _, cx| {
                            if *h && this.highlighted != ix {
                                this.highlighted = ix;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| this.select(item_sel.clone(), window, cx)))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .flex_1()
                                .min_w_0()
                                .when(fav, |d| d.child(star(true, 12.).text_color(misc::YELLOW500)))
                                .child(div().truncate().child(highlight_match(&label, &q, accent))),
                        )
                        .child(
                            div()
                                .id(("dd-star", ix))
                                .flex_none()
                                .p(px(4.))
                                .rounded(px(4.))
                                .hover(|s| s.bg(slate::S700))
                                .group("dd-star")
                                .child(
                                    star(fav, 12.)
                                        .text_color(if fav { misc::YELLOW500 } else { slate::S500 })
                                        .when(!fav, |s| s.group_hover("dd-star", |s| s.text_color(misc::YELLOW500))),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.toggle_favorite(&item_fav, cx);
                                }))
                                .auto_id(
                                    format!("{id}.star.{item}"),
                                    "button",
                                    if fav { "Remove from favorites" } else { "Add to favorites" },
                                ),
                        )
                        .auto_id(format!("{id}.item.{item}"), "option", label.clone())
                        .selected(is_sel)
                        .into_any_element()
                })
                .collect()
        };

        let gutter = self.gutter.overflows();
        let list = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            .child(
                div()
                    .id("dd-list")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.list_scroll)
                    .children(rows),
            )
            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(scrollbar::vertical(self.list_scroll.clone(), self.gutter.clone()));

        deferred(
            div()
                .id("dd-panel")
                .absolute()
                .left_0()
                .right_0()
                .top(px(24. + 42. + 4.))
                .max_h(px(320.))
                .flex()
                .flex_col()
                .rounded(px(6.))
                .glass()
                .occlude()
                .on_mouse_down_out(cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    if !tb.get().contains(&e.position) {
                        this.close(window, cx);
                    }
                }))
                .child(search)
                .child(list)
                .with_animation(
                    "dd-in",
                    Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()),
                    |el, t| el.opacity(t).mt(px(-4. * (1. - t))),
                )
                .auto_id(format!("{}.panel", self.id), "listbox", self.label.clone()),
        )
        .with_priority(1)
    }
}

/// Wrap the first case-insensitive match in accent + semibold.
fn highlight_match(text: &str, query: &str, accent: Rgba) -> AnyElement {
    if query.is_empty() {
        return div().child(text.to_string()).into_any_element();
    }
    let lt = text.to_lowercase();
    let lq = query.to_lowercase();
    let Some(i) = lt.find(&lq) else { return div().child(text.to_string()).into_any_element() };
    if lt.len() != text.len() {
        return div().child(text.to_string()).into_any_element();
    }
    let m_len = lq.len().min(text.len() - i);
    // StyledText with highlights keeps the inherited color for unmatched parts.
    StyledText::new(text.to_string())
        .with_highlights(vec![(
            i..i + m_len,
            HighlightStyle { color: Some(accent.into()), font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() },
        )])
        .into_any_element()
}
