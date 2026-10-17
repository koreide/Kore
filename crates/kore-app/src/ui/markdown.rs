//! Markdown renderer for AI answers (GFM, spec 03 §6.4).
//!
//! [`parse`] turns markdown (GFM: tables, strikethrough, task lists,
//! autolinks, fenced code) into a small block tree; [`render`] lays it out
//! with the AI-answer styles. Raw HTML is dropped, links open in the system
//! browser (quirk 14 — fixed).

use crate::theme::{self, slate, KoreStyled};
use crate::ui::text::measure;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use markdown::mdast::Node;

/// Inline run of text with its formatting.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading(u8, Vec<Span>),
    Code(String),
    List { ordered: bool, start: u32, tight: bool, items: Vec<ListItem> },
    Quote(Vec<Block>),
    Rule,
    Table { header: Vec<Vec<Span>>, rows: Vec<Vec<Vec<Span>>> },
}

/// Parse GFM markdown into blocks.
pub fn parse(src: &str) -> Vec<Block> {
    match markdown::to_mdast(src, &markdown::ParseOptions::gfm()) {
        Ok(Node::Root(root)) => blocks(&root.children),
        _ => vec![Block::Paragraph(vec![Span { text: src.to_string(), ..Default::default() }])],
    }
}

fn blocks(nodes: &[Node]) -> Vec<Block> {
    nodes.iter().filter_map(block).collect()
}

fn block(n: &Node) -> Option<Block> {
    Some(match n {
        Node::Paragraph(p) => {
            let s = spans(&p.children);
            if s.is_empty() {
                return None;
            }
            Block::Paragraph(s)
        }
        Node::Heading(h) => Block::Heading(h.depth, spans(&h.children)),
        Node::Code(c) => Block::Code(c.value.clone()),
        Node::Math(m) => Block::Code(m.value.clone()),
        Node::Blockquote(q) => Block::Quote(blocks(&q.children)),
        Node::ThematicBreak(_) => Block::Rule,
        Node::List(l) => Block::List {
            ordered: l.ordered,
            start: l.start.unwrap_or(1),
            tight: !l.spread && l.children.iter().all(|c| !matches!(c, Node::ListItem(i) if i.spread)),
            items: l
                .children
                .iter()
                .filter_map(|c| match c {
                    Node::ListItem(i) => Some(ListItem { checked: i.checked, blocks: blocks(&i.children) }),
                    _ => None,
                })
                .collect(),
        },
        Node::Table(t) => {
            let mut rows: Vec<Vec<Vec<Span>>> = t
                .children
                .iter()
                .filter_map(|r| match r {
                    Node::TableRow(r) => Some(
                        r.children
                            .iter()
                            .map(|c| match c {
                                Node::TableCell(c) => spans(&c.children),
                                other => spans(std::slice::from_ref(other)),
                            })
                            .collect(),
                    ),
                    _ => None,
                })
                .collect();
            let header = if rows.is_empty() { Vec::new() } else { rows.remove(0) };
            Block::Table { header, rows }
        }
        _ => return None,
    })
}

fn spans(nodes: &[Node]) -> Vec<Span> {
    let mut out = Vec::new();
    collect(nodes, &Span::default(), &mut out);
    // Merge adjacent runs with identical style.
    let mut merged: Vec<Span> = Vec::new();
    for s in out {
        if s.text.is_empty() {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if last.bold == s.bold && last.italic == s.italic && last.code == s.code && last.strike == s.strike && last.link == s.link {
                last.text.push_str(&s.text);
                continue;
            }
        }
        merged.push(s);
    }
    merged
}

fn collect(nodes: &[Node], style: &Span, out: &mut Vec<Span>) {
    for n in nodes {
        match n {
            // Soft line breaks inside a paragraph render as spaces.
            Node::Text(t) => out.push(Span { text: t.value.replace('\n', " "), ..style.clone() }),
            Node::Break(_) => out.push(Span { text: "\n".into(), ..style.clone() }),
            Node::InlineCode(c) => out.push(Span { text: c.value.clone(), code: true, ..style.clone() }),
            Node::InlineMath(c) => out.push(Span { text: c.value.clone(), code: true, ..style.clone() }),
            Node::Strong(s) => collect(&s.children, &Span { bold: true, ..style.clone() }, out),
            Node::Emphasis(s) => collect(&s.children, &Span { italic: true, ..style.clone() }, out),
            Node::Delete(s) => collect(&s.children, &Span { strike: true, ..style.clone() }, out),
            Node::Link(l) => {
                let url = safe_url(&l.url);
                collect(&l.children, &Span { link: url, ..style.clone() }, out)
            }
            Node::LinkReference(l) => collect(&l.children, style, out),
            Node::Image(i) => out.push(Span { text: i.alt.clone(), ..style.clone() }),
            Node::Paragraph(p) => collect(&p.children, style, out),
            // Raw HTML is not rendered.
            Node::Html(_) => {}
            other => {
                if let Some(children) = other.children() {
                    collect(children, style, out);
                }
            }
        }
    }
}

/// URL sanitizer: only http(s), mailto, irc(s), xmpp or relative URLs survive.
fn safe_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    match lower.find(':') {
        None => Some(url.to_string()),
        Some(i) => {
            let scheme = &lower[..i];
            let before_slash = lower.find(['/', '?', '#']).map(|j| j < i).unwrap_or(false);
            if before_slash || ["http", "https", "mailto", "irc", "ircs", "xmpp"].contains(&scheme) {
                Some(url.to_string())
            } else {
                None
            }
        }
    }
}

/// Plain text of spans (for measuring and copy).
pub fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

// ── Rendering ────────────────────────────────────────────────────────────

const FS: f32 = 14.;
const LH: f32 = 14. * 1.625;

/// Vertical margins (top, bottom) of a block, for CSS margin collapsing.
fn margins(b: &Block, in_tight_li: bool) -> (f32, f32) {
    match b {
        Block::Paragraph(_) if in_tight_li => (0., 0.),
        Block::Paragraph(_) => (8., 8.),
        Block::Heading(1, _) | Block::Heading(2, _) => (20., 8.),
        Block::Heading(3, _) => (16., 6.),
        Block::Heading(_, _) => (12., 4.),
        Block::Code(_) | Block::Table { .. } | Block::Quote(_) => (12., 12.),
        Block::List { .. } => (8., 8.),
        Block::Rule => (16., 16.),
    }
}

struct Ctx {
    accent: Rgba,
    id: SharedString,
    counter: usize,
}

impl Ctx {
    fn next_id(&mut self, what: &str) -> ElementId {
        self.counter += 1;
        ElementId::Name(format!("{}-{what}-{}", self.id, self.counter).into())
    }
}

/// Render markdown `src` as the `.ai-markdown` root (14px / 1.625, slate-200).
/// `id` must be unique among siblings (used for scroll containers and links).
pub fn render(id: impl Into<SharedString>, src: &str, window: &mut Window, cx: &App) -> AnyElement {
    let blocks = parse(src);
    let mut c = Ctx { accent: theme::accent(cx), id: id.into(), counter: 0 };
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .text_size(px(FS))
        .line_height(px(LH))
        .text_color(slate::S200)
        .children(render_blocks(&blocks, false, true, &mut c, window))
        .into_any_element()
}

/// Blocks with collapsed margins; the outermost first/last margins are dropped
/// when `root` (`.ai-markdown > :first-child { margin-top: 0 }`).
fn render_blocks(blocks: &[Block], tight: bool, root: bool, c: &mut Ctx, window: &mut Window) -> Vec<AnyElement> {
    let mut out = Vec::new();
    let mut prev_mb: Option<f32> = None;
    let n = blocks.len();
    for (i, b) in blocks.iter().enumerate() {
        let (mt, mb) = margins(b, tight);
        let gap = match prev_mb {
            None => if root { 0. } else { mt },
            Some(p) => p.max(mt),
        };
        let el = render_block(b, tight, c, window);
        out.push(div().mt(px(gap)).min_w_0().child(el).into_any_element());
        prev_mb = Some(mb);
        if i == n - 1 && !root && mb > 0. {
            out.push(div().h(px(mb)).into_any_element());
        }
    }
    out
}

fn render_block(b: &Block, tight: bool, c: &mut Ctx, window: &mut Window) -> AnyElement {
    match b {
        Block::Paragraph(s) => inline(s, c),
        Block::Heading(level, s) => {
            let d = div().text_color(slate::S100);
            let d = match level {
                1 => d.text_size(px(16.)).line_height(px(24.)).font_weight(FontWeight::BOLD),
                2 => d.text_size(px(14.)).line_height(px(20.)).font_weight(FontWeight::BOLD),
                3 => d.font_weight(FontWeight::SEMIBOLD),
                _ => d.font_weight(FontWeight::MEDIUM).text_color(slate::S200),
            };
            d.child(inline(s, c)).into_any_element()
        }
        Block::Code(code) => {
            let id = c.next_id("pre");
            let text = code.strip_suffix('\n').unwrap_or(code).to_string();
            div()
                .id(id)
                .rounded(px(8.))
                .bg(theme::hexa(0x0f172a, 0.8))
                .border_1()
                .border_color(theme::hexa(0x1e293b, 0.6))
                .overflow_x_scroll()
                .child(
                    div()
                        .px(px(16.))
                        .py(px(12.))
                        .font_mono()
                        .text_size(px(12.))
                        .line_height(px(crate::views::yaml_text_area::LINE_H))
                        .text_color(slate::S300)
                        .whitespace_nowrap()
                        .child(text),
                )
                .into_any_element()
        }
        Block::Quote(inner) => div()
            .pl(px(12.))
            .border_l_2()
            .border_color(Rgba { a: 0.4, ..c.accent })
            .text_color(slate::S400)
            .italic()
            .flex()
            .flex_col()
            .children(render_blocks(inner, false, true, c, window))
            .into_any_element(),
        Block::Rule => div().h(px(1.)).w_full().bg(theme::hexa(0x334155, 0.6)).into_any_element(),
        Block::List { ordered, start, tight: list_tight, items } => {
            let _ = tight;
            let mut col = div().ml(px(16.)).flex().flex_col();
            for (i, item) in items.iter().enumerate() {
                let marker: AnyElement = if *ordered {
                    div()
                        .absolute()
                        .top_0()
                        .right_full()
                        .pr(px(4.))
                        .whitespace_nowrap()
                        .child(format!("{}.", *start as usize + i))
                        .into_any_element()
                } else {
                    // `disc` marker, outside the content box.
                    div()
                        .absolute()
                        .top_0()
                        .left(px(-14.))
                        .h(px(LH))
                        .flex()
                        .items_center()
                        .child(div().size(px(5.)).rounded_full().bg(slate::S200))
                        .into_any_element()
                };
                let mut content = render_blocks(&item.blocks, *list_tight, true, c, window);
                if let Some(checked) = item.checked {
                    let bx = div()
                        .flex_none()
                        .size(px(13.))
                        .mr(px(4.))
                        .mt(px(4.))
                        .rounded(px(2.))
                        .border_1()
                        .border_color(slate::S500)
                        .bg(theme::hexa(0x1e293b, 0.6))
                        .when(checked, |d| d.child(div().size_full().flex().items_center().justify_center().text_size(px(10.)).child("✓")))
                        .into_any_element();
                    content = vec![div().flex().flex_row().child(bx).child(div().flex_1().min_w_0().children(content)).into_any_element()];
                }
                col = col.child(
                    div()
                        .relative()
                        .min_w_0()
                        .when(i > 0, |d| d.mt(px(4.)))
                        .child(marker)
                        .child(div().flex().flex_col().min_w_0().children(content)),
                );
            }
            col.into_any_element()
        }
        Block::Table { header, rows } => table(header, rows, c, window),
    }
}

fn table(header: &[Vec<Span>], rows: &[Vec<Vec<Span>>], c: &mut Ctx, window: &mut Window) -> AnyElement {
    let ncols = header.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
    let mut widths = vec![0f32; ncols];
    let cell_w = |s: &[Span], bold: bool, window: &mut Window| -> f32 {
        let w = FontWeight::NORMAL;
        let _ = w;
        s.iter()
            .map(|sp| {
                let (fam, wt) = if sp.code {
                    (theme::MONO, FontWeight::NORMAL)
                } else if bold || sp.bold {
                    (theme::SANS, FontWeight::SEMIBOLD)
                } else {
                    (theme::SANS, FontWeight::NORMAL)
                };
                measure(window, &sp.text, fam, 12., wt) + if sp.code { 8. } else { 0. }
            })
            .sum::<f32>()
    };
    for (i, h) in header.iter().enumerate() {
        widths[i] = widths[i].max(cell_w(h, true, window));
    }
    for r in rows {
        for (i, cell) in r.iter().enumerate().take(ncols) {
            widths[i] = widths[i].max(cell_w(cell, false, window));
        }
    }
    let last = ncols.saturating_sub(1);
    let id = c.next_id("table");
    let head = div().flex().flex_row().border_b_1().border_color(slate::S700).children((0..ncols).map(|i| {
        let s = header.get(i).cloned().unwrap_or_default();
        div()
            .w(px(widths[i].ceil() + 24.))
            .flex_none()
            .px(px(12.))
            .py(px(8.))
            .bg(theme::hexa(0x1e293b, 0.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(slate::S300)
            .when(i == 0, |d| d.rounded_tl(px(6.)))
            .when(i == last, |d| d.rounded_tr(px(6.)))
            .whitespace_nowrap()
            .child(inline(&s, c))
    }));
    let mut body = Vec::new();
    for (ri, r) in rows.iter().enumerate() {
        let g: SharedString = format!("{}-tr-{ri}", c.id).into();
        let row = div()
            .id(ElementId::Name(g.clone()))
            .group(g.clone())
            .flex()
            .flex_row()
            .hover(|s| s.bg(theme::hexa(0x1e293b, 0.2)))
            .children((0..ncols).map(|i| {
                let s = r.get(i).cloned().unwrap_or_default();
                div()
                    .w(px(widths[i].ceil() + 24.))
                    .flex_none()
                    .px(px(12.))
                    .py(px(8.))
                    .text_color(slate::S300)
                    .border_b_1()
                    .border_color(theme::hexa(0x1e293b, 0.4))
                    .whitespace_nowrap()
                    .child(inline(&s, c))
            }));
        body.push(row);
    }
    div()
        .id(id)
        .overflow_x_scroll()
        .text_size(px(12.))
        .line_height(px(16.))
        .child(div().flex().flex_col().child(head).children(body))
        .into_any_element()
}

/// CSS max-content width of the rendered markdown (the bubble is
/// shrink-to-fit; GPUI flex items would otherwise size text to min-content).
pub fn max_content_width(src: &str, window: &mut Window) -> f32 {
    blocks_width(&parse(src), window)
}

fn blocks_width(blocks: &[Block], window: &mut Window) -> f32 {
    blocks.iter().map(|b| block_width(b, window)).fold(0., f32::max)
}

fn spans_width(spans: &[Span], size: f32, base: FontWeight, window: &mut Window) -> f32 {
    // Max over hard line breaks.
    let mut best = 0f32;
    let mut cur = 0f32;
    for sp in spans {
        let mut parts = sp.text.split('\n').peekable();
        while let Some(part) = parts.next() {
            let (fam, wt) = if sp.code {
                (theme::MONO, FontWeight::NORMAL)
            } else if sp.bold {
                (theme::SANS, FontWeight::SEMIBOLD)
            } else {
                (theme::SANS, base)
            };
            cur += measure(window, part, fam, size, wt);
            if sp.code {
                cur += 2. * measure(window, "\u{2009}", theme::MONO, size, FontWeight::NORMAL);
            }
            if parts.peek().is_some() {
                best = best.max(cur);
                cur = 0.;
            }
        }
    }
    best.max(cur)
}

fn block_width(b: &Block, window: &mut Window) -> f32 {
    match b {
        Block::Paragraph(s) => spans_width(s, FS, FontWeight::NORMAL, window),
        Block::Heading(1, s) => spans_width(s, 16., FontWeight::BOLD, window),
        Block::Heading(2, s) => spans_width(s, FS, FontWeight::BOLD, window),
        Block::Heading(3, s) => spans_width(s, FS, FontWeight::SEMIBOLD, window),
        Block::Heading(_, s) => spans_width(s, FS, FontWeight::MEDIUM, window),
        Block::Code(c) => {
            c.lines().map(|l| measure(window, l, theme::MONO, 12., FontWeight::NORMAL)).fold(0., f32::max) + 32. + 2.
        }
        Block::Quote(inner) => 14. + blocks_width(inner, window),
        Block::Rule => 0.,
        Block::List { items, .. } => {
            16. + items
                .iter()
                .map(|i| blocks_width(&i.blocks, window) + if i.checked.is_some() { 17. } else { 0. })
                .fold(0., f32::max)
        }
        Block::Table { header, rows } => {
            let ncols = header.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
            (0..ncols)
                .map(|i| {
                    let h = header.get(i).map(|c| spans_width(c, 12., FontWeight::SEMIBOLD, window)).unwrap_or(0.);
                    let r = rows
                        .iter()
                        .filter_map(|r| r.get(i))
                        .map(|c| spans_width(c, 12., FontWeight::NORMAL, window))
                        .fold(0., f32::max);
                    h.max(r).ceil() + 24.
                })
                .sum()
        }
    }
}

/// Inline spans as one wrapping text element (links clickable).
fn inline(spans: &[Span], c: &mut Ctx) -> AnyElement {
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    let mut families: Vec<(std::ops::Range<usize>, SharedString)> = Vec::new();
    let mut links: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let code_color = mix_white(c.accent, 0.9);
    for s in spans {
        let start = text.len();
        if s.code {
            // `padding: 2px 6px` emulated with thin spaces inside the tinted run.
            text.push('\u{2009}');
            text.push_str(&s.text);
            text.push('\u{2009}');
        } else {
            text.push_str(&crate::ui::text::hyphen_wrap(&s.text));
        }
        let range = start..text.len();
        let mut h = HighlightStyle::default();
        if s.bold {
            h.font_weight = Some(FontWeight::SEMIBOLD);
            h.color = Some(slate::S100.into());
        }
        if s.italic {
            h.font_style = Some(FontStyle::Italic);
            h.color = Some(slate::S300.into());
        }
        if s.strike {
            h.strikethrough = Some(StrikethroughStyle { thickness: px(1.), color: None });
        }
        if s.code {
            h.color = Some(code_color.into());
            h.background_color = Some(theme::hexa(0x1e293b, 0.8).into());
            families.push((range.clone(), theme::MONO.into()));
        }
        if let Some(url) = &s.link {
            h.color = Some(c.accent.into());
            h.underline = Some(UnderlineStyle { thickness: px(1.), color: Some(c.accent.into()), wavy: false });
            links.push((range.clone(), url.clone()));
        }
        if h != HighlightStyle::default() {
            highlights.push((range, h));
        }
    }
    let styled = StyledText::new(text).with_highlights(highlights).with_font_family_overrides(families);
    if links.is_empty() {
        return styled.into_any_element();
    }
    let id = c.next_id("link");
    let ranges: Vec<_> = links.iter().map(|(r, _)| r.clone()).collect();
    let urls: Vec<String> = links.into_iter().map(|(_, u)| u).collect();
    InteractiveText::new(id, styled)
        .on_click(ranges, move |ix, _, cx| {
            if let Some(u) = urls.get(ix) {
                cx.open_url(u);
            }
        })
        .into_any_element()
}

/// `color-mix(in srgb, c p%, white)`.
fn mix_white(c: Rgba, p: f32) -> Rgba {
    Rgba { r: c.r * p + (1. - p), g: c.g * p + (1. - p), b: c.b * p + (1. - p), a: 1. }
}

#[cfg(test)]
mod tests {
    use super::{parse, plain, Block};

    const MOCK: &str = "## Summary\n\nCluster **looks** fine.\n\n| Pod | Status |\n|---|---|\n| web | Running |\n\n`inline` and\n```\ncode block\n```";

    #[test]
    fn parses_mock_answer() {
        let b = parse(MOCK);
        assert_eq!(b.len(), 5, "{b:?}");
        assert!(matches!(&b[0], Block::Heading(2, s) if plain(s) == "Summary"));
        match &b[1] {
            Block::Paragraph(s) => {
                assert_eq!(plain(s), "Cluster looks fine.");
                assert!(s.iter().any(|x| x.bold && x.text == "looks"));
            }
            other => panic!("{other:?}"),
        }
        match &b[2] {
            Block::Table { header, rows } => {
                assert_eq!(header.iter().map(|h| plain(h)).collect::<Vec<_>>(), ["Pod", "Status"]);
                assert_eq!(rows.len(), 1);
                assert_eq!(plain(&rows[0][1]), "Running");
            }
            other => panic!("{other:?}"),
        }
        match &b[3] {
            Block::Paragraph(s) => {
                assert!(s[0].code && s[0].text == "inline");
                assert_eq!(plain(s), "inline and");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(b[4], Block::Code("code block".into()));
    }

    #[test]
    fn bold_error_and_lists() {
        let b = parse("text\n\n**Error:** boom");
        assert!(matches!(&b[1], Block::Paragraph(s) if s[0].bold && s[0].text == "Error:"));
        let b = parse("- a\n- [x] b\n\n1. one\n2. two");
        match &b[0] {
            Block::List { ordered: false, tight: true, items, .. } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[1].checked, Some(true));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&b[1], Block::List { ordered: true, start: 1, .. }));
    }

    #[test]
    fn strips_unsafe_links_and_html() {
        let b = parse("[x](javascript:alert(1)) <b>hi</b> [y](https://a.b)");
        match &b[0] {
            Block::Paragraph(s) => {
                assert!(s.iter().all(|x| x.link.as_deref() != Some("javascript:alert(1)")));
                assert!(s.iter().any(|x| x.link.as_deref() == Some("https://a.b")));
                assert!(!plain(s).contains("<b>"));
            }
            other => panic!("{other:?}"),
        }
    }
}
