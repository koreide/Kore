//! Single-line text input styled per call site (GPUI has no native `<input>`).
//!
//! The entity draws only the text, caret, selection and placeholder; the owner
//! wraps it in a styled box (border, background, radius) and sets the
//! inner padding through [`InputStyle`] so the whole box is clickable.
//!
//! Keys: printable characters go through the platform IME handler; editing
//! keys are actions bound in the `KoreInput` key context. Enter/Escape/Up/
//! Down/Tab and Backspace-on-empty are surfaced as [`InputEvent`]s so owners
//! can implement submit, close, list navigation and autocomplete.

use crate::keys;
use crate::theme::{self, slate};
use gpui_kit::*;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

actions!(
    kore_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        Paste,
        Cut,
        Copy,
        Enter,
        ShiftEnter,
        Escape,
        Up,
        Down,
        Tab,
    ]
);

pub const KEY_CONTEXT: &str = "KoreInput";

pub fn init_keymap(cx: &mut App) {
    let c = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("cmd-left", Home, c),
        KeyBinding::new("cmd-right", End, c),
        KeyBinding::new("shift-home", SelectHome, c),
        KeyBinding::new("shift-end", SelectEnd, c),
        KeyBinding::new("cmd-shift-left", SelectHome, c),
        KeyBinding::new("cmd-shift-right", SelectEnd, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("shift-enter", ShiftEnter, c),
        KeyBinding::new("escape", Escape, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("tab", Tab, c),
    ]);
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    Change(String),
    Enter { shift: bool },
    Escape,
    Up,
    Down,
    Tab,
    /// Backspace pressed while the input was already empty.
    BackspaceEmpty,
    Focus,
    Blur,
}

#[derive(Debug, Clone)]
pub struct InputStyle {
    pub text_color: Rgba,
    pub placeholder_color: Rgba,
    pub font_family: SharedString,
    pub font_size: f32,
    pub line_height: f32,
    pub weight: FontWeight,
    /// Inner padding: left, right, top, bottom.
    pub padding: (f32, f32, f32, f32),
    /// Render bullets instead of the text (password inputs).
    pub masked: bool,
}

impl Default for InputStyle {
    fn default() -> Self {
        InputStyle {
            text_color: slate::S100,
            placeholder_color: theme::PREFLIGHT_PLACEHOLDER,
            font_family: theme::SANS.into(),
            font_size: 14.,
            line_height: 20.,
            weight: FontWeight::NORMAL,
            padding: (0., 0., 0., 0.),
            masked: false,
        }
    }
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    pub style: InputStyle,
    /// Inline completion suffix drawn after the text (SearchableDropdown ghost).
    ghost: Option<(String, Rgba)>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    is_selecting: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<InputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, style: InputStyle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        keys::register_text_entry(&focus_handle);
        let subs = vec![
            cx.on_focus(&focus_handle, window, |_, _, cx| {
                cx.emit(InputEvent::Focus);
                cx.notify();
            }),
            cx.on_blur(&focus_handle, window, |this, _, cx| {
                this.is_selecting = false;
                cx.emit(InputEvent::Blur);
                cx.notify();
            }),
        ];
        TextInput {
            focus_handle,
            content: String::new(),
            placeholder: placeholder.into(),
            style,
            ghost: None,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            is_selecting: false,
            _subs: subs,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replace the text (no `Change` event), cursor at end.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.marked_range = None;
        cx.notify();
    }

    pub fn set_placeholder(&mut self, p: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = p.into();
        cx.notify();
    }

    pub fn set_ghost(&mut self, ghost: Option<(String, Rgba)>, cx: &mut Context<Self>) {
        if self.ghost != ghost {
            self.ghost = ghost;
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

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            cx.emit(InputEvent::BackspaceEmpty);
            return;
        }
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                return;
            }
            self.select_to(prev, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                return;
            }
            self.select_to(next, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.style.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            if !self.style.masked {
                cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            }
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        cx.stop_propagation();
        self.is_selecting = true;
        if event.click_count >= 2 {
            self.move_to(0, cx);
            self.select_to(self.content.len(), cx);
            return;
        }
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        let idx = line.closest_index_for_x(position.x - bounds.left() + self.scroll_x);
        self.display_to_content(idx)
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    /// Text as displayed (bullets when masked).
    fn display_text(&self) -> String {
        if self.style.masked {
            "•".repeat(self.content.chars().count())
        } else {
            self.content.clone()
        }
    }

    fn content_to_display(&self, offset: usize) -> usize {
        if self.style.masked {
            self.content[..offset].chars().count() * '•'.len_utf8()
        } else {
            offset
        }
    }

    fn display_to_content(&self, offset: usize) -> usize {
        if self.style.masked {
            let n = offset / '•'.len_utf8();
            self.content.char_indices().nth(n).map(|(i, _)| i).unwrap_or(self.content.len())
        } else {
            offset
        }
    }

    fn font(&self) -> Font {
        Font { weight: self.style.weight, ..font(self.style.font_family.clone()) }
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.range_to_utf16(&self.selected_range), reversed: self.selection_reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content = self.content[0..range.start].to_owned() + new_text + &self.content[range.end..];
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        self.changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content = self.content[0..range.start].to_owned() + new_text + &self.content[range.end..];
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let (a, b) = (self.content_to_display(range.start), self.content_to_display(range.end));
        Some(Bounds::from_corners(
            point(bounds.left() + last_layout.x_for_index(a) - self.scroll_x, bounds.top()),
            point(bounds.left() + last_layout.x_for_index(b) - self.scroll_x, bounds.bottom()),
        ))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let bounds = self.last_bounds?;
        let last_layout = self.last_layout.as_ref()?;
        let idx = last_layout.index_for_x(point.x - bounds.left() + self.scroll_x)?;
        Some(self.offset_to_utf16(self.display_to_content(idx)))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    ghost: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let lh = self.input.read(cx).style.line_height;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = px(lh).into();
        style.flex_grow = 1.;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let input = self.input.read(cx);
        let content_empty = input.content.is_empty();
        let display = input.display_text();
        let selected_range = input.content_to_display(input.selected_range.start)..input.content_to_display(input.selected_range.end);
        let cursor = input.content_to_display(input.cursor_offset());
        let font = input.font();
        let font_size = px(input.style.font_size);
        let (text, color): (SharedString, Rgba) = if content_empty {
            (input.placeholder.clone(), input.style.placeholder_color)
        } else {
            (display.into(), input.style.text_color)
        };
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
            color: color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match input.marked_range.as_ref() {
            Some(m) if !content_empty => vec![
                TextRun { len: m.start, ..run.clone() },
                TextRun {
                    len: m.end - m.start,
                    underline: Some(UnderlineStyle { color: Some(run.color), thickness: px(1.0), wavy: false }),
                    ..run.clone()
                },
                TextRun { len: text.len() - m.end, ..run },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            _ => vec![run],
        };
        let line = window.text_system().shape_line(text, font_size, &runs, None);
        let ghost = input.ghost.as_ref().filter(|(g, _)| !g.is_empty() && !content_empty).map(|(g, c)| {
            let g: SharedString = g.clone().into();
            window.text_system().shape_line(
                g.clone(),
                font_size,
                &[TextRun { len: g.len(), font: font.clone(), color: (*c).into(), background_color: None, underline: None, strikethrough: None }],
                None,
            )
        });

        let cursor_x = if content_empty { px(0.) } else { line.x_for_index(cursor) };
        // Keep the caret visible.
        let width = bounds.size.width;
        let mut scroll_x = input.scroll_x;
        if cursor_x - scroll_x > width - px(2.) {
            scroll_x = cursor_x - width + px(2.);
        }
        if cursor_x < scroll_x {
            scroll_x = cursor_x;
        }
        if line.width <= width {
            scroll_x = px(0.);
        }

        let text_color: Hsla = input.style.text_color.into();
        let (selection, cursor) = if selected_range.is_empty() || content_empty {
            (
                None,
                Some(fill(
                    Bounds::new(point(bounds.left() + cursor_x - scroll_x, bounds.top() + px(1.)), size(px(1.5), bounds.size.height - px(2.))),
                    text_color,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(bounds.left() + line.x_for_index(selected_range.start) - scroll_x, bounds.top()),
                        point(bounds.left() + line.x_for_index(selected_range.end) - scroll_x, bounds.bottom()),
                    ),
                    theme::hexa(0x3b82f6, 0.4),
                )),
                None,
            )
        };
        PrepaintState { line: Some(line), ghost, cursor, selection, scroll_x }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        let lh = px(self.input.read(cx).style.line_height);
        let content_empty = self.input.read(cx).content.is_empty();
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.input.clone()), cx);
        let scroll_x = prepaint.scroll_x;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection)
            }
            let Some(line) = prepaint.line.take() else { return };
            let origin = point(bounds.origin.x - scroll_x, bounds.origin.y);
            line.paint(origin, lh, TextAlign::Left, None, window, cx).ok();
            if let Some(ghost) = prepaint.ghost.take() {
                ghost.paint(point(origin.x + line.width, origin.y), lh, TextAlign::Left, None, window, cx).ok();
            }
            if focus_handle.is_focused(window) {
                if let Some(cursor) = prepaint.cursor.take() {
                    window.paint_quad(cursor);
                }
            }
            self.input.update(cx, |input, _| {
                input.last_layout = if content_empty { None } else { Some(line) };
                input.last_bounds = Some(bounds);
                input.scroll_x = scroll_x;
            });
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (l, r, t, b) = self.style.padding;
        div()
            .id(("kore-input", cx.entity_id()))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(|_, _: &Enter, _, cx| cx.emit(InputEvent::Enter { shift: false })))
            .on_action(cx.listener(|_, _: &ShiftEnter, _, cx| cx.emit(InputEvent::Enter { shift: true })))
            .on_action(cx.listener(|_, _: &Escape, _, cx| cx.emit(InputEvent::Escape)))
            .on_action(cx.listener(|_, _: &Up, _, cx| cx.emit(InputEvent::Up)))
            .on_action(cx.listener(|_, _: &Down, _, cx| cx.emit(InputEvent::Down)))
            .on_action(cx.listener(|_, _: &Tab, _, cx| cx.emit(InputEvent::Tab)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .size_full()
            .flex()
            .flex_row()
            .items_center()
            .pl(px(l))
            .pr(px(r))
            .pt(px(t))
            .pb(px(b))
            .overflow_hidden()
            .child(TextElement { input: cx.entity() })
    }
}
