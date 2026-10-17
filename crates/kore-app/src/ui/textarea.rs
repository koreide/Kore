//! Multi-line, wrapping text area (the AI Chat `<textarea>`): grows with its
//! content up to `max_height` (then scrolls to keep the caret visible).
//!
//! Shares the `KoreInput` key context and actions with [`super::input::TextInput`]:
//! Enter emits [`InputEvent::Enter`] (the owner decides to send), Shift+Enter
//! inserts a newline, Escape emits [`InputEvent::Escape`]. Registered as a
//! text-entry surface so global single-key shortcuts don't fire while typing.

use super::input::{
    Backspace, Copy, Cut, Delete, End, Enter, Escape, Home, InputEvent, Left, Paste, Right, SelectAll, SelectLeft,
    SelectRight, ShiftEnter, Up, Down, KEY_CONTEXT,
};
use crate::keys;
use crate::theme::{self, slate};
use gpui_kit::*;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub struct TextArea {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    pub text_color: Rgba,
    pub placeholder_color: Rgba,
    pub font_size: f32,
    pub line_height: f32,
    /// Max height of the text box (excluding the owner's padding).
    pub max_height: f32,
    pub disabled: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// Width used for the last layout (wrap width).
    last_width: Option<Pixels>,
    last_bounds: Option<Bounds<Pixels>>,
    lines: Vec<(usize, WrappedLine, Pixels)>, // (start offset, line, y)
    scroll_y: Pixels,
    _subs: Vec<Subscription>,
}

impl EventEmitter<InputEvent> for TextArea {}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextArea {
    pub fn new(placeholder: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        keys::register_text_entry(&focus_handle);
        let subs = vec![
            cx.on_focus(&focus_handle, window, |_, _, cx| {
                cx.emit(InputEvent::Focus);
                cx.notify();
            }),
            cx.on_blur(&focus_handle, window, |_, _, cx| {
                cx.emit(InputEvent::Blur);
                cx.notify();
            }),
        ];
        TextArea {
            focus_handle,
            content: String::new(),
            placeholder: placeholder.into(),
            text_color: slate::S100,
            placeholder_color: theme::PREFLIGHT_PLACEHOLDER,
            font_size: 14.,
            line_height: 20.,
            max_height: 120.,
            disabled: false,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_width: None,
            last_bounds: None,
            lines: Vec::new(),
            scroll_y: px(0.),
            _subs: subs,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.marked_range = None;
        cx.notify();
    }

    pub fn set_placeholder(&mut self, p: impl Into<SharedString>, cx: &mut Context<Self>) {
        let p = p.into();
        if p != self.placeholder {
            self.placeholder = p;
            cx.notify();
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus_handle, cx);
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Change(self.content.clone()));
        cx.notify();
    }

    fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, off: usize, cx: &mut Context<Self>) {
        self.selected_range = off..off;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, off: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = off
        } else {
            self.selected_range.end = off
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn prev_boundary(&self, off: usize) -> usize {
        self.content.grapheme_indices(true).rev().find_map(|(i, _)| (i < off).then_some(i)).unwrap_or(0)
    }

    fn next_boundary(&self, off: usize) -> usize {
        self.content.grapheme_indices(true).find_map(|(i, _)| (i > off).then_some(i)).unwrap_or(self.content.len())
    }

    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        let r = self.marked_range.take().unwrap_or(self.selected_range.clone());
        self.content.replace_range(r.clone(), text);
        let c = r.start + text.len();
        self.selected_range = c..c;
        self.selection_reversed = false;
        self.changed(cx);
    }

    // ── Actions ──────────────────────────────────────────────────────────

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            let p = self.prev_boundary(self.cursor());
            if p == self.cursor() {
                return;
            }
            self.selected_range = p..self.cursor();
        }
        self.insert("", cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            let n = self.next_boundary(self.cursor());
            if n == self.cursor() {
                return;
            }
            self.selected_range = self.cursor()..n;
        }
        self.insert("", cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.prev_boundary(self.cursor()), cx)
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor()), cx)
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_boundary(self.cursor()), cx)
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), cx)
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        let start = self.content[..c].rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.move_to(start, cx)
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        let end = self.content[c..].find('\n').map(|i| c + i).unwrap_or(self.content.len());
        self.move_to(end, cx)
    }

    fn vertical(&mut self, dir: f32, cx: &mut Context<Self>) {
        let lh = px(self.line_height);
        if let Some(p) = self.position_for(self.cursor()) {
            let target = point(p.x, p.y + lh * dir + lh / 2.);
            let off = self.index_for(target);
            self.move_to(off, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1., cx);
        cx.emit(InputEvent::Up);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1., cx);
        cx.emit(InputEvent::Down);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.insert(&t, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.disabled {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            self.insert("", cx);
        }
    }

    fn shift_enter(&mut self, _: &ShiftEnter, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            self.insert("\n", cx);
        }
    }

    // ── Geometry (relative to the text origin, unscrolled) ───────────────

    fn position_for(&self, off: usize) -> Option<Point<Pixels>> {
        let lh = px(self.line_height);
        let (start, line, y) = self.lines.iter().rev().find(|(s, _, _)| *s <= off)?;
        let p = line.position_for_index(off - start, lh)?;
        Some(point(p.x, p.y + *y))
    }

    fn index_for(&self, pos: Point<Pixels>) -> usize {
        let lh = px(self.line_height);
        if self.lines.is_empty() || pos.y < px(0.) {
            return 0;
        }
        let Some((start, line, y)) = self.lines.iter().rev().find(|(_, _, y)| *y <= pos.y) else { return 0 };
        let local = point(pos.x.max(px(0.)), (pos.y - *y).min(line.size(lh).height - px(1.)));
        let ix = match line.closest_index_for_position(local, lh) {
            Ok(i) | Err(i) => i,
        };
        (start + ix).min(self.content.len())
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle, cx);
        cx.stop_propagation();
        let Some(b) = self.last_bounds else { return };
        let local = point(ev.position.x - b.left(), ev.position.y - b.top() + self.scroll_y);
        let off = self.index_for(local);
        if ev.modifiers.shift {
            self.select_to(off, cx)
        } else {
            self.move_to(off, cx)
        }
    }

    fn utf16_to(&self, o: usize) -> usize {
        let (mut u8o, mut u16o) = (0, 0);
        for ch in self.content.chars() {
            if u16o >= o {
                break;
            }
            u16o += ch.len_utf16();
            u8o += ch.len_utf8();
        }
        u8o
    }

    fn to_utf16(&self, o: usize) -> usize {
        self.content[..o.min(self.content.len())].chars().map(|c| c.len_utf16()).sum()
    }

    /// Height of the text box for the current content and `width`.
    fn layout_lines(&mut self, width: Pixels, window: &mut Window) -> Pixels {
        let lh = px(self.line_height);
        let empty = self.content.is_empty();
        let (text, color): (SharedString, Rgba) =
            if empty { (self.placeholder.clone(), self.placeholder_color) } else { (self.content.clone().into(), self.text_color) };
        let run = TextRun {
            len: text.len(),
            font: font(theme::SANS),
            color: color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_text(text.clone(), px(self.font_size), &[run], Some(width), None).unwrap_or_default();
        self.lines.clear();
        let mut y = px(0.);
        let mut start = 0;
        for l in shaped {
            let h = l.size(lh).height;
            let len = l.len();
            self.lines.push((start, l, y));
            y += h;
            start += len + 1;
        }
        if empty {
            // Caret geometry for an empty box: a single empty line.
            y = lh.max(y.min(lh));
        }
        y.max(lh)
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(&mut self, r: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let range = self.utf16_to(r.start)..self.utf16_to(r.end);
        actual.replace(self.to_utf16(range.start)..self.to_utf16(range.end));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.to_utf16(self.selected_range.start)..self.to_utf16(self.selected_range.end),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.to_utf16(r.start)..self.to_utf16(r.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(&mut self, r: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(r) = r {
            self.selected_range = self.utf16_to(r.start)..self.utf16_to(r.end);
            self.marked_range = None;
        }
        self.insert(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        _sel: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = r
            .map(|r| self.utf16_to(r.start)..self.utf16_to(r.end))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content.replace_range(range.clone(), text);
        self.marked_range = (!text.is_empty()).then(|| range.start..range.start + text.len());
        let c = range.start + text.len();
        self.selected_range = c..c;
        self.changed(cx);
    }

    fn bounds_for_range(&mut self, r: Range<usize>, bounds: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let p = self.position_for(self.utf16_to(r.start))?;
        let o = point(bounds.left() + p.x, bounds.top() + p.y - self.scroll_y);
        Some(Bounds::new(o, size(px(2.), px(self.line_height))))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let b = self.last_bounds?;
        Some(self.to_utf16(self.index_for(point(p.x - b.left(), p.y - b.top() + self.scroll_y))))
    }
}

struct AreaElement {
    area: Entity<TextArea>,
}

impl IntoElement for AreaElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for AreaElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let h = self.area.update(cx, |a, _| {
            let lh = px(a.line_height);
            let width = a.last_width;
            (width, lh, px(a.max_height))
        });
        let (width, lh, max_h) = h;
        let content_h = match width {
            Some(w) => self.area.update(cx, |a, _| a.layout_lines(w, window)),
            None => lh,
        };
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = content_h.min(max_h).max(lh).into();
        style.flex_grow = 1.;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut Window, _: &mut App) {}

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let width_changed = self.area.read(cx).last_width != Some(bounds.size.width);
        let total = self.area.update(cx, |a, _| {
            a.last_width = Some(bounds.size.width);
            a.last_bounds = Some(bounds);
            a.layout_lines(bounds.size.width, window)
        });
        if width_changed {
            // Height depends on the wrap width: lay out again next frame.
            let e = self.area.clone();
            window.on_next_frame(move |_, cx| e.update(cx, |_, cx| cx.notify()));
        }
        let area = self.area.read(cx);
        let focus = area.focus_handle.clone();
        let lh = px(area.line_height);
        let empty = area.content.is_empty();
        // Keep the caret visible inside the clipped box.
        let caret = if empty { Some(point(px(0.), px(0.))) } else { area.position_for(area.cursor()) };
        let mut scroll = area.scroll_y;
        if let Some(c) = caret {
            if c.y + lh - scroll > bounds.size.height {
                scroll = c.y + lh - bounds.size.height;
            }
            if c.y < scroll {
                scroll = c.y;
            }
        }
        scroll = scroll.max(px(0.)).min((total - bounds.size.height).max(px(0.)));
        let sel = area.selected_range.clone();
        let sel_quads: Vec<PaintQuad> = if sel.is_empty() || empty {
            Vec::new()
        } else {
            let mut q = Vec::new();
            let a = area.position_for(sel.start).unwrap_or_default();
            let b = area.position_for(sel.end).unwrap_or_default();
            let mut y = a.y;
            while y <= b.y {
                let x0 = if y == a.y { a.x } else { px(0.) };
                let x1 = if y == b.y { b.x } else { bounds.size.width };
                q.push(fill(
                    Bounds::from_corners(
                        point(bounds.left() + x0, bounds.top() + y - scroll),
                        point(bounds.left() + x1, bounds.top() + y + lh - scroll),
                    ),
                    theme::hexa(0x3b82f6, 0.4),
                ));
                y += lh;
            }
            q
        };
        let text_color: Hsla = area.text_color.into();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.area.clone()), cx);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for q in sel_quads {
                window.paint_quad(q);
            }
            let lines = self.area.update(cx, |a, _| std::mem::take(&mut a.lines));
            for (_, l, y) in &lines {
                l.paint(point(bounds.left(), bounds.top() + *y - scroll), lh, TextAlign::Left, None, window, cx).ok();
            }
            self.area.update(cx, |a, _| a.lines = lines);
            if focus.is_focused(window) && !self.area.read(cx).disabled {
                if let Some(c) = caret {
                    window.paint_quad(fill(
                        Bounds::new(point(bounds.left() + c.x, bounds.top() + c.y - scroll + px(1.)), size(px(1.5), lh - px(2.))),
                        text_color,
                    ));
                }
            }
        });
        self.area.update(cx, |a, _| a.scroll_y = scroll);
    }
}

impl Render for TextArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("kore-textarea", cx.entity_id()))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(if self.disabled { CursorStyle::OperationNotAllowed } else { CursorStyle::IBeam })
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::shift_enter))
            .on_action(cx.listener(|_, _: &Enter, _, cx| cx.emit(InputEvent::Enter { shift: false })))
            .on_action(cx.listener(|_, _: &Escape, _, cx| cx.emit(InputEvent::Escape)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .w_full()
            .flex()
            .child(AreaElement { area: cx.entity() })
    }
}
