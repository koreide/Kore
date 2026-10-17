//! macOS-style native form controls (dark appearance): checkbox, radio,
//! popup button and the number-input stepper glyph.
//!
//! Colors match the system dark-appearance controls: unchecked boxes are
//! `#1e1e1e` with a `#9a9a9a` ring, checked boxes are the system accent
//! (`#3478f6`, or the CSS `accent-color`) with a white check.

use crate::automation::Automation;
use crate::keys;
use crate::state::AppState;
use crate::theme::{self, KoreStyled};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// macOS default control accent (dark appearance).
pub const SYSTEM_BLUE: Rgba = theme::hex(0x3478f6);
const UNCHECKED_BG: Rgba = theme::hex(0x1e1e1e);
const UNCHECKED_RING: Rgba = theme::hex(0x9a9a9a);

/// Native checkbox. `accent` is the checked fill (`SYSTEM_BLUE` unless the
/// page set `accent-color`).
pub fn checkbox(checked: bool, size: f32, accent: Rgba) -> Div {
    let d = div().flex_none().size(px(size)).rounded(px(size * 0.27)).flex().items_center().justify_center();
    if checked {
        d.bg(accent).child(svg().path("kore/checkmark.svg").size(px(size * 0.8)).text_color(theme::hex(0xffffff)))
    } else {
        d.bg(UNCHECKED_BG).border(px(1.5)).border_color(UNCHECKED_RING)
    }
}

/// Native radio button.
pub fn radio(checked: bool, size: f32, accent: Rgba) -> Div {
    let d = div().flex_none().size(px(size)).rounded_full().flex().items_center().justify_center();
    if checked {
        d.bg(accent).child(div().size(px(size * 0.36)).rounded_full().bg(theme::hex(0xffffff)))
    } else {
        d.bg(UNCHECKED_BG).border(px(1.5)).border_color(UNCHECKED_RING)
    }
}

/// The up/down stepper glyph of a native number input.
pub fn number_stepper(color: Rgba) -> impl IntoElement {
    svg().path("kore/stepper.svg").w(px(14.)).h(px(17.)).flex_none().text_color(color)
}

#[derive(Clone)]
pub struct SelectStyle {
    pub padding: (f32, f32),
    pub radius: f32,
    pub font_size: f32,
    pub line_height: f32,
    pub text_color: Rgba,
    pub bg: Rgba,
    pub border: Rgba,
    pub full_width: bool,
}

pub enum SelectEvent {
    Change(String),
}

impl EventEmitter<SelectEvent> for NativeSelect {}

/// `<select>`: a popup button that opens a macOS-style menu. Keyboard while
/// open: type-ahead, ↑/↓, Enter/Space selects, Esc closes.
pub struct NativeSelect {
    id: SharedString,
    pub options: Vec<(String, String)>,
    pub selected: String,
    style: SelectStyle,
    open: bool,
    highlighted: usize,
    typeahead: String,
    last_type: Instant,
    focus: FocusHandle,
    trigger_bounds: Rc<Cell<Bounds<Pixels>>>,
    app: WeakEntity<AppState>,
    /// Draw the popup-button arrows (false for CSS `appearance: none` selects).
    pub show_arrow: bool,
}

const ITEM_H: f32 = 22.;
const MENU_PAD: f32 = 5.;

impl NativeSelect {
    pub fn new(
        id: &str,
        options: Vec<(String, String)>,
        selected: String,
        style: SelectStyle,
        app: WeakEntity<AppState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        keys::register_text_entry(&focus);
        NativeSelect {
            id: id.to_string().into(),
            options,
            selected,
            style,
            open: false,
            highlighted: 0,
            typeahead: String::new(),
            last_type: Instant::now(),
            focus,
            trigger_bounds: Rc::default(),
            app,
            show_arrow: true,
        }
    }

    pub fn set_options(&mut self, options: Vec<(String, String)>, cx: &mut Context<Self>) {
        self.options = options;
        cx.notify();
    }

    pub fn set_selected(&mut self, v: String, cx: &mut Context<Self>) {
        self.selected = v;
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn selected_index(&self) -> usize {
        self.options.iter().position(|(v, _)| *v == self.selected).unwrap_or(0)
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        if let Some(app) = self.app.upgrade() {
            let id = format!("select:{}", self.id);
            cx.defer(move |cx| app.update(cx, |s, cx| s.set_modal(&id, open, cx)));
        }
        if open {
            self.highlighted = self.selected_index();
            self.typeahead.clear();
            window.focus(&self.focus, cx);
        } else if self.focus.is_focused(window) {
            window.blur(cx);
        }
        cx.notify();
    }

    fn choose(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((v, _)) = self.options.get(ix).cloned() {
            let changed = v != self.selected;
            self.selected = v.clone();
            if changed {
                cx.emit(SelectEvent::Change(v));
            }
        }
        self.set_open(false, window, cx);
    }

    fn type_ahead(&mut self, text: &str, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        if self.last_type.elapsed() > Duration::from_millis(1000) {
            self.typeahead.clear();
        }
        self.last_type = Instant::now();
        self.typeahead.push_str(&text.to_lowercase());
        if let Some(ix) = typeahead_match(&self.options, &self.typeahead) {
            self.highlighted = ix;
        }
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let k = ev.keystroke.key.as_str();
        let n = self.options.len();
        match k {
            "escape" => self.set_open(false, window, cx),
            "enter" | "space" => self.choose(self.highlighted, window, cx),
            "up" => {
                self.highlighted = self.highlighted.saturating_sub(1);
                cx.notify();
            }
            "down" => {
                if self.highlighted + 1 < n {
                    self.highlighted += 1;
                }
                cx.notify();
            }
            // Printable keys arrive through the text-input path (`type_ahead`).
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// First option whose label starts with `prefix` (case-insensitive).
pub fn typeahead_match(options: &[(String, String)], prefix: &str) -> Option<usize> {
    let p = prefix.to_lowercase();
    options.iter().position(|(_, l)| l.to_lowercase().starts_with(&p))
}

impl Render for NativeSelect {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.style.clone();
        let label = self.options.iter().find(|(v, _)| *v == self.selected).map(|(_, l)| l.clone()).unwrap_or_default();
        let tb = self.trigger_bounds.clone();
        let tb2 = self.trigger_bounds.clone();
        let id = self.id.clone();

        let trigger = div()
            .id(SharedString::from(format!("{}-trigger", self.id)))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(9.))
            .pl(px(s.padding.0))
            .pr(px(6.))
            .py(px(s.padding.1))
            .rounded(px(s.radius))
            .border_1()
            .border_color(s.border)
            .bg(s.bg)
            .text_size(px(s.font_size))
            .line_height(px(s.line_height))
            .text_color(s.text_color)
            .when(s.full_width, |d| d.w_full())
            .cursor_default()
            .child(div().flex_1().min_w_0().truncate().child(label.clone()))
            .when(self.show_arrow, |d| d.child(svg().path("kore/select-chevrons.svg").w(px(8.)).h(px(10.)).flex_none().text_color(s.text_color)))
            .on_click(cx.listener(|this, _, window, cx| {
                let open = !this.open;
                this.set_open(open, window, cx);
            }))
            .child(
                canvas(move |b, _, _| tb.set(b), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            );

        let me = cx.entity();
        let focus = self.focus.clone();
        let mut root = div()
            .relative()
            .child(
                canvas(|_, _, _| {}, move |b, _, window, cx| {
                    window.handle_input(&focus, ElementInputHandler::new(b, me.clone()), cx);
                })
                .absolute()
                .size_0(),
            )
            .when(s.full_width, |d| d.w_full())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(trigger.auto_id(format!("{id}"), "combobox", label));

        if self.open {
            let sel = self.selected_index();
            let rows: Vec<AnyElement> = self
                .options
                .iter()
                .enumerate()
                .map(|(ix, (v, l))| {
                    let hl = ix == self.highlighted;
                    div()
                        .id(("native-opt", ix))
                        .h(px(ITEM_H))
                        .mx(px(5.))
                        .pl(px(6.))
                        .pr(px(16.))
                        .rounded(px(4.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(13.))
                        .font_sans()
                        .text_color(theme::hex(0xffffff))
                        .when(hl, |d| d.bg(theme::hex(0x0a5fd1)))
                        .child(div().w(px(14.)).flex().justify_center().when(*v == self.selected, |d| {
                            d.child(svg().path("kore/checkmark.svg").size(px(11.)).text_color(theme::hex(0xffffff)))
                        }))
                        .child(l.clone())
                        .on_hover(cx.listener(move |this, h: &bool, _, cx| {
                            if *h && this.highlighted != ix {
                                this.highlighted = ix;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| this.choose(ix, window, cx)))
                        .auto_id(format!("{id}.option.{v}"), "option", l.clone())
                        .into_any_element()
                })
                .collect();
            let tw = tb2.get().size.width;
            root = root.child(
                deferred(
                    div()
                        .id(SharedString::from(format!("{}-menu", self.id)))
                        .absolute()
                        .left(px(-6.))
                        .top(px(-(MENU_PAD + sel as f32 * ITEM_H) + (s.padding.1 * 2. + s.line_height + 2. - ITEM_H) / 2.))
                        .min_w(tw + px(12.))
                        .py(px(MENU_PAD))
                        .rounded(px(6.))
                        .bg(theme::hexa(0x2a2a2c, 0.98))
                        .border_1()
                        .border_color(theme::hexa(0xffffff, 0.15))
                        .shadow(vec![BoxShadow {
                            color: theme::hexa(0, 0.4).into(),
                            offset: point(px(0.), px(8.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .occlude()
                        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.set_open(false, window, cx)))
                        .children(rows),
                )
                .with_priority(2),
            );
        }
        root
    }
}

impl EntityInputHandler for NativeSelect {
    fn text_for_range(&mut self, _: std::ops::Range<usize>, _: &mut Option<std::ops::Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        None
    }
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        None
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}
    fn replace_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.type_ahead(text, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }
    fn bounds_for_range(&mut self, _: std::ops::Range<usize>, b: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        Some(b)
    }
    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::typeahead_match;

    #[test]
    fn typeahead_matches_label_prefix() {
        let o: Vec<(String, String)> = [("", "All containers"), ("main", "main"), ("sidecar", "sidecar")]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        assert_eq!(typeahead_match(&o, "side"), Some(2));
        assert_eq!(typeahead_match(&o, "ALL"), Some(0));
        assert_eq!(typeahead_match(&o, "m"), Some(1));
        assert_eq!(typeahead_match(&o, "zz"), None);
    }
}
