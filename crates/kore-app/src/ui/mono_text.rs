//! Single-line monospace text at the 0.601em SF Mono pitch.
//!
//! The design lays out `SFMono-Regular` with a tighter pitch than the font's
//! own advance (≈7.22px vs 7.42px at 12px: same glyphs, same size). GPUI has
//! no letter-spacing, so this element shapes the text normally (glyph ids, font
//! fallback) and paints every glyph at `char_index * pitch` with
//! `pitch = theme::MONO_ADVANCE_EM * font_size`.
//!
//! Font family, size, weight, color and line height come from the inherited
//! text style, like a plain string child. Use [`MonoText::truncate`] for the
//! CSS `truncate` (ellipsis) behaviour.

use crate::theme;
use gpui_kit::*;

pub struct MonoText {
    text: SharedString,
    max_width: Option<Pixels>,
}

/// Mono text at the 0.601em pitch. The parent should set `.font_mono()` and the size.
pub fn mono_text(text: impl Into<SharedString>) -> MonoText {
    MonoText { text: text.into(), max_width: None }
}

/// Pitch (advance per character) of mono text at `font_size`.
pub fn pitch(font_size: Pixels) -> Pixels {
    font_size * theme::MONO_ADVANCE_EM
}

/// Width of `chars` mono characters at `font_size` (0.601em pitch).
pub fn width(chars: usize, font_size: f32) -> f32 {
    chars as f32 * font_size * theme::MONO_ADVANCE_EM
}

impl MonoText {
    /// Clip to `max` and end with "…" when the text is longer (CSS `truncate`
    /// with a max-width).
    pub fn truncate(mut self, max: Pixels) -> Self {
        self.max_width = Some(max);
        self
    }
}

impl IntoElement for MonoText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct MonoLayout {
    chars: usize,
    shown: usize,
    ellipsis: bool,
}

impl Element for MonoText {
    type RequestLayoutState = MonoLayout;
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
    ) -> (LayoutId, MonoLayout) {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let line_height = style.line_height_in_pixels(window.rem_size());
        let p = pitch(size);
        let chars = self.text.chars().count();
        let mut w = p * chars as f32;
        let (mut shown, mut ellipsis) = (chars, false);
        if let Some(max) = self.max_width {
            if w > max {
                ellipsis = true;
                shown = ((max / p).floor() as usize).saturating_sub(1);
                w = max;
            }
        }
        let style = Style {
            flex_shrink: 0.,
            size: Size { width: w.into(), height: line_height.into() },
            ..Default::default()
        };
        (window.request_layout(style, [], cx), MonoLayout { chars, shown, ellipsis })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut MonoLayout,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut MonoLayout,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if layout.chars == 0 {
            return;
        }
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let line_height = style.line_height_in_pixels(window.rem_size());
        let color = style.color;
        let text: SharedString = if layout.ellipsis {
            let mut s: String = self.text.chars().take(layout.shown).collect();
            s.push('…');
            s.into()
        } else {
            self.text.clone()
        };
        let run = TextRun { len: text.len(), font: style.font(), color, background_color: None, underline: None, strikethrough: None };
        let line = window.text_system().layout_line(&text, size, &[run], None);
        let p = pitch(size);
        let padding_top = (line_height - line.ascent - line.descent) / 2.;
        let baseline = bounds.origin.y + padding_top + line.ascent;
        // Map byte index → char index once.
        let char_ix: Vec<usize> = {
            let mut v = vec![0; text.len() + 1];
            for (ci, (bi, _)) in text.char_indices().enumerate() {
                v[bi] = ci;
            }
            v
        };
        let _ = cx;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for run in &line.runs {
                for glyph in &run.glyphs {
                    let ci = char_ix.get(glyph.index).copied().unwrap_or(0);
                    let origin = point(bounds.origin.x + p * ci as f32, baseline);
                    if glyph.is_emoji {
                        window.paint_emoji(origin, run.font_id, glyph.id, size).ok();
                    } else {
                        window.paint_glyph(origin, run.font_id, glyph.id, size, color).ok();
                    }
                }
            }
        });
    }
}

// ── Wrapped mono text (CSS `white-space: pre-wrap; word-break: break-word`) ──

/// Tab stop width (CSS `tab-size: 8`).
pub const TAB_SIZE: usize = 8;

/// Columns that fit in `width` at `font_size` (0.601em pitch), at least 1.
pub fn cols_for(width: Pixels, font_size: Pixels) -> usize {
    let p = pitch(font_size);
    if p <= px(0.) {
        return 1;
    }
    ((width / p) + 0.001).floor().max(1.) as usize
}

/// One visual row: byte range into the text plus the column of each char is
/// implied (chars are laid out from column 0; tabs advance to the next stop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub start: usize,
    pub end: usize,
}

fn char_cols(c: char, col: usize) -> usize {
    if c == '\t' {
        TAB_SIZE - (col % TAB_SIZE)
    } else {
        1
    }
}

/// Split `text` into visual rows of at most `cols` columns, like CSS pre-wrap
/// with break-word: explicit `\n` always breaks (the `\n` is not part of a
/// row); lines wrap at UAX #14 break opportunities (see [`break_allowed`]) and
/// after runs of spaces; a word longer than a row breaks anywhere; spaces that
/// overflow the row "hang" at its end instead of wrapping.
/// An empty text (or empty line) still yields one empty row.
pub fn wrap_rows(text: &str, cols: usize) -> Vec<Row> {
    let cols = cols.max(1);
    let mut rows = Vec::new();
    let mut line_start = 0;
    for line in text.split('\n') {
        wrap_line(line_start, line, cols, &mut rows);
        line_start += line.len() + 1;
    }
    rows
}

fn wrap_line(base: usize, line: &str, cols: usize, rows: &mut Vec<Row>) {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut row_start = 0usize; // index into chars
    let mut col = 0usize;
    let mut last_break: Option<usize> = None; // char index where the next row may start
    let mut i = 0usize;
    if chars.is_empty() {
        rows.push(Row { start: base, end: base });
        return;
    }
    while i < chars.len() {
        let c = chars[i].1;
        let w = char_cols(c, col);
        if c == ' ' || c == '\t' {
            // Spaces never start a wrap; they hang past the edge.
            col += w;
            last_break = Some(i + 1);
            i += 1;
            continue;
        }
        if col + w > cols && i > row_start {
            let brk = match last_break {
                Some(b) if b > row_start && b <= i => b,
                _ => i,
            };
            let start = chars[row_start].0;
            let end = chars[brk - 1].0 + chars[brk - 1].1.len_utf8();
            rows.push(Row { start: base + start, end: base + end });
            row_start = brk;
            last_break = None;
            // Re-measure the carried-over part of the word.
            col = 0;
            for &(_, ch) in &chars[row_start..i] {
                col += char_cols(ch, col);
            }
            continue;
        }
        col += w;
        if let Some(&(_, n)) = chars.get(i + 1) {
            if n != ' ' && n != '\t' && break_allowed(c, n) {
                last_break = Some(i + 1);
            }
        }
        i += 1;
    }
    let start = chars[row_start].0;
    rows.push(Row { start: base + start, end: base + line.len() });
}

/// UAX #14 line-break class (ASCII subset; everything else is AL).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lb {
    Al,
    Nu,
    Hy,
    Ba,
    Is,
    Qu,
    Op,
    Cl,
    Pr,
    Po,
    Sy,
    Ex,
}

fn lb_class(c: char) -> Lb {
    match c {
        '0'..='9' => Lb::Nu,
        '-' => Lb::Hy,
        '|' | '\t' => Lb::Ba,
        ',' | '.' | ':' | ';' => Lb::Is,
        '"' | '\'' => Lb::Qu,
        '(' | '[' | '{' => Lb::Op,
        ')' | ']' | '}' => Lb::Cl,
        '\\' | '$' | '+' => Lb::Pr,
        '%' => Lb::Po,
        '/' => Lb::Sy,
        '!' | '?' => Lb::Ex,
        _ => Lb::Al,
    }
}

/// Whether a line may break between `a` and `b` (neither a space), per the
/// UAX #14 pair rules (LB13–LB31, simplified to ASCII).
pub fn break_allowed(a: char, b: char) -> bool {
    use Lb::*;
    let (x, y) = (lb_class(a), lb_class(b));
    // LB13 / LB21: no break before CL, EX, IS, SY, BA, HY.
    if matches!(y, Cl | Ex | Is | Sy | Ba | Hy) {
        return false;
    }
    // LB14: no break after OP. LB19: no break around quotation marks.
    if x == Op || x == Qu || y == Qu {
        return false;
    }
    match (x, y) {
        // LB23/LB28: letters and digits stay together.
        (Al | Nu, Al | Nu) => false,
        // LB24/LB25: prefix/postfix and numeric sequences.
        (Pr | Po, Al | Nu | Op) => false,
        (Al | Nu, Pr | Po) => false,
        (Hy | Is | Sy, Nu) => false,
        // LB29: IS × AL.
        (Is, Al) => false,
        // LB30: (AL|NU) × OP, CL × (AL|NU) is allowed only for CP; keep CL ÷.
        (Al | Nu, Op) => false,
        // LB31: everything else breaks.
        _ => true,
    }
}

/// Number of visual rows `text` occupies in `width` at `font_size` (for
/// virtualized lists that need row heights up front).
pub fn row_count(text: &str, width: Pixels, font_size: Pixels) -> usize {
    wrap_rows(text, cols_for(width, font_size)).len()
}

/// Wrapped monospace paragraph at the 0.601em pitch. Size, weight, family and the
/// default color come from the inherited text style.
pub struct MonoParagraph {
    text: SharedString,
    colors: Vec<(std::ops::Range<usize>, Hsla)>,
    highlights: Vec<(std::ops::Range<usize>, Hsla)>,
    line_height: Option<Pixels>,
    wrap: bool,
}

/// Wrapped mono text: CSS `pre-wrap` + `break-word` at the 0.601em pitch.
pub fn mono_paragraph(text: impl Into<SharedString>) -> MonoParagraph {
    MonoParagraph { text: text.into(), colors: Vec::new(), highlights: Vec::new(), line_height: None, wrap: true }
}

impl MonoParagraph {
    /// Per-span text colors (byte ranges; unspecified bytes use the inherited color).
    pub fn colors(mut self, spans: Vec<(std::ops::Range<usize>, impl Into<Hsla>)>) -> Self {
        self.colors = spans.into_iter().map(|(r, c)| (r, c.into())).collect();
        self
    }

    /// Background highlights (search matches), painted behind the glyphs.
    pub fn highlights(mut self, spans: Vec<(std::ops::Range<usize>, impl Into<Hsla>)>) -> Self {
        self.highlights = spans.into_iter().map(|(r, c)| (r, c.into())).collect();
        self
    }

    /// Fixed row height (e.g. 19px for 12px `leading-relaxed` rows).
    pub fn line_height(mut self, h: Pixels) -> Self {
        self.line_height = Some(h);
        self
    }

    /// `false` = `white-space: pre` (no wrapping; clipped by the parent).
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }
}

impl IntoElement for MonoParagraph {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn max_line_cols(text: &str) -> usize {
    text.split('\n')
        .map(|l| {
            let mut col = 0;
            for c in l.chars() {
                col += char_cols(c, col);
            }
            col
        })
        .max()
        .unwrap_or(0)
}

impl Element for MonoParagraph {
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
        _: &mut App,
    ) -> (LayoutId, ()) {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let lh = self.line_height.unwrap_or_else(|| style.line_height_in_pixels(window.rem_size()));
        let text = self.text.clone();
        let wrap = self.wrap;
        let natural = pitch(size) * max_line_cols(&text) as f32;
        let layout = window.request_measured_layout(Style::default(), move |known, available, _, _| {
            let width = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(w) => w.min(natural),
                AvailableSpace::MinContent => pitch(size),
                AvailableSpace::MaxContent => natural,
            });
            let rows = if wrap {
                let avail = known.width.or(match available.width {
                    AvailableSpace::Definite(w) => Some(w),
                    _ => None,
                });
                match avail {
                    Some(w) => wrap_rows(&text, cols_for(w, size)).len(),
                    None => text.split('\n').count(),
                }
            } else {
                text.split('\n').count()
            };
            size_of(width, lh * rows as f32)
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let style = window.text_style();
        let size = style.font_size.to_pixels(window.rem_size());
        let lh = self.line_height.unwrap_or_else(|| style.line_height_in_pixels(window.rem_size()));
        let p = pitch(size);
        let cols = if self.wrap { cols_for(bounds.size.width, size) } else { usize::MAX / 2 };
        let rows = wrap_rows(&self.text, cols);
        let default_color = style.color;
        let font = style.font();
        let text = self.text.clone();
        // Visible rows only (cheap for long texts inside scroll views).
        let mask = window.content_mask().bounds;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (ri, row) in rows.iter().enumerate() {
                let top = bounds.origin.y + lh * ri as f32;
                if top + lh < mask.origin.y || top > mask.origin.y + mask.size.height {
                    continue;
                }
                if row.start == row.end {
                    continue;
                }
                let slice = &text[row.start..row.end];
                // Column of each byte in the row.
                let mut col_of = vec![0usize; slice.len() + 1];
                let mut col = 0;
                for (bi, c) in slice.char_indices() {
                    col_of[bi] = col;
                    col += char_cols(c, col);
                }
                for (r, color) in &self.highlights {
                    let s = r.start.max(row.start);
                    let e = r.end.min(row.end);
                    if s < e {
                        let c0 = col_of[s - row.start];
                        let c1 = if e - row.start < slice.len() { col_of[e - row.start] } else { col };
                        window.paint_quad(fill(
                            Bounds::new(point(bounds.origin.x + p * c0 as f32, top), size_of(p * (c1 - c0) as f32, lh)),
                            *color,
                        ));
                    }
                }
                let run = TextRun {
                    len: slice.len(),
                    font: font.clone(),
                    color: default_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window.text_system().layout_line(slice, size, &[run], None);
                let baseline = top + (lh - line.ascent - line.descent) / 2. + line.ascent;
                for srun in &line.runs {
                    for g in &srun.glyphs {
                        if slice[g.index..].starts_with('\t') {
                            continue;
                        }
                        let abs = row.start + g.index;
                        let color = self
                            .colors
                            .iter()
                            .rev()
                            .find(|(r, _)| r.contains(&abs))
                            .map(|(_, c)| *c)
                            .unwrap_or(default_color);
                        let origin = point(bounds.origin.x + p * col_of[g.index] as f32, baseline);
                        if g.is_emoji {
                            window.paint_emoji(origin, srun.font_id, g.id, size).ok();
                        } else {
                            window.paint_glyph(origin, srun.font_id, g.id, size, color).ok();
                        }
                    }
                }
            }
        });
    }
}

fn size_of(w: Pixels, h: Pixels) -> Size<Pixels> {
    Size { width: w, height: h }
}

#[cfg(test)]
mod tests {
    use super::{max_line_cols, wrap_rows, Row};

    fn rows(text: &str, cols: usize) -> Vec<&str> {
        wrap_rows(text, cols).into_iter().map(|Row { start, end }| &text[start..end]).collect()
    }

    #[test]
    fn short_and_empty() {
        assert_eq!(rows("", 10), vec![""]);
        assert_eq!(rows("abc", 10), vec!["abc"]);
        assert_eq!(rows("a\n\nb", 10), vec!["a", "", "b"]);
        assert_eq!(rows("abc\n", 10), vec!["abc", ""]);
    }

    #[test]
    fn wraps_at_spaces_and_hangs_trailing_spaces() {
        assert_eq!(rows("hello world foo", 11), vec!["hello world ", "foo"]);
        assert_eq!(rows("hello world", 5), vec!["hello ", "world"]);
        assert_eq!(rows("aa    bb", 4), vec!["aa    ", "bb"]);
    }

    #[test]
    fn breaks_long_words_anywhere() {
        assert_eq!(rows("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(rows("x abcdefgh", 4), vec!["x ", "abcd", "efgh"]);
    }

    #[test]
    fn breaks_after_hyphen_between_words() {
        assert_eq!(rows("kore-demo-probe", 10), vec!["kore-demo-", "probe"]);
        // LB25: no break between a hyphen and a digit.
        assert_eq!(rows("kore-demo-6978", 10), vec!["kore-", "demo-6978"]);
        assert_eq!(rows("a - b", 3), vec!["a - ", "b"]);
    }

    #[test]
    fn uax14_json_breaks() {
        // Break between `:` and `{`, and after `,` before `\"` (prefix class).
        let s = r#"\"annotations\":{},\"name\""#;
        assert_eq!(rows(s, 16), vec![r#"\"annotations\":"#, r#"{},\"name\""#]);
        assert_eq!(rows(r#"{},\"name\""#, 4), vec!["{},", r#"\"na"#, r#"me\""#]);
        // No break inside numbers / around quotes / before closing brackets.
        assert!(!super::break_allowed('3', '.'));
        assert!(!super::break_allowed('a', '"'));
        assert!(!super::break_allowed('a', ')'));
        assert!(super::break_allowed(')', 'a'));
        assert!(super::break_allowed('/', 'a'));
        assert!(!super::break_allowed('-', '6'));
    }

    #[test]
    fn tabs_advance_to_stops() {
        assert_eq!(max_line_cols("a\tb"), 9);
        assert_eq!(rows("\tabcdefgh", 12), vec!["\t", "abcdefgh"]);
        assert_eq!(rows("ab\tcdefghijklmn", 12), vec!["ab\t", "cdefghijklmn"]);
    }

    #[test]
    fn multibyte() {
        assert_eq!(rows("ééééé", 2), vec!["éé", "éé", "é"]);
    }
}
