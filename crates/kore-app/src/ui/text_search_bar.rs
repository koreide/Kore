//! TextSearchBar (spec 01 §16), shared by the describe / YAML / log views.
//!
//! The owner keeps the matches: it listens to [`SearchBarEvent`]s, computes
//! matches for `Query`, moves between them on `Next`/`Prev`, and reports the
//! position back with [`TextSearchBar::set_matches`]. Place the entity inside a
//! `relative()` container; it positions itself (top 8, right 24).

use crate::automation::Automation;
use crate::theme::{self, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::time::Duration;

pub enum SearchBarEvent {
    Query(String),
    Next,
    Prev,
    Close,
}

impl EventEmitter<SearchBarEvent> for TextSearchBar {}

pub struct TextSearchBar {
    input: Entity<TextInput>,
    query: String,
    current: usize,
    count: usize,
    /// Pod/multi-pod log variant (spec 02 §2.5.5): `Search logs...`
    /// placeholder, no "No results" label, counter with 4px margins.
    logs_mode: bool,
    _sub: Subscription,
}

impl TextSearchBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(
                "Search...",
                InputStyle {
                    text_color: slate::S200,
                    placeholder_color: slate::S600,
                    font_size: 12.,
                    line_height: 16.,
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let sub = cx.subscribe(&input, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::Change(q) => {
                this.query = q.clone();
                cx.emit(SearchBarEvent::Query(q.clone()));
                cx.notify();
            }
            InputEvent::Enter { shift: false } => cx.emit(SearchBarEvent::Next),
            InputEvent::Enter { shift: true } => cx.emit(SearchBarEvent::Prev),
            // Esc while the input is focused closes the bar (and nothing else).
            InputEvent::Escape => cx.emit(SearchBarEvent::Close),
            _ => {}
        });
        // Auto-focus after 50ms.
        let focus_input = input.clone();
        let t = cx.background_executor().timer(Duration::from_millis(50));
        cx.spawn_in(window, async move |_, cx| {
            t.await;
            cx.update(|window, cx| focus_input.update(cx, |i, cx| i.focus(window, cx))).ok();
        })
        .detach();
        TextSearchBar { input, query: String::new(), current: 0, count: 0, logs_mode: false, _sub: sub }
    }

    /// Switch to the log-search variant (see `logs_mode`).
    pub fn set_logs_mode(&mut self, cx: &mut Context<Self>) {
        self.logs_mode = true;
        self.input.update(cx, |i, cx| i.set_placeholder("Search logs...", cx));
        cx.notify();
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Report the match position (0-based `current`) and total.
    pub fn set_matches(&mut self, current: usize, count: usize, cx: &mut Context<Self>) {
        self.current = current;
        self.count = count;
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        let h = self.input.focus_handle(cx);
        window.focus(&h, cx);
    }
}

impl Render for TextSearchBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let btn = |id: &'static str, ic: &'static str| {
            div()
                .id(id)
                .group(id)
                .cursor_pointer()
                .child(icon(ic, 14.).text_color(slate::S400).group_hover(id, |s| s.text_color(slate::S200)))
        };
        div()
            .absolute()
            .top(px(8.))
            .right(px(24.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .bg(theme::SURFACE)
            .border_1()
            .border_color(slate::S700)
            .rounded(px(8.))
            .px(px(12.))
            .py(px(6.))
            .shadow(vec![
                BoxShadow {
                    color: theme::hexa(0, 0.1).into(),
                    offset: point(px(0.), px(10.)),
                    blur_radius: px(15.),
                    spread_radius: px(-3.),
                    inset: false,
                },
                BoxShadow {
                    color: theme::hexa(0, 0.1).into(),
                    offset: point(px(0.), px(4.)),
                    blur_radius: px(6.),
                    spread_radius: px(-4.),
                    inset: false,
                },
            ])
            .child(icon("search", 14.).text_color(slate::S500))
            .child(div().w(px(160.)).h(px(16.)).child(self.input.clone()).auto_id("searchbar.input", "textbox", "Search..."))
            .when(!self.logs_mode && !self.query.is_empty() && self.count == 0, |d| {
                d.child(
                    div()
                        .mx(px(4.))
                        .text_10()
                        .text_color(theme::hexa(0xf87171, 0.7))
                        .child("No results")
                        .auto_id("searchbar.count", "status", "No results"),
                )
            })
            .when(self.count > 0, |d| {
                d.child(
                    div()
                        .mx(px(4.))
                        .text_10()
                        .text_color(slate::S500)
                        .child(format!("{}/{}", self.current + 1, self.count))
                        .auto_id("searchbar.count", "status", format!("{}/{}", self.current + 1, self.count)),
                )
            })
            .child(btn("searchbar-prev", "chevron-up").on_click(cx.listener(|_, _, _, cx| cx.emit(SearchBarEvent::Prev))))
            .child(btn("searchbar-next", "chevron-down").on_click(cx.listener(|_, _, _, cx| cx.emit(SearchBarEvent::Next))))
            .child(
                btn("searchbar-close", "x")
                    .ml(px(4.))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SearchBarEvent::Close)))
                    .auto_id("searchbar.close", "button", "Close"),
            )
            .auto_id("searchbar", "search", "Search")
    }
}
