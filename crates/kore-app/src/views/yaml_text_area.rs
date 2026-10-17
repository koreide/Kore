//! Multi-line plain-text editor used by the YAML tab (spec 02 §5): a text
//! area plus its line-number gutter and search-highlight layer, drawn
//! by one element so the gutter can never drift (Fix 15).
//!
//! 12px mono, 19.5px lines, 16px padding, no wrapping (horizontal scroll), no
//! syntax highlighting. Glyphs are laid out on the 0.601em SF Mono advance
//! (`theme::MONO_ADVANCE_EM`).

use crate::keys;
use crate::theme::{self, slate};
use gpui_kit::*;
use std::ops::Range;
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

pub const FONT_SIZE: f32 = 12.;
/// `text-xs leading-relaxed` (12px × 1.625 = 19.5px) lines are laid out 19px
/// apart.
pub const LINE_H: f32 = 19.;
pub const PAD: f32 = 16.;

pub fn advance() -> f32 {
    FONT_SIZE * theme::MONO_ADVANCE_EM
}

/// Gutter width for `lines` lines: `px-2` number cells, `pr-3`, 1px border.
pub fn gutter_width(lines: usize) -> f32 {
    let digits = lines.max(1).to_string().len() as f32;
    digits * advance() + 16. + 12. + 1.
}

actions!(
    kore_textarea,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DocStart,
        DocEnd,
        SelectDocStart,
        SelectDocEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        PageUp,
        PageDown,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Newline,
        InsertTab,
        Undo,
        Redo,
    ]
);

pub const KEY_CONTEXT: &str = "KoreTextArea";

fn ensure_keymap(cx: &mut App) {
    thread_local! { static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    let c = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("cmd-left", LineStart, c),
        KeyBinding::new("cmd-right", LineEnd, c),
        KeyBinding::new("shift-home", SelectLineStart, c),
        KeyBinding::new("shift-end", SelectLineEnd, c),
        KeyBinding::new("cmd-shift-left", SelectLineStart, c),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, c),
        KeyBinding::new("cmd-up", DocStart, c),
        KeyBinding::new("cmd-down", DocEnd, c),
        KeyBinding::new("cmd-shift-up", SelectDocStart, c),
        KeyBinding::new("cmd-shift-down", SelectDocEnd, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("enter", Newline, c),
        KeyBinding::new("tab", InsertTab, c),
        KeyBinding::new("cmd-z", Undo, c),
        KeyBinding::new("ctrl-z", Undo, c),
        KeyBinding::new("cmd-shift-z", Redo, c),
        KeyBinding::new("ctrl-shift-z", Redo, c),
    ]);
}

pub enum TextAreaEvent {
    Changed,
}

/// Search highlight layer state.
#[derive(Default, Clone)]
pub struct SearchMarks {
    pub starts: Vec<usize>,
    pub len: usize,
    pub current: usize,
}

pub struct YamlTextArea {
    focus: FocusHandle,
    text: String,
    line_starts: Vec<usize>,
    max_cols: usize,
    read_only: bool,
    sel: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    /// Positive scroll offsets of the text (and gutter, vertically).
    scroll: Point<Pixels>,
    goal_col: Option<usize>,
    selecting: bool,
    marks: SearchMarks,
    pending_line: Option<usize>,
    autoscroll: bool,
    viewport: Size<Pixels>,
    text_origin: Point<Pixels>,
    undo: Vec<(String, Range<usize>)>,
    redo: Vec<(String, Range<usize>)>,
    last_edit: Option<Instant>,
}

impl EventEmitter<TextAreaEvent> for YamlTextArea {}

impl Focusable for YamlTextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Byte offsets where each line starts.
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut v = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

impl YamlTextArea {
    pub fn new(cx: &mut Context<Self>) -> Self {
        ensure_keymap(cx);
        let focus = cx.focus_handle();
        keys::register_text_entry(&focus);
        YamlTextArea {
            focus,
            text: String::new(),
            line_starts: vec![0],
            max_cols: 0,
            read_only: true,
            sel: 0..0,
            reversed: false,
            marked: None,
            scroll: point(px(0.), px(0.)),
            goal_col: None,
            selecting: false,
            marks: SearchMarks::default(),
            pending_line: None,
            autoscroll: false,
            viewport: size(px(0.), px(0.)),
            text_origin: point(px(0.), px(0.)),
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Replace the whole value. Keeps the caret where it was when possible.
    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        if text == self.text {
            return;
        }
        self.text = text;
        self.reindex();
        let n = self.text.len();
        let clamp = |i: usize, t: &str| {
            let mut i = i.min(n);
            while !t.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        self.sel = clamp(self.sel.start, &self.text)..clamp(self.sel.end, &self.text);
        self.marked = None;
        self.undo.clear();
        self.redo.clear();
        cx.notify();
    }

    pub fn set_read_only(&mut self, ro: bool, cx: &mut Context<Self>) {
        self.read_only = ro;
        cx.notify();
    }

    pub fn set_marks(&mut self, marks: SearchMarks, cx: &mut Context<Self>) {
        self.marks = marks;
        cx.notify();
    }

    /// `scrollToPosition`: put the line of byte `pos` ~1/3 from the top.
    pub fn scroll_to_pos(&mut self, pos: usize, cx: &mut Context<Self>) {
        self.pending_line = Some(self.line_of(pos.min(self.text.len())));
        cx.notify();
    }

    fn reindex(&mut self) {
        self.line_starts = line_starts(&self.text);
        self.max_cols = (0..self.line_starts.len()).map(|l| self.line_text(l).chars().count()).max().unwrap_or(0);
    }

    fn line_range(&self, l: usize) -> Range<usize> {
        let s = self.line_starts[l];
        let e = if l + 1 < self.line_starts.len() { self.line_starts[l + 1] - 1 } else { self.text.len() };
        s..e
    }

    fn line_text(&self, l: usize) -> &str {
        &self.text[self.line_range(l)]
    }

    fn line_of(&self, ix: usize) -> usize {
        match self.line_starts.binary_search(&ix) {
            Ok(l) => l,
            Err(l) => l - 1,
        }
    }

    fn col_of(&self, ix: usize) -> usize {
        let l = self.line_of(ix);
        self.text[self.line_starts[l]..ix].chars().count()
    }

    fn ix_for(&self, line: usize, col: usize) -> usize {
        let r = self.line_range(line);
        let t = &self.text[r.clone()];
        t.char_indices().nth(col).map(|(i, _)| r.start + i).unwrap_or(r.end)
    }

    fn cursor(&self) -> usize {
        if self.reversed {
            self.sel.start
        } else {
            self.sel.end
        }
    }

    fn move_to(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.sel = ix..ix;
        self.reversed = false;
        self.autoscroll = true;
        cx.notify();
    }

    fn select_to(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.sel.start = ix
        } else {
            self.sel.end = ix
        }
        if self.sel.end < self.sel.start {
            self.reversed = !self.reversed;
            self.sel = self.sel.end..self.sel.start;
        }
        self.autoscroll = true;
        cx.notify();
    }

    fn prev_boundary(&self, ix: usize) -> usize {
        self.text.grapheme_indices(true).rev().find_map(|(i, _)| (i < ix).then_some(i)).unwrap_or(0)
    }

    fn next_boundary(&self, ix: usize) -> usize {
        self.text.grapheme_indices(true).find_map(|(i, _)| (i > ix).then_some(i)).unwrap_or(self.text.len())
    }

    fn word_left(&self, ix: usize) -> usize {
        let b = self.text[..ix].trim_end_matches(|c: char| !c.is_alphanumeric());
        let w = b.trim_end_matches(|c: char| c.is_alphanumeric());
        w.len()
    }

    fn word_right(&self, ix: usize) -> usize {
        let rest = &self.text[ix..];
        let skip = rest.len() - rest.trim_start_matches(|c: char| !c.is_alphanumeric()).len();
        let r2 = &rest[skip..];
        let word = r2.len() - r2.trim_start_matches(|c: char| c.is_alphanumeric()).len();
        ix + skip + word
    }

    fn vertical(&mut self, delta: isize, select: bool, cx: &mut Context<Self>) {
        let cur = self.cursor();
        let line = self.line_of(cur) as isize;
        let goal = self.goal_col.unwrap_or_else(|| self.col_of(cur));
        let target = line + delta;
        let ix = if target < 0 {
            0
        } else if target as usize >= self.line_count() {
            self.text.len()
        } else {
            self.ix_for(target as usize, goal)
        };
        if select {
            self.select_to(ix, cx);
        } else {
            self.move_to(ix, cx);
        }
        self.goal_col = Some(goal);
    }

    fn snapshot(&mut self) {
        let now = Instant::now();
        let coalesce = self.last_edit.map(|t| now.duration_since(t) < Duration::from_millis(400)).unwrap_or(false);
        if !coalesce {
            self.undo.push((self.text.clone(), self.sel.clone()));
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = Some(now);
    }

    /// Replace `range` with `new` (no-op when read-only).
    fn edit(&mut self, range: Range<usize>, new: &str, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        self.snapshot();
        self.text.replace_range(range.clone(), new);
        let c = range.start + new.len();
        self.sel = c..c;
        self.reversed = false;
        self.marked = None;
        self.goal_col = None;
        self.autoscroll = true;
        self.reindex();
        cx.emit(TextAreaEvent::Changed);
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let p = self.prev_boundary(self.cursor());
            if p == self.cursor() {
                return;
            }
            self.edit(p..self.cursor(), "", cx);
        } else {
            self.edit(self.sel.clone(), "", cx);
        }
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.sel.is_empty() {
            let n = self.next_boundary(self.cursor());
            if n == self.cursor() {
                return;
            }
            self.edit(self.cursor()..n, "", cx);
        } else {
            self.edit(self.sel.clone(), "", cx);
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_col = None;
        if self.sel.is_empty() {
            self.move_to(self.prev_boundary(self.cursor()), cx)
        } else {
            self.move_to(self.sel.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_col = None;
        if self.sel.is_empty() {
            self.move_to(self.next_boundary(self.cursor()), cx)
        } else {
            self.move_to(self.sel.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_col = None;
        self.select_to(self.prev_boundary(self.cursor()), cx)
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_col = None;
        self.select_to(self.next_boundary(self.cursor()), cx)
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, false, cx)
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, false, cx)
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, true, cx)
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, true, cx)
    }

    fn page(&self) -> isize {
        ((self.viewport.height / px(LINE_H)) as isize - 1).max(1)
    }

    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.page();
        self.vertical(-p, false, cx)
    }

    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.page();
        self.vertical(p, false, cx)
    }

    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_col = None;
        let l = self.line_of(self.cursor());
        self.move_to(self.line_starts[l], cx)
    }

    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_col = None;
        let l = self.line_of(self.cursor());
        self.move_to(self.line_range(l).end, cx)
    }

    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let l = self.line_of(self.cursor());
        self.select_to(self.line_starts[l], cx)
    }

    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let l = self.line_of(self.cursor());
        self.select_to(self.line_range(l).end, cx)
    }

    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx)
    }

    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.text.len(), cx)
    }

    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx)
    }

    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.text.len(), cx)
    }

    fn word_left_a(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.word_left(self.cursor()), cx)
    }

    fn word_right_a(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.word_right(self.cursor()), cx)
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.word_left(self.cursor()), cx)
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.word_right(self.cursor()), cx)
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.sel = 0..self.text.len();
        self.reversed = false;
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.text[self.sel.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.is_empty() && !self.read_only {
            cx.write_to_clipboard(ClipboardItem::new_string(self.text[self.sel.clone()].to_string()));
            self.edit(self.sel.clone(), "", cx);
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
            let t = t.replace("\r\n", "\n");
            self.edit(self.sel.clone(), &t, cx);
        }
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        self.last_edit = None;
        self.edit(self.sel.clone(), "\n", cx);
    }

    /// Tab inserts two spaces at the selection start (replacing the selection).
    fn tab(&mut self, _: &InsertTab, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(self.sel.clone(), "  ", cx);
    }

    fn undo_a(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some((t, s)) = self.undo.pop() {
            self.redo.push((std::mem::replace(&mut self.text, t), self.sel.clone()));
            self.sel = s;
            self.reindex();
            self.last_edit = None;
            cx.emit(TextAreaEvent::Changed);
            cx.notify();
        }
    }

    fn redo_a(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some((t, s)) = self.redo.pop() {
            self.undo.push((std::mem::replace(&mut self.text, t), self.sel.clone()));
            self.sel = s;
            self.reindex();
            cx.emit(TextAreaEvent::Changed);
            cx.notify();
        }
    }

    fn index_for_point(&self, p: Point<Pixels>) -> usize {
        let y = p.y - self.text_origin.y - px(PAD) + self.scroll.y;
        let line = (y / px(LINE_H)).floor().max(0.) as usize;
        if line >= self.line_count() {
            return self.text.len();
        }
        let x = p.x - self.text_origin.x - px(PAD) + self.scroll.x;
        let col = (x / px(advance())).round().max(0.) as usize;
        self.ix_for(line, col)
    }

    fn mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let ix = self.index_for_point(ev.position);
        self.goal_col = None;
        if ev.click_count == 2 {
            let s = self.word_left(self.next_boundary(ix).min(self.text.len()));
            let e = self.word_right(s);
            self.sel = s..e;
            self.reversed = false;
            cx.notify();
            return;
        }
        if ev.click_count >= 3 {
            let l = self.line_of(ix);
            self.sel = self.line_range(l);
            cx.notify();
            return;
        }
        self.selecting = true;
        if ev.modifiers.shift {
            self.select_to(ix, cx)
        } else {
            self.move_to(ix, cx)
        }
    }

    fn mouse_drag(&mut self, p: Point<Pixels>, cx: &mut Context<Self>) {
        if self.selecting {
            let ix = self.index_for_point(p);
            self.select_to(ix, cx);
        }
    }

    fn content_size(&self) -> Size<Pixels> {
        size(
            px(self.max_cols as f32 * advance() + 2. * PAD),
            px(self.line_count() as f32 * LINE_H + 2. * PAD),
        )
    }

    fn clamp_scroll(&mut self) {
        let c = self.content_size();
        let max_x = (c.width - self.viewport.width).max(px(0.));
        let max_y = (c.height - self.viewport.height).max(px(0.));
        self.scroll.x = self.scroll.x.clamp(px(0.), max_x);
        self.scroll.y = self.scroll.y.clamp(px(0.), max_y);
    }

    fn scroll_by(&mut self, d: Point<Pixels>, cx: &mut Context<Self>) {
        self.scroll.x -= d.x;
        self.scroll.y -= d.y;
        self.clamp_scroll();
        cx.notify();
    }

    fn utf16_to_ix(&self, u: usize) -> usize {
        let mut n = 0;
        for (i, ch) in self.text.char_indices() {
            if n >= u {
                return i;
            }
            n += ch.len_utf16();
        }
        self.text.len()
    }

    fn ix_to_utf16(&self, ix: usize) -> usize {
        self.text[..ix.min(self.text.len())].chars().map(char::len_utf16).sum()
    }
}

impl EntityInputHandler for YamlTextArea {
    fn text_for_range(
        &mut self,
        r: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.utf16_to_ix(r.start)..self.utf16_to_ix(r.end);
        actual.replace(self.ix_to_utf16(range.start)..self.ix_to_utf16(range.end));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.ix_to_utf16(self.sel.start)..self.ix_to_utf16(self.sel.end),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.ix_to_utf16(r.start)..self.ix_to_utf16(r.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, r: Option<Range<usize>>, new: &str, _: &mut Window, cx: &mut Context<Self>) {
        let range = r
            .map(|r| self.utf16_to_ix(r.start)..self.utf16_to_ix(r.end))
            .or(self.marked.clone())
            .unwrap_or(self.sel.clone());
        self.edit(range, new, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        new: &str,
        new_sel: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = r
            .map(|r| self.utf16_to_ix(r.start)..self.utf16_to_ix(r.end))
            .or(self.marked.clone())
            .unwrap_or(self.sel.clone());
        self.edit(range.clone(), new, cx);
        self.marked = (!new.is_empty()).then(|| range.start..range.start + new.len());
        if let Some(ns) = new_sel {
            let base = range.start;
            let s = (base + ns.start).min(self.text.len());
            let e = (base + ns.end).min(self.text.len());
            self.sel = s..e;
        }
    }

    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        _bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let ix = self.utf16_to_ix(r.start);
        let l = self.line_of(ix);
        let x = self.text_origin.x + px(PAD + self.col_of(ix) as f32 * advance()) - self.scroll.x;
        let y = self.text_origin.y + px(PAD + l as f32 * LINE_H) - self.scroll.y;
        Some(Bounds::new(point(x, y), size(px(advance()), px(LINE_H))))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.ix_to_utf16(self.index_for_point(p)))
    }
}

impl Render for YamlTextArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("yaml-textarea")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::word_left_a))
            .on_action(cx.listener(Self::word_right_a))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::undo_a))
            .on_action(cx.listener(Self::redo_a))
            .child(TextAreaElement { area: cx.entity() })
    }
}

/// Paint one line with every glyph at `char_index * pitch` (the 0.601em SF
/// Mono pitch; see `ui::mono_text`), vertically centred in a `line_h` row.
fn paint_mono(window: &mut Window, text: &str, run: TextRun, size: Pixels, pitch: Pixels, origin: Point<Pixels>, line_h: Pixels) {
    if text.is_empty() {
        return;
    }
    let color = run.color;
    let line = window.text_system().layout_line(text, size, &[run], None);
    let baseline = origin.y + (line_h - line.ascent - line.descent) / 2. + line.ascent;
    let mut char_ix = vec![0usize; text.len() + 1];
    for (ci, (bi, _)) in text.char_indices().enumerate() {
        char_ix[bi] = ci;
    }
    for r in &line.runs {
        for g in &r.glyphs {
            let ci = char_ix.get(g.index).copied().unwrap_or(0);
            let o = point(origin.x + pitch * ci as f32, baseline);
            if g.is_emoji {
                window.paint_emoji(o, r.font_id, g.id, size).ok();
            } else {
                window.paint_glyph(o, r.font_id, g.id, size, color).ok();
            }
        }
    }
}

struct TextAreaElement {
    area: Entity<YamlTextArea>,
}

impl IntoElement for TextAreaElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct Prepaint {
    hitbox: Hitbox,
    text_hitbox: Hitbox,
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

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
        let style = Style { size: size(relative(1.).into(), relative(1.).into()), ..Default::default() };
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
    ) -> Prepaint {
        self.area.update(cx, |a, _| {
            let gw = px(gutter_width(a.line_count()));
            a.text_origin = point(bounds.left() + gw, bounds.top());
            a.viewport = size(bounds.size.width - gw, bounds.size.height);
            if let Some(line) = a.pending_line.take() {
                let c = a.content_size();
                let per_line = c.height / a.line_count().max(1) as f32;
                a.scroll.y = (per_line * line as f32 - a.viewport.height / 3.).max(px(0.));
            }
            if a.autoscroll {
                a.autoscroll = false;
                let cur = a.cursor();
                let l = a.line_of(cur) as f32;
                let top = px(PAD + l * LINE_H);
                let bottom = top + px(LINE_H);
                if top < a.scroll.y + px(PAD) {
                    a.scroll.y = (top - px(PAD)).max(px(0.));
                }
                if bottom > a.scroll.y + a.viewport.height - px(PAD) {
                    a.scroll.y = bottom - a.viewport.height + px(PAD);
                }
                let x = px(PAD + a.col_of(cur) as f32 * advance());
                if x < a.scroll.x + px(PAD) {
                    a.scroll.x = (x - px(PAD)).max(px(0.));
                }
                if x > a.scroll.x + a.viewport.width - px(PAD) {
                    a.scroll.x = x - a.viewport.width + px(PAD);
                }
            }
            a.clamp_scroll();
        });
        let text_bounds = Bounds::from_corners(self.area.read(cx).text_origin, bounds.bottom_right());
        Prepaint {
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
            text_hitbox: window.insert_hitbox(text_bounds, HitboxBehavior::Normal),
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        pre: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let accent = theme::accent(cx);
        let area_entity = self.area.clone();
        let focus = area_entity.update(cx, |a, _cx| {
        let a = &*a;
        let focus = a.focus.clone();
        let focused = focus.is_focused(window);
        let ro = a.read_only;
        let lines = a.line_count();
        let gw = px(gutter_width(lines));
        let origin = a.text_origin;
        let scroll = a.scroll;
        let viewport = a.viewport;
        let adv = px(advance());
        let lh = px(LINE_H);
        let fsize = px(FONT_SIZE);
        let first = ((scroll.y - px(PAD)) / lh).floor().max(0.) as usize;
        let last = (((scroll.y + viewport.height - px(PAD)) / lh).ceil().max(0.) as usize).min(lines.saturating_sub(1));
        let mono = font(SharedString::from(theme::MONO));
        let text_color: Hsla = if ro { slate::S400.into() } else { slate::S300.into() };
        let run = |len: usize, color: Hsla| TextRun {
            len,
            font: mono.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line_y = |l: usize| origin.y + px(PAD) + lh * l as f32 - scroll.y;
        let col_x = |c: usize| origin.x + px(PAD) + adv * c as f32 - scroll.x;

        // Gutter: numbers right-aligned before `px-2` + `pr-3`, right border.
        let gutter = Bounds::new(bounds.origin, size(gw, bounds.size.height));
        window.paint_quad(fill(
            Bounds::new(point(bounds.left() + gw - px(1.), bounds.top()), size(px(1.), bounds.size.height)),
            theme::hexa(0x1e293b, 0.5),
        ));
        window.with_content_mask(Some(ContentMask { bounds: gutter }), |window| {
            if lines == 0 {
                return;
            }
            for l in first..=last {
                let s: SharedString = (l + 1).to_string().into();
                let w = adv * s.chars().count() as f32;
                let x = bounds.left() + gw - px(1. + 12. + 8.) - w;
                paint_mono(window, &s, run(s.len(), slate::S600.into()), fsize, adv, point(x, line_y(l)), lh);
            }
        });

        let text_area = Bounds::from_corners(origin, bounds.bottom_right());
        window.with_content_mask(Some(ContentMask { bounds: text_area }), |window| {
            if lines == 0 {
                return;
            }
            // Search marks (behind the text).
            if a.marks.len > 0 {
                for (i, &start) in a.marks.starts.iter().enumerate() {
                    let l = a.line_of(start);
                    if l < first || l > last {
                        continue;
                    }
                    let lr = a.line_range(l);
                    let end = (start + a.marks.len).min(lr.end);
                    let c0 = a.text[lr.start..start].chars().count();
                    let c1 = c0 + a.text[start..end.max(start)].chars().count();
                    let r = Bounds::from_corners(point(col_x(c0), line_y(l)), point(col_x(c1), line_y(l) + lh));
                    if i == a.marks.current {
                        window.paint_quad(quad(
                            r,
                            px(2.),
                            theme::hexa(0xfbbf24, 0.35),
                            px(1.),
                            theme::hexa(0xfbbf24, 0.5),
                            BorderStyle::Solid,
                        ));
                    } else {
                        window.paint_quad(fill(r, theme::hexa(0xfbbf24, 0.15)).corner_radii(px(2.)));
                    }
                }
            }
            // Selection.
            if !a.sel.is_empty() {
                let (sl, el) = (a.line_of(a.sel.start), a.line_of(a.sel.end));
                for l in sl.max(first)..=el.min(last) {
                    let lr = a.line_range(l);
                    let s = a.sel.start.max(lr.start);
                    let e = a.sel.end.min(lr.end);
                    let c0 = a.text[lr.start..s].chars().count();
                    let mut c1 = c0 + a.text[s..e].chars().count();
                    if a.sel.end > lr.end {
                        c1 += 1; // newline
                    }
                    let r = Bounds::from_corners(point(col_x(c0), line_y(l)), point(col_x(c1), line_y(l) + lh));
                    window.paint_quad(fill(r, theme::hexa(0x3b82f6, 0.4)));
                }
            }
            for l in first..=last {
                let t = a.line_text(l);
                if t.is_empty() {
                    continue;
                }
                let s: SharedString = t.to_string().into();
                paint_mono(window, &s, run(s.len(), text_color), fsize, adv, point(col_x(0), line_y(l)), lh);
            }
            // Caret (editing only; `caret-accent`).
            if focused && !ro && a.sel.is_empty() {
                let cur = a.cursor();
                let l = a.line_of(cur);
                let c = a.col_of(cur);
                window.paint_quad(fill(
                    Bounds::new(point(col_x(c), line_y(l) + px(2.)), size(px(1.5), lh - px(4.))),
                    accent,
                ));
            }
            // Custom scroll thumbs.
            let content = a.content_size();
            let thumb = theme::hex(0x334155);
            if content.height > viewport.height + px(0.5) {
                let h = (viewport.height * (viewport.height / content.height)).max(px(20.));
                let y = (viewport.height - h) * (scroll.y / (content.height - viewport.height));
                window.paint_quad(
                    fill(Bounds::new(point(bounds.right() - px(6.), origin.y + y), size(px(6.), h)), thumb)
                        .corner_radii(px(3.)),
                );
            }
            if content.width > viewport.width + px(0.5) {
                let w = (viewport.width * (viewport.width / content.width)).max(px(20.));
                let x = (viewport.width - w) * (scroll.x / (content.width - viewport.width));
                window.paint_quad(
                    fill(Bounds::new(point(origin.x + x, bounds.bottom() - px(6.)), size(w, px(6.))), thumb)
                        .corner_radii(px(3.)),
                );
            }
        });

        focus
        });
        let text_area = Bounds::from_corners(self.area.read(cx).text_origin, bounds.bottom_right());
        window.handle_input(&focus, ElementInputHandler::new(text_area, self.area.clone()), cx);
        window.set_cursor_style(CursorStyle::IBeam, &pre.text_hitbox);

        let hitbox = pre.hitbox.clone();
        let area = self.area.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left && hitbox.is_hovered(window) {
                area.update(cx, |a, cx| a.mouse_down(ev, window, cx));
                cx.stop_propagation();
            }
        });
        let area = self.area.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && ev.pressed_button == Some(MouseButton::Left) {
                area.update(cx, |a, cx| a.mouse_drag(ev.position, cx));
            }
        });
        let area = self.area.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, _, _, cx| {
            area.update(cx, |a, _| a.selecting = false);
        });
        let hitbox = pre.hitbox.clone();
        let area = self.area.clone();
        window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                let mut d = ev.delta.pixel_delta(px(LINE_H));
                if ev.modifiers.shift && d.x == px(0.) {
                    d = point(d.y, px(0.));
                }
                area.update(cx, |a, cx| a.scroll_by(d, cx));
                cx.stop_propagation();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{advance, gutter_width, line_starts};

    #[test]
    fn line_index() {
        assert_eq!(line_starts("a\nbc\n"), vec![0, 2, 5]);
        assert_eq!(line_starts(""), vec![0]);
    }

    #[test]
    fn gutter_width_follows_digits() {
        let a = advance();
        assert!((gutter_width(9) - (a + 29.)).abs() < 1e-3);
        assert!((gutter_width(150) - (3. * a + 29.)).abs() < 1e-3);
    }
}
