//! Text helpers for CSS features GPUI lacks.

use gpui_kit::*;

/// Text with CSS-style `letter-spacing` (GPUI has none). The string is shaped
/// once (keeping kerning) and each glyph is painted at its shaped x plus
/// `index * spacing`; like CSS, spacing also follows the last glyph. Font,
/// size, weight and color are inherited from the parent text style; `em` is
/// relative to the inherited font size.
pub fn tracked(text: impl AsRef<str>, _size: f32, em: f32) -> Tracked {
    Tracked { text: text.as_ref().to_string().into(), em }
}

/// `uppercase tracking-wider` (0.05em).
pub fn caps(text: impl AsRef<str>, size: f32) -> Tracked {
    tracked(text.as_ref().to_uppercase(), size, 0.05)
}

pub struct Tracked {
    text: SharedString,
    em: f32,
}

impl IntoElement for Tracked {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct TrackedLayout {
    line: ShapedLine,
    gap: Pixels,
    line_height: Pixels,
}

fn shape(text: &SharedString, window: &mut Window) -> (ShapedLine, Pixels, Pixels) {
    let style = window.text_style();
    let size = style.font_size.to_pixels(window.rem_size());
    let line_height = style.line_height_in_pixels(window.rem_size());
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(text.clone(), size, &[run], None);
    (line, size, line_height)
}

impl Element for Tracked {
    type RequestLayoutState = Option<TrackedLayout>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (line, size, line_height) = shape(&self.text, window);
        let gap = size * self.em;
        let n = self.text.chars().count() as f32;
        let style = Style {
            flex_shrink: 0.,
            size: Size { width: (line.width + gap * n).into(), height: line_height.into() },
            ..Default::default()
        };
        (window.request_layout(style, [], cx), Some(TrackedLayout { line, gap, line_height }))
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(l) = layout.take() else { return };
        // Re-shape with the paint-time style so the color is current.
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        for (i, (ix, ch)) in self.text.char_indices().enumerate() {
            if ch == ' ' {
                continue;
            }
            let s: SharedString = ch.to_string().into();
            let run = TextRun {
                len: s.len(),
                font: style.font(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let glyph = window.text_system().shape_line(s, size, &[run], None);
            let x = bounds.origin.x + l.line.x_for_index(ix) + l.gap * i as f32;
            glyph.paint(point(x, bounds.origin.y), l.line_height, TextAlign::Left, None, window, cx).ok();
        }
    }
}

/// Width of `text` laid out in a single line.
pub fn measure(window: &mut Window, text: &str, family: &str, size: f32, weight: FontWeight) -> f32 {
    if text.is_empty() {
        return 0.;
    }
    let f = Font { weight, ..font(SharedString::from(family.to_string())) };
    let s: SharedString = text.to_string().into();
    let run = TextRun { len: s.len(), font: f, color: Hsla::default(), background_color: None, underline: None, strikethrough: None };
    let line = window.text_system().shape_line(s, px(size), &[run], None);
    f32::from(line.width)
}

/// Width of a [`tracked`] label: shaped width + spacing after every glyph.
pub fn measure_tracked(window: &mut Window, text: &str, family: &str, size: f32, weight: FontWeight, em: f32) -> f32 {
    measure(window, text, family, size, weight) + size * em * text.chars().count() as f32
}

/// Extra break opportunities for wrapped prose: per UAX #14, text may
/// break after a hyphen that precedes a letter/digit; GPUI's wrapper treats `-`
/// as part of a word. Inserts U+200B (zero-width space) after such hyphens.
pub fn hyphen_wrap(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut chars = s.chars().peekable();
    let mut prev_alnum = false;
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '-' && prev_alnum && chars.peek().map(|n| n.is_alphanumeric()).unwrap_or(false) {
            out.push('\u{200B}');
        }
        prev_alnum = c.is_alphanumeric();
    }
    out
}

/// Break opportunities of browser prose wrapping: after spaces and after a
/// hyphen between letters/digits. Returns the byte ends of each segment.
pub fn break_segments(s: &str) -> Vec<usize> {
    let mut ends = Vec::new();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    for (i, (ix, c)) in chars.iter().enumerate() {
        let next = chars.get(i + 1).map(|(_, n)| *n);
        let end = ix + c.len_utf8();
        let after_space = *c == ' ' && next.map(|n| n != ' ').unwrap_or(false);
        let after_hyphen = *c == '-'
            && i > 0
            && chars[i - 1].1.is_alphanumeric()
            && next.map(|n| n.is_alphanumeric()).unwrap_or(false);
        if after_space || after_hyphen {
            ends.push(end);
        }
    }
    if ends.last() != Some(&s.len()) {
        ends.push(s.len());
    }
    ends
}

/// Greedy line breaking with a width oracle over whole candidate lines, so
/// kerning is accounted for (GPUI's own wrapper sums
/// unkerned per-glyph advances and breaks a word early). Trailing spaces
/// hang (do not count). `\n` forces a break. Returns byte ranges of `text`
/// (trailing spaces excluded).
pub fn wrap_ranges(text: &str, max_width: f32, mut width_of: impl FnMut(&str) -> f32) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut base = 0;
    for para in text.split('\n') {
        let trim = |a: usize, b: usize| a..a + para[a..b].trim_end_matches(' ').len();
        let ends = break_segments(para);
        let mut line_start = 0;
        let mut last_ok = 0; // end of the last segment that fit
        for &end in &ends {
            let candidate = para[line_start..end].trim_end_matches(' ');
            if width_of(candidate) > max_width && last_ok > line_start {
                let r = trim(line_start, last_ok);
                out.push(base + r.start..base + r.end);
                line_start = last_ok;
            }
            last_ok = end;
        }
        let r = trim(line_start, para.len());
        out.push(base + r.start..base + r.end);
        base += para.len() + 1;
    }
    out
}

/// [`wrap_ranges`] as strings.
pub fn wrap_lines(text: &str, max_width: f32, width_of: impl FnMut(&str) -> f32) -> Vec<String> {
    wrap_ranges(text, max_width, width_of).into_iter().map(|r| text[r].to_string()).collect()
}

/// Wrapped prose with kerning-aware line breaks (see [`wrap_ranges`]). Font,
/// size, color and line height are inherited from the parent text style.
pub fn prose_text(text: impl Into<SharedString>) -> ProseText {
    ProseText { text: text.into(), colors: Vec::new(), lines: Default::default() }
}

/// [`prose_text`] made of colored parts (inline `<span>`s); `None` keeps the
/// inherited color.
pub fn prose_text_parts(parts: Vec<(String, Option<Rgba>)>) -> ProseText {
    let mut text = String::new();
    let mut colors = Vec::new();
    for (p, c) in parts {
        let start = text.len();
        text.push_str(&p);
        colors.push((start..text.len(), c));
    }
    ProseText { text: text.into(), colors, lines: Default::default() }
}

pub struct ProseText {
    text: SharedString,
    colors: Vec<(std::ops::Range<usize>, Option<Rgba>)>,
    lines: std::rc::Rc<std::cell::RefCell<Vec<std::ops::Range<usize>>>>,
}

impl IntoElement for ProseText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn run_for(len: usize, style: &TextStyle) -> TextRun {
    TextRun { len, font: style.font(), color: style.color, background_color: None, underline: None, strikethrough: None }
}

/// Text runs for `range` of the full text, colored per `colors`.
fn runs_for(range: &std::ops::Range<usize>, colors: &[(std::ops::Range<usize>, Option<Rgba>)], style: &TextStyle) -> Vec<TextRun> {
    if colors.is_empty() {
        return vec![run_for(range.len(), style)];
    }
    let mut runs = Vec::new();
    for (r, c) in colors {
        let a = r.start.max(range.start);
        let b = r.end.min(range.end);
        if a < b {
            let mut run = run_for(b - a, style);
            if let Some(c) = c {
                run.color = (*c).into();
            }
            runs.push(run);
        }
    }
    runs
}

impl Element for ProseText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let line_height = style.line_height_in_pixels(window.rem_size());
        let text = self.text.clone();
        let lines = self.lines.clone();
        let id = window.request_measured_layout(Default::default(), move |known, avail, window, _| {
            let max = known.width.or(match avail.width {
                AvailableSpace::Definite(x) => Some(x),
                _ => None,
            });
            let mut width_of = |s: &str| {
                if s.is_empty() {
                    return 0.;
                }
                let ss: SharedString = s.to_string().into();
                f32::from(window.text_system().shape_line(ss, size, &[run_for(s.len(), &style)], None).width)
            };
            let result = wrap_ranges(&text, max.map(f32::from).unwrap_or(f32::INFINITY), &mut width_of);
            let widest = result.iter().map(|r| width_of(&text[r.clone()])).fold(0., f32::max);
            let n = result.len().max(1);
            *lines.borrow_mut() = result;
            Size { width: known.width.unwrap_or(px(widest)), height: line_height * n as f32 }
        });
        (id, ())
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
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let line_height = style.line_height_in_pixels(window.rem_size());
        let lines = self.lines.borrow().clone();
        for (i, r) in lines.iter().enumerate() {
            if r.is_empty() {
                continue;
            }
            let s: SharedString = self.text[r.clone()].to_string().into();
            let runs = runs_for(r, &self.colors, &style);
            let shaped = window.text_system().shape_line(s, size, &runs, None);
            let origin = point(bounds.origin.x, bounds.origin.y + line_height * i as f32);
            shaped.paint(origin, line_height, TextAlign::Left, None, window, cx).ok();
        }
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::{break_segments, wrap_lines};

    #[test]
    fn segments_break_after_spaces_and_inner_hyphens() {
        assert_eq!(break_segments("a b-c d"), vec![2, 4, 6, 7]);
        assert_eq!(break_segments("x - y"), vec![2, 4, 5]);
    }

    #[test]
    fn greedy_wrap_uses_whole_line_width() {
        // width = chars, but "kerned" pairs "to" count as 1.5 instead of 2.
        let w = |s: &str| s.len() as f32 - if s.ends_with("to") { 0.5 } else { 0. };
        // "aaaa bb to" is 10 unkerned but 9.5 kerned: it fits a 9.6 line.
        assert_eq!(wrap_lines("aaaa bb to cc", 9.6, w), vec!["aaaa bb to", "cc"]);
        assert_eq!(wrap_lines("aaaa bb to cc", 9.4, w), vec!["aaaa bb", "to cc"]);
        assert_eq!(wrap_lines("one\ntwo", 100., |s: &str| s.len() as f32), vec!["one", "two"]);
        assert_eq!(wrap_lines("longword x", 3., |s: &str| s.len() as f32), vec!["longword", "x"]);
    }
}
