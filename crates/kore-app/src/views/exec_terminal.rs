//! Pod Shell tab: exec terminal (spec 02 §8). VT emulation is
//! `alacritty_terminal` (see `terminal_model.rs`); the grid is painted here
//! with Kore's terminal theme. Fix 7: the PTY is resized to the rendered
//! rows × columns.

use crate::automation::Automation;
use crate::backend;
use crate::keys;
use crate::state::resource_item::ResourceItem;
use crate::state::AppState;
use crate::theme::{self, amber, emerald, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::widgets::spinning_icon;
use crate::views::terminal_model::{self as tm, TermModel, CELL_H, CELL_W, FONT_SIZE};
use alacritty_terminal::index::{Column, Line, Point as GridPoint};
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;
use futures::future::{select, Either};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Status {
    Connecting,
    Connected,
    Error,
    Closed,
}

const SHELLS: [&str; 2] = ["/bin/sh", "/bin/bash"];
/// Host padding (`p-1`).
const PAD: f32 = 4.;
const BLINK: Duration = Duration::from_millis(600);

pub struct ExecTerminal {
    #[allow(dead_code)]
    app: Entity<AppState>,
    item: ResourceItem,
    namespace: String,
    container: Option<String>,
    shell: &'static str,
    status: Status,
    error: Option<String>,
    model: Rc<RefCell<TermModel>>,
    session_id: Rc<RefCell<Option<String>>>,
    input_tx: Option<async_channel::Sender<Vec<u8>>>,
    session_task: Option<Task<()>>,
    writer_task: Option<Task<()>>,
    blink_task: Option<Task<()>>,
    blink_on: Rc<Cell<bool>>,
    focus: FocusHandle,
    grid_origin: Rc<Cell<gpui_kit::Point<Pixels>>>,
    laid_out: Rc<Cell<bool>>,
    drag: Option<(usize, usize)>,
    dragged: bool,
    _subs: Vec<Subscription>,
}

impl ExecTerminal {
    pub fn new(app: Entity<AppState>, item: ResourceItem, container: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let namespace = if item.namespace.is_empty() { "default".to_string() } else { item.namespace.clone() };
        let focus = cx.focus_handle();
        keys::register_text_entry(&focus);
        let subs = vec![
            cx.on_focus(&focus, window, |this, _, cx| {
                this.start_blink(cx);
                cx.notify();
            }),
            cx.on_blur(&focus, window, |this, _, cx| {
                this.blink_task = None;
                this.blink_on.set(true);
                cx.notify();
            }),
        ];
        let mut this = ExecTerminal {
            app,
            item,
            namespace,
            container,
            shell: SHELLS[0],
            status: Status::Connecting,
            error: None,
            model: Rc::new(RefCell::new(TermModel::new(80, 24))),
            session_id: Rc::default(),
            input_tx: None,
            session_task: None,
            writer_task: None,
            blink_task: None,
            blink_on: Rc::new(Cell::new(true)),
            focus,
            grid_origin: Rc::default(),
            laid_out: Rc::default(),
            drag: None,
            dragged: false,
            _subs: subs,
        };
        this.start_session(cx);
        this
    }

    fn start_session(&mut self, cx: &mut Context<Self>) {
        // Old session: unlisten + stop_exec + dispose (fresh, cleared terminal).
        self.session_task = None;
        self.writer_task = None;
        self.input_tx = None;
        if let Some(id) = self.session_id.borrow_mut().take() {
            let core = backend::core(cx);
            cx.background_executor().spawn(async move { core.stop_exec(id).await.ok() }).detach();
        }
        let (cols, rows) = {
            let m = self.model.borrow();
            (m.cols, m.rows)
        };
        *self.model.borrow_mut() = TermModel::new(cols, rows);

        let (tx, rx) = async_channel::unbounded::<Vec<u8>>();
        self.input_tx = Some(tx);
        let core = backend::core(cx);
        let sid = self.session_id.clone();
        self.writer_task = Some(cx.spawn(async move |_, _| {
            while let Ok(mut bytes) = rx.recv().await {
                while let Ok(more) = rx.try_recv() {
                    bytes.extend(more);
                }
                let id = sid.borrow().clone();
                if let Some(id) = id {
                    let _ = core.send_exec_bytes(id, bytes).await;
                }
            }
        }));

        let core = backend::core(cx);
        let (ns, pod, container, shell) = (self.namespace.clone(), self.item.name.clone(), self.container.clone(), self.shell.to_string());
        self.session_task = Some(cx.spawn(async move |this, cx| {
            let res = core.exec_into_pod(ns, pod, container, Some(shell)).await;
            let session = match res {
                Ok(s) => s,
                Err(e) => {
                    this.update(cx, |t, cx| {
                        t.status = Status::Error;
                        t.error = Some(backend::format_error(&e));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let kore_core::ExecSession { session_id, mut stdout, exit } = session;
            let ok = this.update(cx, |t, cx| {
                *t.session_id.borrow_mut() = Some(session_id.clone());
                t.status = Status::Connected;
                t.send_resize(cx);
                cx.notify();
            });
            if ok.is_err() {
                return;
            }
            let mut exit = exit;
            loop {
                let ev = match select(Box::pin(stdout.recv()), &mut exit).await {
                    Either::Left((v, _)) => Either::Left(v),
                    Either::Right((ex, _)) => Either::Right(ex.ok()),
                };
                match ev {
                    Either::Left(Some(first)) => {
                        let mut bytes = first;
                        for more in stdout.try_recv_batch(256) {
                            bytes.extend(more);
                        }
                        if this.update(cx, |t, cx| t.output(&bytes, cx)).is_err() {
                            return;
                        }
                    }
                    Either::Left(None) => {
                        // Output ended; wait for the exit payload.
                        let ex = exit.await.ok();
                        this.update(cx, |t, cx| t.exited(ex, cx)).ok();
                        return;
                    }
                    Either::Right(ex) => {
                        let rest: Vec<u8> = stdout.try_recv_batch(4096).into_iter().flatten().collect();
                        this.update(cx, |t, cx| {
                            if !rest.is_empty() {
                                t.output(&rest, cx);
                            }
                            t.exited(ex, cx);
                        })
                        .ok();
                        return;
                    }
                }
            }
        }));
        cx.notify();
    }

    fn output(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        let writes = {
            let mut m = self.model.borrow_mut();
            m.feed(bytes);
            m.take_writes()
        };
        for w in writes {
            self.send(w.into_bytes());
        }
        cx.notify();
    }

    fn exited(&mut self, ex: Option<kore_core::ExecExit>, cx: &mut Context<Self>) {
        match ex.and_then(|e| e.error) {
            Some(err) => {
                self.status = Status::Error;
                self.model.borrow_mut().feed(format!("\r\n\x1b[31m{err}\x1b[0m\r\n").as_bytes());
                self.error = Some(err);
            }
            None => {
                self.status = Status::Closed;
                self.model.borrow_mut().feed(b"\r\n\x1b[33mSession ended.\x1b[0m\r\n");
            }
        }
        cx.notify();
    }

    fn send(&self, bytes: Vec<u8>) {
        if self.session_id.borrow().is_none() {
            return;
        }
        if let Some(tx) = &self.input_tx {
            let _ = tx.try_send(bytes);
        }
    }

    /// Fix 7: tell the backend the rendered grid size.
    fn send_resize(&self, cx: &mut Context<Self>) {
        if !self.laid_out.get() {
            return;
        }
        let Some(id) = self.session_id.borrow().clone() else { return };
        let (cols, rows) = {
            let m = self.model.borrow();
            (m.cols, m.rows)
        };
        let core = backend::core(cx);
        cx.background_executor().spawn(async move { core.resize_exec(id, cols, rows).await.ok() }).detach();
    }

    fn set_shell(&mut self, shell: &'static str, cx: &mut Context<Self>) {
        if self.shell == shell {
            return;
        }
        self.shell = shell;
        self.start_session(cx);
    }

    fn start_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_on.set(true);
        let on = self.blink_on.clone();
        self.blink_task = Some(cx.spawn(async move |this, cx| loop {
            let t = cx.update(|cx| backend::sleep(cx, BLINK));
            t.await;
            on.set(!on.get());
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }));
    }

    fn cell_at(&self, pos: gpui_kit::Point<Pixels>) -> (usize, usize, bool) {
        let o = self.grid_origin.get();
        let m = self.model.borrow();
        let x = (f32::from(pos.x - o.x) / CELL_W).max(0.);
        let y = (f32::from(pos.y - o.y) / CELL_H).max(0.);
        let col = (x.floor() as usize).min(m.cols.saturating_sub(1) as usize);
        let row = (y.floor() as usize).min(m.rows.saturating_sub(1) as usize);
        (row, col, x.fract() > 0.5)
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = &ks.modifiers;
        if m.platform && ks.key == "c" {
            if let Some(t) = self.model.borrow().selection_text() {
                cx.write_to_clipboard(ClipboardItem::new_string(t));
            }
            cx.stop_propagation();
            return;
        }
        if m.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                let b = tm::paste_bytes(&text, self.model.borrow().bracketed_paste());
                self.send(b);
            }
            cx.stop_propagation();
            return;
        }
        if tm::is_text_key(&ks.key, ks.key_char.as_deref(), m.control, m.platform) {
            // Delivered via `replace_text_in_range` (IME path).
            return;
        }
        let app_cursor = self.model.borrow().app_cursor();
        if let Some(bytes) = tm::key_bytes(&ks.key, ks.key_char.as_deref(), m.control, m.alt, m.shift, m.platform, app_cursor) {
            {
                let mut mm = self.model.borrow_mut();
                mm.clear_selection();
                mm.scroll_to_bottom();
            }
            self.send(bytes);
            self.blink_on.set(true);
            if self.focus.is_focused(_window) {
                self.start_blink(cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// Text typed into the terminal (printable input).
    fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        {
            let mut mm = self.model.borrow_mut();
            mm.clear_selection();
            mm.scroll_to_bottom();
        }
        self.send(text.as_bytes().to_vec());
        self.blink_on.set(true);
        cx.notify();
    }

    fn render_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let status: AnyElement = match self.status {
            Status::Connecting => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .text_color(amber::A400)
                .child(spinning_icon("exec-connecting", "loader-2", 12., amber::A400))
                .child("Connecting...")
                .into_any_element(),
            Status::Connected => div().text_color(emerald::E400).child("Connected").into_any_element(),
            Status::Error => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .text_color(red::R400)
                .child(icon("alert-circle", 12.).text_color(red::R400))
                .child(self.error.clone().unwrap_or_else(|| "Error".into()))
                .into_any_element(),
            Status::Closed => div().text_color(slate::S400).child("Session ended").into_any_element(),
        };
        let label = match self.status {
            Status::Connecting => "Connecting...".to_string(),
            Status::Connected => "Connected".into(),
            Status::Error => self.error.clone().unwrap_or_else(|| "Error".into()),
            Status::Closed => "Session ended".into(),
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(8.))
            .border_b_1()
            .border_color(theme::hexa(0x1e293b, 0.5))
            .bg(theme::hexa(0x101828, 0.3))
            .child(div().text_10().text_color(slate::S500).child("SHELL:"))
            .children(SHELLS.iter().map(|s| {
                let sel = self.shell == *s;
                let name = s.rsplit('/').next().unwrap_or(s);
                let shell: &'static str = s;
                div()
                    .id(SharedString::from(format!("exec-shell-{name}")))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .border_1()
                    .text_xs_tw()
                    .cursor_pointer()
                    // bg-accent/10 + border-accent/30 render as no tint + preflight gray.
                    .when(sel, |d| d.text_color(accent).border_color(theme::PREFLIGHT_BORDER))
                    .when(!sel, |d| d.text_color(slate::S400).border_color(slate::S800).hover(|s| s.text_color(slate::S200)))
                    .child(name.to_string())
                    .on_click(cx.listener(move |this, _, _, cx| this.set_shell(shell, cx)))
                    .auto_id(format!("exec.shell.{name}"), "button", name.to_string())
                    .selected(sel)
            }))
            .child(div().ml_auto().text_xs_tw().child(status).auto_id("exec.status", "status", label))
    }

    fn render_grid(&mut self, focused: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.clone();
        let model_p = self.model.clone();
        let origin = self.grid_origin.clone();
        let laid_out = self.laid_out.clone();
        let this = cx.entity().downgrade();
        let input_entity = this.clone();
        let accent = theme::accent(cx);
        let blink_on = self.blink_on.get();
        canvas(
            move |bounds, _window, cx| {
                origin.set(bounds.origin);
                let (cols, rows) = tm::grid_size(f32::from(bounds.size.width), f32::from(bounds.size.height));
                let changed = model.borrow_mut().resize(cols, rows);
                let first = !laid_out.get();
                laid_out.set(true);
                if changed || first {
                    let this = this.clone();
                    cx.defer(move |cx| {
                        this.update(cx, |t, cx| t.send_resize(cx)).ok();
                    });
                }
            },
            move |bounds, _, window, cx| {
                if let Some(entity) = input_entity.upgrade() {
                    let fh = entity.read(cx).focus.clone();
                    window.handle_input(&fh, ElementInputHandler::new(bounds, entity), cx);
                }
                paint_grid(bounds.origin, &model_p.borrow(), accent, focused, blink_on, window)
            },
        )
        .size_full()
    }
}

/// (char, flags, fg, bg, selected)
type GridCell = (char, Flags, u32, u32, bool);

struct Seg {
    col: usize,
    text: String,
    fg: u32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

#[allow(clippy::too_many_arguments)]
fn paint_grid(
    o: gpui_kit::Point<Pixels>,
    m: &TermModel,
    accent: Rgba,
    focused: bool,
    blink_on: bool,
    window: &mut Window,
) {
    let content = m.term.renderable_content();
    let colors = content.colors;
    let off = content.display_offset as i32;
    let rows = m.rows as usize;
    let cols = m.cols as usize;
    let sel = content.selection;
    let cursor = content.cursor;
    let mut grid: Vec<Vec<GridCell>> = vec![Vec::with_capacity(cols); rows];
    for ind in content.display_iter {
        let r = ind.point.line.0 + off;
        if r < 0 || r as usize >= rows {
            continue;
        }
        let cell = ind.cell;
        let bold = cell.flags.contains(Flags::BOLD);
        let mut fg = tm::resolve_color(cell.fg, colors, bold, true);
        let mut bg = tm::resolve_color(cell.bg, colors, false, false);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        let selected = sel.map(|s| s.contains(ind.point)).unwrap_or(false);
        grid[r as usize].push((cell.c, cell.flags, fg, bg, selected));
    }
    let font_for = |bold: bool, italic: bool| Font {
        family: theme::MONO.into(),
        weight: if bold { FontWeight::BOLD } else { FontWeight::NORMAL },
        style: if italic { FontStyle::Italic } else { FontStyle::Normal },
        ..font(theme::MONO)
    };
    let sel_bg = Rgba { a: 0x33 as f32 / 255., ..accent };
    let lh = px(CELL_H);
    for (r, row) in grid.iter().enumerate() {
        let y = o.y + px(r as f32 * CELL_H);
        // Backgrounds (merged runs) and selection.
        let mut c = 0;
        while c < row.len() {
            let bg = row[c].3;
            let s = row[c].4;
            let mut e = c + 1;
            while e < row.len() && row[e].3 == bg && row[e].4 == s {
                e += 1;
            }
            let rect = Bounds::new(point(o.x + px(c as f32 * CELL_W), y), size(px((e - c) as f32 * CELL_W), lh));
            if bg != tm::BACKGROUND {
                window.paint_quad(fill(rect, tm::rgba(bg)));
            }
            if s {
                window.paint_quad(fill(rect, sel_bg));
            }
            c = e;
        }
        // Text segments: contiguous non-blank cells of one style.
        let mut segs: Vec<Seg> = Vec::new();
        for (ci, (ch, flags, fg, _, _)) in row.iter().enumerate() {
            if flags.contains(Flags::WIDE_CHAR_SPACER) || flags.contains(Flags::HIDDEN) || *ch == ' ' || *ch == '\0' {
                continue;
            }
            let mut fg = *fg;
            if flags.contains(Flags::DIM) {
                // Dim text is drawn at 50% opacity: blend with the background.
                fg = blend(fg, tm::BACKGROUND, 0.5);
            }
            let (bold, italic) = (flags.contains(Flags::BOLD), flags.contains(Flags::ITALIC));
            let underline = flags.intersects(Flags::ALL_UNDERLINES);
            let strike = flags.contains(Flags::STRIKEOUT);
            match segs.last_mut() {
                Some(s)
                    if s.col + s.text.chars().count() == ci
                        && s.fg == fg
                        && s.bold == bold
                        && s.italic == italic
                        && s.underline == underline
                        && s.strike == strike =>
                {
                    s.text.push(*ch);
                }
                _ => segs.push(Seg { col: ci, text: ch.to_string(), fg, bold, italic, underline, strike }),
            }
        }
        for s in segs {
            let color: Hsla = tm::rgba(s.fg).into();
            let run = TextRun {
                len: s.text.len(),
                font: font_for(s.bold, s.italic),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            paint_cells(window, &s.text, run, point(o.x + px(s.col as f32 * CELL_W), y));
            if s.underline || s.strike {
                let w = px(s.text.chars().count() as f32 * CELL_W);
                let x0 = o.x + px(s.col as f32 * CELL_W);
                if s.underline {
                    window.paint_quad(fill(Bounds::new(point(x0, y + lh - px(2.)), size(w, px(1.))), color));
                }
                if s.strike {
                    window.paint_quad(fill(Bounds::new(point(x0, y + lh / 2.), size(w, px(1.))), color));
                }
            }
        }
    }
    // Cursor (block, accent; char drawn in the background color on top).
    let cr = cursor.point.line.0 + off;
    if cursor.shape != CursorShape::Hidden && cr >= 0 && (cr as usize) < rows {
        let cc = cursor.point.column.0.min(cols.saturating_sub(1));
        let rect = Bounds::new(
            point(o.x + px(cc as f32 * CELL_W), o.y + px(cr as f32 * CELL_H)),
            size(px(CELL_W), px(CELL_H)),
        );
        // Unfocused: no cursor is shown at all.
        if focused && blink_on {
            window.paint_quad(fill(rect, accent));
            let cell = &m.term.grid()[GridPoint::new(Line(cursor.point.line.0), Column(cc))];
            if cell.c != ' ' && cell.c != '\0' {
                let run = TextRun {
                    len: cell.c.len_utf8(),
                    font: font_for(cell.flags.contains(Flags::BOLD), cell.flags.contains(Flags::ITALIC)),
                    color: tm::rgba(tm::BACKGROUND).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                paint_cells(window, &cell.c.to_string(), run, rect.origin);
            }
        }
    }
}

/// Paint `text` with one glyph per cell (the terminal grid pitch), like
/// `ui::mono_text` does for the 0.601em SF Mono pitch.
fn paint_cells(window: &mut Window, text: &str, run: TextRun, origin: gpui_kit::Point<Pixels>) {
    let color = run.color;
    let size = px(FONT_SIZE);
    let line = window.text_system().layout_line(text, size, &[run], None);
    let baseline = origin.y + (px(CELL_H) - line.ascent - line.descent) / 2. + line.ascent;
    let mut char_ix = vec![0usize; text.len() + 1];
    for (ci, (bi, _)) in text.char_indices().enumerate() {
        char_ix[bi] = ci;
    }
    for r in &line.runs {
        for g in &r.glyphs {
            let ci = char_ix.get(g.index).copied().unwrap_or(0);
            let at = point(origin.x + px(ci as f32 * CELL_W), baseline);
            if g.is_emoji {
                window.paint_emoji(at, r.font_id, g.id, size).ok();
            } else {
                window.paint_glyph(at, r.font_id, g.id, size, color).ok();
            }
        }
    }
}

fn blend(fg: u32, bg: u32, a: f32) -> u32 {
    let ch = |s: u32| {
        let f = ((fg >> s) & 0xff) as f32;
        let b = ((bg >> s) & 0xff) as f32;
        ((f * a + b * (1. - a)).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

impl Render for ExecTerminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let bar = self.render_bar(cx);
        let grid = self.render_grid(focused, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(bar)
            .child(
                div()
                    .id("exec-host")
                    .flex_1()
                    .min_h_0()
                    .p(px(PAD))
                    .bg(tm::rgba(tm::BACKGROUND))
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .cursor(CursorStyle::IBeam)
                    .on_key_down(cx.listener(Self::on_key))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, e: &MouseDownEvent, window, cx| {
                            // Keep the root's "click outside a text input blurs it" from
                            // stealing focus back.
                            cx.stop_propagation();
                            window.focus(&this.focus, cx);
                            let (r, c, right) = this.cell_at(e.position);
                            let kind = match e.click_count {
                                2 => Some(SelectionType::Semantic),
                                3 => Some(SelectionType::Lines),
                                _ => None,
                            };
                            match kind {
                                Some(k) => {
                                    this.model.borrow_mut().start_selection(r, c, right, k);
                                    this.dragged = true;
                                }
                                None => {
                                    this.model.borrow_mut().clear_selection();
                                    this.dragged = false;
                                }
                            }
                            this.drag = Some((r, c));
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                        let Some(start) = this.drag else { return };
                        if e.pressed_button != Some(MouseButton::Left) {
                            this.drag = None;
                            return;
                        }
                        let (r, c, right) = this.cell_at(e.position);
                        if !this.dragged && (r, c) != start {
                            this.model.borrow_mut().start_selection(start.0, start.1, false, SelectionType::Simple);
                            this.dragged = true;
                        }
                        if this.dragged {
                            this.model.borrow_mut().update_selection(r, c, right);
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, e: &MouseUpEvent, _, cx| {
                            let was_drag = this.dragged;
                            this.drag = None;
                            if !was_drag {
                                // Clicking a URL opens it.
                                let (r, c, _) = this.cell_at(e.position);
                                let line = this.model.borrow().row_text(r);
                                if let Some(url) = tm::url_at(&line, c) {
                                    cx.open_url(&url);
                                }
                            }
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                        let dy = f32::from(e.delta.pixel_delta(px(CELL_H)).y);
                        let lines = (dy / CELL_H).round() as i32;
                        if lines != 0 {
                            this.model.borrow_mut().scroll(lines);
                            cx.notify();
                        }
                    }))
                    .child(grid)
                    .auto_id("exec.terminal", "textbox", "Terminal"),
            )
            .auto_id("view.exec-terminal", "region", "Shell")
    }
}

impl EntityInputHandler for ExecTerminal {
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
        self.type_text(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        _: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        // Composition (dead keys / IME) is committed through replace_text_in_range.
    }

    fn bounds_for_range(&mut self, _: std::ops::Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let o = self.grid_origin.get();
        let m = self.model.borrow();
        let c = m.term.grid().cursor.point;
        Some(Bounds::new(
            point(o.x + px(c.column.0 as f32 * CELL_W), o.y + px(c.line.0.max(0) as f32 * CELL_H)),
            size(px(CELL_W), px(CELL_H)),
        ))
    }

    fn character_index_for_point(&mut self, _: gpui_kit::Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Drop for ExecTerminal {
    fn drop(&mut self) {
        // Dropping the session task drops the ExecSession (ends the backend
        // session); the writer task ends with its channel.
        self.session_task = None;
        self.input_tx = None;
    }
}
