//! Shared pieces of the pod and multi-pod log viewers (spec 02 §2.5, §4):
//! level coloring, search matching, the line buffer, skeleton and the
//! toolbar button/chip styles.

use crate::theme::{self, red, slate, KoreStyled};
use crate::ui::native_controls::{checkbox, SYSTEM_BLUE};
use crate::ui::widgets::skeleton_bar;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::ops::Range;

pub const GREEN_400: Rgba = theme::hex(0x4ade80);
pub const BLUE_400: Rgba = theme::hex(0x60a5fa);
pub const AMBER_400: Rgba = theme::hex(0xfbbf24);

/// Height of a single-line log row (1px top padding + the 19px line).
/// Wrapped lines take `1 + 19 × rows` (variable-height list).
pub const ROW_H: f32 = 20.;
/// `text-xs leading-relaxed` line height: 19px (not 19.5).
pub const LINE_H: f32 = 19.;

/// True when `word` occurs in `upper` with `\b` boundaries on both sides.
fn has_word(upper: &str, word: &str) -> bool {
    let b = upper.as_bytes();
    let is_w = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut from = 0;
    while let Some(i) = upper[from..].find(word) {
        let s = from + i;
        let e = s + word.len();
        let left_ok = s == 0 || !is_w(b[s - 1]);
        let right_ok = e == b.len() || !is_w(b[e]);
        if left_ok && right_ok {
            return true;
        }
        from = s + 1;
        if from >= upper.len() {
            break;
        }
    }
    false
}

/// `getLogLevelColor` (pod logs, `info_rule = true`) and the multi-pod
/// variant without the INFO→blue rule. `None` = no level (green-400).
pub fn level_color(line: &str, info_rule: bool) -> Rgba {
    let upper = line.to_uppercase();
    if has_word(&upper, "ERROR") || has_word(&upper, "FATAL") || has_word(&upper, "PANIC") {
        red::R400
    } else if has_word(&upper, "WARN") || has_word(&upper, "WARNING") {
        AMBER_400
    } else if info_rule && has_word(&upper, "INFO") {
        BLUE_400
    } else if has_word(&upper, "DEBUG") || has_word(&upper, "TRACE") {
        slate::S500
    } else {
        GREEN_400
    }
}

/// Case-insensitive `includes` (JS `toLowerCase().includes`).
pub fn contains_ci(hay: &str, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return false;
    }
    if hay.is_ascii() && query_lower.is_ascii() {
        let (h, q) = (hay.as_bytes(), query_lower.as_bytes());
        return h.len() >= q.len() && h.windows(q.len()).any(|w| w.eq_ignore_ascii_case(q));
    }
    hay.to_lowercase().contains(query_lower)
}

/// Byte ranges of every case-insensitive occurrence of `query` (the
/// `<mark>` spans of `highlightMatches`).
pub fn match_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let ql = query.to_lowercase();
    let tl = text.to_lowercase();
    if tl.len() != text.len() {
        // Lowercasing changed byte lengths: compare char by char.
        let tc: Vec<(usize, char)> = text.char_indices().collect();
        let qc: Vec<char> = ql.chars().collect();
        let mut i = 0;
        while i + qc.len() <= tc.len() {
            let ok = (0..qc.len()).all(|k| tc[i + k].1.to_lowercase().eq(std::iter::once(qc[k])));
            if ok {
                let s = tc[i].0;
                let e = tc.get(i + qc.len()).map(|c| c.0).unwrap_or(text.len());
                out.push(s..e);
                i += qc.len();
            } else {
                i += 1;
            }
        }
        return out;
    }
    let mut from = 0;
    while let Some(i) = tl[from..].find(&ql) {
        let s = from + i;
        out.push(s..s + ql.len());
        from = s + ql.len().max(1);
        if from > tl.len() {
            break;
        }
    }
    out
}

/// Log text at the 0.601em mono pitch (`pre-wrap`), with search marks
/// (amber-200 text on amber-400/30).
pub fn log_text(text: SharedString, query: &str) -> crate::ui::mono_text::MonoParagraph {
    let ranges = if query.is_empty() { Vec::new() } else { match_ranges(&text, query) };
    let p = crate::ui::mono_text::mono_paragraph(text).line_height(px(LINE_H));
    if ranges.is_empty() {
        return p;
    }
    p.colors(ranges.iter().map(|r| (r.clone(), theme::hex(0xfde68a))).collect())
        .highlights(ranges.into_iter().map(|r| (r, theme::hexa(0xfbbf24, 0.3))).collect())
}

/// The pod log buffer: one string split on `\n` (`logs.split("\n")`). Kept
/// as lines so appends are O(new text).
#[derive(Default)]
pub struct LogBuffer {
    lines: Vec<SharedString>,
    len: usize,
}

impl LogBuffer {
    pub fn clear(&mut self) {
        self.lines.clear();
        self.len = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of rows as rendered (`"" → 0`, else `split("\n").length`).
    pub fn rows(&self) -> usize {
        if self.len == 0 {
            0
        } else {
            self.lines.len()
        }
    }

    pub fn line(&self, i: usize) -> SharedString {
        self.lines.get(i).cloned().unwrap_or_default()
    }

    pub fn replace(&mut self, text: &str) {
        self.clear();
        self.append(text);
    }

    /// Append text; returns the index of the first row that changed.
    pub fn append(&mut self, text: &str) -> usize {
        if text.is_empty() {
            return self.lines.len();
        }
        self.len += text.len();
        let mut parts = text.split('\n');
        let first = parts.next().unwrap_or("");
        let changed;
        let n = self.lines.len();
        if let Some(last) = self.lines.last_mut() {
            changed = n - 1;
            if !first.is_empty() {
                let mut s = String::with_capacity(last.len() + first.len());
                s.push_str(last);
                s.push_str(first);
                *last = s.into();
            }
        } else {
            changed = 0;
            self.lines.push(first.to_string().into());
        }
        for p in parts {
            self.lines.push(p.to_string().into());
        }
        changed
    }

    pub fn text(&self) -> String {
        if self.len == 0 {
            return String::new();
        }
        let mut s = String::with_capacity(self.len);
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(l);
        }
        s
    }

    /// Indexes of rows whose lower-cased text contains `query`.
    pub fn matches(&self, query: &str) -> Vec<usize> {
        if query.is_empty() || self.len == 0 {
            return Vec::new();
        }
        let q = query.to_lowercase();
        self.lines.iter().enumerate().filter(|(_, l)| contains_ci(l, &q)).map(|(i, _)| i).collect()
    }
}

/// LogSkeleton: 12 bars, 12px tall, 10px gaps, padding 16.
pub fn log_skeleton(id: &str) -> impl IntoElement {
    const W: [f32; 12] = [0.80, 0.65, 0.90, 0.45, 0.75, 0.55, 0.85, 0.40, 0.70, 0.60, 0.88, 0.50];
    div().p(px(16.)).flex().flex_col().gap(px(10.)).children(W.iter().enumerate().map(|(i, w)| {
        div()
            .w(relative(*w))
            .opacity(1. - i as f32 * 0.06)
            .child(skeleton_bar(SharedString::from(format!("{id}-sk-{i}")), relative(1.), 12.))
    }))
}

/// Standard bordered toolbar button (spec 02 §0.4). `active` = toggled
/// (`border-accent/50` renders as the preflight gray, bg tint dropped).
pub fn tool_button(id: impl Into<ElementId>, active: bool, accent: Rgba, px_: f32, py_: f32) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .px(px(px_))
        .py(px(py_))
        .rounded(px(6.))
        .border_1()
        .cursor_pointer()
        .when(active, |d| d.border_color(theme::PREFLIGHT_BORDER).text_color(accent))
        .when(!active, |d| {
            d.border_color(slate::S800)
                .text_color(slate::S300)
                .hover(|s| s.border_color(theme::PREFLIGHT_BORDER).bg(theme::hexa(0x1d2939, 0.3)))
        })
}

/// Toolbar checkbox chip (`label` with a native checkbox).
pub fn check_chip(id: impl Into<ElementId>, checked: bool, label: &'static str, size: f32, gap: f32, px_: f32, py_: f32) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(gap))
        .px(px(px_))
        .py(px(py_))
        .rounded(px(6.))
        .border_1()
        .border_color(slate::S800)
        .text_color(slate::S300)
        .cursor_pointer()
        .hover(|s| s.border_color(theme::PREFLIGHT_BORDER).bg(theme::hexa(0x1d2939, 0.3)))
        .child(checkbox(checked, size, SYSTEM_BLUE))
        .child(div().child(label))
}

/// The logs card (`bg-black/40 border rounded-lg m-4`): inset 16px on every
/// side, so the whole card, bottom border and corners included, is visible.
pub fn content_card() -> Div {
    div()
        .absolute()
        .top(px(16.))
        .left(px(16.))
        .right(px(16.))
        .bottom(px(16.))
        .bg(theme::hexa(0, 0.4))
        .border_1()
        .border_color(slate::S800)
        .rounded(px(8.))
        .overflow_hidden()
}

/// Line-number gutter cell (48px, right-aligned, border-r slate-800/50).
/// 1px top padding: a single-line row is exactly [`ROW_H`] tall.
pub fn gutter_cell(n: usize, group: &'static str) -> Div {
    div()
        .w(px(48.))
        .flex_none()
        .pr(px(12.))
        .pt(px(1.))
        .flex()
        .justify_end()
        .border_r_1()
        .border_color(theme::hexa(0x1e293b, 0.5))
        .text_color(slate::S600)
        .group_hover(group, |s| s.text_color(slate::S500))
        .child(n.to_string())
}

/// Painted 6px scrollbar thumb for a variable-height log `list` (place it as
/// an absolutely positioned overlay over the list's row). Dragging scrolls.
pub fn list_scrollbar(state: ListState) -> impl IntoElement {
    use std::cell::Cell;
    use std::rc::Rc;
    let drag: Rc<Cell<Option<(Pixels, Pixels)>>> = Rc::new(Cell::new(None));
    canvas(
        {
            let state = state.clone();
            move |b, window, _| {
                let vp = state.viewport_bounds();
                let max = state.max_offset_for_scrollbar().y;
                if max <= px(0.5) || vp.size.height <= px(0.) {
                    return None;
                }
                let vh = vp.size.height;
                let thumb_h = (vh * (vh / (vh + max))).max(px(20.));
                let off = -state.scroll_px_offset_for_scrollbar().y;
                let track = vh - thumb_h;
                let y = track * (off / max).clamp(0., 1.);
                let thumb = Bounds::new(
                    point(b.right() - px(crate::ui::scrollbar::WIDTH), vp.top() + y),
                    size(px(crate::ui::scrollbar::WIDTH), thumb_h),
                );
                let hitbox = window.insert_hitbox(thumb, HitboxBehavior::Normal);
                Some((thumb, hitbox, track, max))
            }
        },
        move |_, prep, window, _| {
            let Some((thumb, hitbox, track, max)) = prep else { return };
            let hovered = hitbox.is_hovered(window) || drag.get().is_some();
            window.paint_quad(fill(thumb, theme::hex(if hovered { 0x475569 } else { 0x334155 })).corner_radii(px(3.)));
            let (d, st) = (drag.clone(), state.clone());
            window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && e.button == MouseButton::Left && hitbox.is_hovered(window) {
                    d.set(Some((e.position.y, -st.scroll_px_offset_for_scrollbar().y)));
                    st.scrollbar_drag_started();
                    cx.stop_propagation();
                }
            });
            let (d, st) = (drag.clone(), state.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, _| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                if let Some((start_y, start_off)) = d.get() {
                    if e.pressed_button != Some(MouseButton::Left) {
                        d.set(None);
                        st.scrollbar_drag_ended();
                        return;
                    }
                    let scale = if track > px(0.) { max / track } else { 0. };
                    let next = (start_off + (e.position.y - start_y) * scale).clamp(px(0.), max);
                    st.set_offset_from_scrollbar(point(px(0.), -next));
                    window.refresh();
                }
            });
            let (d, st) = (drag.clone(), state.clone());
            window.on_mouse_event(move |_: &MouseUpEvent, _, _, _| {
                if d.take().is_some() {
                    st.scrollbar_drag_ended();
                }
            });
        },
    )
    .absolute()
    .inset_0()
}

/// Mono 12px / 19.5px text used by log rows.
pub fn log_font(d: Div) -> Div {
    d.font_mono().text_size(px(12.)).line_height(px(LINE_H))
}

#[cfg(test)]
mod tests {
    use super::{contains_ci, level_color, match_ranges, LogBuffer, AMBER_400, BLUE_400, GREEN_400};
    use crate::theme::{red, slate};

    #[test]
    fn levels() {
        assert_eq!(level_color("KORE-LOG x line=1 level=ERROR", true), red::R400);
        assert_eq!(level_color("fatal: boom", true), red::R400);
        assert_eq!(level_color("WARN line 3", true), AMBER_400);
        assert_eq!(level_color("[warning] x", true), AMBER_400);
        assert_eq!(level_color("level=INFO", true), BLUE_400);
        assert_eq!(level_color("level=INFO", false), GREEN_400);
        assert_eq!(level_color("debug: x", true), slate::S500);
        assert_eq!(level_color("trace", false), slate::S500);
        // Word boundaries: ERRORS / INFORMATION / _INFO do not match.
        assert_eq!(level_color("ERRORS happen", true), GREEN_400);
        assert_eq!(level_color("INFORMATION", true), GREEN_400);
        assert_eq!(level_color("x_INFO", true), GREEN_400);
        assert_eq!(level_color("10-listen-on-ipv6-by-default.sh: info: Getting", true), BLUE_400);
        // First rule wins.
        assert_eq!(level_color("INFO then ERROR", true), red::R400);
        assert_eq!(level_color("", true), GREEN_400);
    }

    #[test]
    fn ranges() {
        assert_eq!(match_ranges("Level=ERROR level=error", "level="), vec![0..6, 12..18]);
        assert_eq!(match_ranges("aaaa", "aa"), vec![0..2, 2..4]);
        assert!(match_ranges("abc", "").is_empty());
        assert!(contains_ci("Hello World", "world"));
        assert!(!contains_ci("Hello", ""));
        // Non-ASCII with length-changing lowercase.
        assert_eq!(match_ranges("İx", "x"), vec![2..3]);
    }

    #[test]
    fn buffer_split_semantics() {
        let mut b = LogBuffer::default();
        assert_eq!(b.rows(), 0);
        b.append("a\n");
        assert_eq!(b.rows(), 2); // trailing empty row
        b.append("b");
        b.append("c\nd\n");
        assert_eq!(b.rows(), 4);
        assert_eq!(b.line(1).as_ref(), "bc");
        assert_eq!(b.text(), "a\nbc\nd\n");
        assert_eq!(b.matches("BC"), vec![1]);
        b.replace("x");
        assert_eq!(b.rows(), 1);
        b.append("\n[ERROR] boom\n");
        assert_eq!(b.text(), "x\n[ERROR] boom\n");
        b.clear();
        assert!(b.is_empty());
    }
}
