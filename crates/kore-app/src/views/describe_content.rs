//! DescribeContent (spec 02 §2.6.1): syntax-highlighted JSON in a scrollable
//! `pre-wrap` block with ⌘F search (TextSearchBar).
//!
//! Highlighting is a small JSON-aware tokenizer that splits the text into
//! colored nodes without changing it (string literals are opaque, so digits
//! inside a timestamp string are never colored as numbers). Search runs over
//! those nodes; the nodes become text runs.

use crate::automation::Automation;
use crate::keys;
use crate::theme::{self, amber, emerald, slate, KoreStyled};
use crate::ui::scrollbar::{self, Gutter};
use crate::ui::text_search_bar::{SearchBarEvent, TextSearchBar};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use crate::views::yaml_text_area::LINE_H as WEB_RELAXED_12;
use regex::Regex;
use std::cell::Cell;
use std::rc::Rc;

/// Color class of a highlighted span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Key,
    Str,
    Num,
    Bool,
    Null,
}

/// One text node: its text and its color class.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub text: String,
    pub tok: Tok,
}

/// Split (pretty-printed) JSON into highlighted nodes. The concatenated node
/// text is exactly `json`:
/// - a string literal followed by `:` is a key;
/// - after a `:`, a string, number, `true`/`false` or `null` value gets its color;
/// - everything else (punctuation, array elements, string contents) is plain
///   text or part of its string token, so e.g. `"10:41:26Z"` stays one string.
pub fn highlight_nodes(json: &str) -> Vec<Node> {
    let b = json.as_bytes();
    let mut out: Vec<Node> = Vec::new();
    let push = |out: &mut Vec<Node>, text: &str, tok: Tok| {
        if text.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.tok == tok && tok == Tok::Plain => last.text.push_str(text),
            _ => out.push(Node { text: text.to_string(), tok }),
        }
    };
    // Whether the last significant (non-whitespace) character was a `:`.
    let mut after_colon = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            let mut j = i + 1;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(b.len());
            let mut k = end;
            while k < b.len() && b[k].is_ascii_whitespace() {
                k += 1;
            }
            let tok = if k < b.len() && b[k] == b':' {
                Tok::Key
            } else if after_colon {
                Tok::Str
            } else {
                Tok::Plain
            };
            push(&mut out, &json[i..end], tok);
            after_colon = false;
            i = end;
            continue;
        }
        if after_colon && (c.is_ascii_digit() || (c == b'-' && b.get(i + 1).is_some_and(u8::is_ascii_digit))) {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_digit() || matches!(b[j], b'.' | b'e' | b'E' | b'+' | b'-')) {
                j += 1;
            }
            push(&mut out, &json[i..j], Tok::Num);
            after_colon = false;
            i = j;
            continue;
        }
        if after_colon && c.is_ascii_alphabetic() {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_alphanumeric() {
                j += 1;
            }
            let word = &json[i..j];
            let tok = match word {
                "true" | "false" => Tok::Bool,
                "null" => Tok::Null,
                _ => Tok::Plain,
            };
            push(&mut out, word, tok);
            after_colon = false;
            i = j;
            continue;
        }
        // Plain run up to the next interesting byte (always a char boundary).
        let mut j = i + 1;
        while j < b.len() && !json.is_char_boundary(j) {
            j += 1;
        }
        if !c.is_ascii_whitespace() {
            after_colon = c == b':';
        }
        push(&mut out, &json[i..j], Tok::Plain);
        i = j;
    }
    out
}

/// Search matches (byte ranges in the concatenated text). A match never spans two text nodes; matching is case-insensitive.
pub fn find_matches(nodes: &[Node], query: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let Ok(rx) = Regex::new(&format!("(?i){}", regex::escape(query))) else { return out };
    let mut base = 0;
    for n in nodes {
        for m in rx.find_iter(&n.text) {
            out.push(base + m.start()..base + m.end());
        }
        base += n.text.len();
    }
    out
}

/// Font size at which GPUI's mono glyph advance equals the 0.601em SF Mono
/// advance at `size` (GPUI renders the system mono font ~2.5% wider), so
/// wrapped `pre-wrap` text breaks at the designed columns.
pub fn pitched_mono_size(window: &mut Window, size: f32) -> f32 {
    thread_local! { static RATIO: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) }; }
    let ratio = RATIO.with(|r| {
        if let Some(v) = r.get() {
            return v;
        }
        let w = crate::ui::text::measure(window, &"0".repeat(20), theme::MONO, 100., FontWeight::NORMAL) / 20.;
        let v = if w > 0. { (100. * theme::MONO_ADVANCE_EM) / w } else { 1. };
        r.set(Some(v));
        v
    });
    size * ratio
}

fn tok_color(t: Tok, accent: Rgba) -> Rgba {
    match t {
        Tok::Plain => slate::S100,
        Tok::Key => accent,
        Tok::Str => emerald::E400,
        Tok::Num => amber::A400,
        Tok::Bool => theme::hex(0x818cf8),
        Tok::Null => slate::S500,
    }
}

pub struct DescribeContent {
    content: String,
    nodes: Vec<Node>,
    text: SharedString,
    search: Option<Entity<TextSearchBar>>,
    query: String,
    matches: Vec<std::ops::Range<usize>>,
    current: usize,
    scroll: ScrollHandle,
    gutter: Gutter,
    /// Byte index of the match to center on the next paint.
    pending_scroll: Rc<Cell<Option<usize>>>,
    _subs: Vec<Subscription>,
}

impl DescribeContent {
    pub fn new(content: String, cx: &mut Context<Self>) -> Self {
        let key = keys::on_window_key(cx, |this: &mut Self, k, window, cx| {
            if k.mod_key("f") && !k.shift {
                this.open_search(window, cx);
                return true;
            }
            false
        });
        let mut this = DescribeContent {
            content: String::new(),
            nodes: Vec::new(),
            text: SharedString::default(),
            search: None,
            query: String::new(),
            matches: Vec::new(),
            current: 0,
            scroll: ScrollHandle::new(),
            gutter: Gutter::default(),
            pending_scroll: Rc::new(Cell::new(None)),
            _subs: vec![key],
        };
        this.set_content(content, cx);
        this
    }

    pub fn set_content(&mut self, content: String, cx: &mut Context<Self>) {
        if content == self.content && !self.nodes.is_empty() {
            return;
        }
        let shown = if content.is_empty() { "No details available".to_string() } else { content.clone() };
        self.content = content;
        self.nodes = highlight_nodes(&shown);
        self.text = self.nodes.iter().map(|n| n.text.as_str()).collect::<String>().into();
        self.current = 0;
        self.recompute(cx);
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = &self.search {
            s.update(cx, |s, cx| s.focus(window, cx));
            return;
        }
        let bar = cx.new(|cx| TextSearchBar::new(window, cx));
        self._subs.push(cx.subscribe(&bar, |this, _, ev: &SearchBarEvent, cx| match ev {
            SearchBarEvent::Query(q) => {
                this.query = q.clone();
                this.current = 0;
                this.recompute(cx);
            }
            SearchBarEvent::Next => {
                if !this.matches.is_empty() {
                    this.current = (this.current + 1) % this.matches.len();
                    this.recompute(cx);
                }
            }
            SearchBarEvent::Prev => {
                if !this.matches.is_empty() {
                    let n = this.matches.len();
                    this.current = (this.current + n - 1) % n;
                    this.recompute(cx);
                }
            }
            SearchBarEvent::Close => {
                this.search = None;
                this.query.clear();
                this.current = 0;
                this.recompute(cx);
            }
        }));
        self.search = Some(bar);
        cx.notify();
    }

    fn recompute(&mut self, cx: &mut Context<Self>) {
        self.matches = find_matches(&self.nodes, &self.query);
        if self.current >= self.matches.len() {
            self.current = 0;
        }
        if let Some(bar) = &self.search {
            let (c, n) = (self.current, self.matches.len());
            bar.update(cx, |b, cx| b.set_matches(c, n, cx));
        }
        self.pending_scroll.set(self.matches.get(self.current).map(|r| r.start));
        cx.notify();
    }

    fn runs(&self, accent: Rgba) -> Vec<TextRun> {
        let font = font(SharedString::from(theme::MONO));
        let mut runs: Vec<TextRun> = Vec::new();
        let mut push = |len: usize, color: Rgba, bg: Option<Rgba>| {
            if len == 0 {
                return;
            }
            runs.push(TextRun {
                len,
                font: font.clone(),
                color: color.into(),
                background_color: bg.map(Into::into),
                underline: None,
                strikethrough: None,
            });
        };
        let mut pos = 0usize;
        let mut mi = 0usize;
        for n in &self.nodes {
            let color = tok_color(n.tok, accent);
            let end = pos + n.text.len();
            let mut p = pos;
            while p < end {
                while mi < self.matches.len() && self.matches[mi].end <= p {
                    mi += 1;
                }
                match self.matches.get(mi) {
                    Some(m) if m.start < end => {
                        let s = m.start.max(p);
                        push(s - p, color, None);
                        let e = m.end.min(end);
                        let bg = if mi == self.current {
                            theme::hexa(0xfbbf24, 0.35)
                        } else {
                            theme::hexa(0xfbbf24, 0.15)
                        };
                        push(e - s, color, Some(bg));
                        p = e;
                    }
                    _ => {
                        push(end - p, color, None);
                        p = end;
                    }
                }
            }
            pos = end;
        }
        runs
    }
}

impl Render for DescribeContent {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        // GPUI's wrapper breaks long JSON strings per UAX #14; the font is
        // scaled so the column count matches the 0.601em mono pitch.
        let font_size = pitched_mono_size(window, 12.);
        let styled = StyledText::new(self.text.clone()).with_runs(self.runs(accent));
        let layout = styled.layout().clone();
        let pending = self.pending_scroll.clone();
        let handle = self.scroll.clone();
        let reserve = self.gutter.overflows();
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("describe-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p(px(16.))
                    .when(reserve, |d| d.pr(px(16. + scrollbar::WIDTH)))
                    .font_mono()
                    .text_size(px(font_size))
                    .line_height(px(WEB_RELAXED_12))
                    .text_color(slate::S100)
                    .child(styled.auto_id("describe.text", "text", self.text.clone()))
                    .child(
                        canvas(
                            move |_, window, _| {
                                let Some(ix) = pending.take() else { return };
                                let Some(p) = layout.position_for_index(ix) else { return };
                                let vp = handle.bounds();
                                let off = handle.offset();
                                let content_y = p.y - vp.top() - off.y;
                                let max = handle.max_offset().y;
                                let target = (content_y - vp.size.height / 2. + px(WEB_RELAXED_12 / 2.)).clamp(px(0.), max.max(px(0.)));
                                handle.set_offset(point(off.x, -target));
                                window.refresh();
                            },
                            |_, _, _, _| {},
                        )
                        .size_0(),
                    )
                    .auto_id("describe", "document", "Describe"),
            )
            .child(div().absolute().inset_0().child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone())))
            .children(self.search.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{find_matches, highlight_nodes, Node, Tok};

    fn flat(nodes: &[Node]) -> Vec<(String, Tok)> {
        nodes.iter().map(|n| (n.text.clone(), n.tok)).collect()
    }

    fn text(nodes: &[Node]) -> String {
        nodes.iter().map(|n| n.text.as_str()).collect()
    }

    #[test]
    fn highlights_keys_and_values() {
        let json = "{\n  \"a\": \"x\",\n  \"n\": 3,\n  \"f\": -1.5e3,\n  \"t\": true,\n  \"z\": null,\n  \"l\": [1, \"s\"]\n}";
        let nodes = highlight_nodes(json);
        let f = flat(&nodes);
        assert!(f.contains(&("\"a\"".into(), Tok::Key)));
        assert!(f.contains(&("\"x\"".into(), Tok::Str)));
        assert!(f.contains(&("3".into(), Tok::Num)));
        assert!(f.contains(&("-1.5e3".into(), Tok::Num)));
        assert!(f.contains(&("true".into(), Tok::Bool)));
        assert!(f.contains(&("null".into(), Tok::Null)));
        // Array scalars stay plain; the text is unchanged.
        assert!(!f.contains(&("\"s\"".into(), Tok::Str)));
        assert!(!f.contains(&("1".into(), Tok::Num)));
        assert_eq!(text(&nodes), json);
    }

    #[test]
    fn numbers_inside_strings_are_not_highlighted() {
        // Formerly the number rule matched ":41" inside the timestamp and inserted a space.
        let json = "{\"ts\": \"2026-10-15T10:41:26Z\", \"img\": \"nginx:1.27\", \"k:1\": 2}";
        let nodes = highlight_nodes(json);
        assert_eq!(text(&nodes), json);
        let f = flat(&nodes);
        assert!(f.contains(&("\"2026-10-15T10:41:26Z\"".into(), Tok::Str)));
        assert!(f.contains(&("\"nginx:1.27\"".into(), Tok::Str)));
        assert!(f.contains(&("\"k:1\"".into(), Tok::Key)));
        assert!(f.contains(&("2".into(), Tok::Num)));
        assert!(!f.iter().any(|(t, k)| *k == Tok::Num && (t == "41" || t == "1")));
    }

    #[test]
    fn escapes_and_non_json_text() {
        let json = "{\"m\": \"say \\\"hi\\\": 5\"}";
        let nodes = highlight_nodes(json);
        assert_eq!(text(&nodes), json);
        assert_eq!(flat(&nodes).iter().filter(|(_, k)| *k == Tok::Num).count(), 0);
        let plain = "Error fetching pod details: Kubernetes error: boom ünïcode";
        assert_eq!(text(&highlight_nodes(plain)), plain);
        assert_eq!(text(&highlight_nodes("{\"open\": \"unterminated")), "{\"open\": \"unterminated");
    }

    #[test]
    fn search_is_per_node_and_case_insensitive() {
        let nodes = highlight_nodes("{\"image\": \"Busybox:1.36\"}");
        assert_eq!(find_matches(&nodes, "busybox").len(), 1);
        // Does not span the key/value boundary.
        assert_eq!(find_matches(&nodes, "image\": \"busy").len(), 0);
        assert!(find_matches(&nodes, "").is_empty());
    }
}
