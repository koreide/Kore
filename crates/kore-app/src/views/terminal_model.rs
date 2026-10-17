//! VT emulation for the exec terminal: an `alacritty_terminal::Term` plus the
//! terminal configuration (theme, 13px, scrollback 1000), key → byte mapping
//! and grid sizing (fit to the view).

use crate::theme;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, Rgb};
use alacritty_terminal::Term;
use gpui_kit::Rgba;
use std::cell::RefCell;
use std::rc::Rc;

/// Terminal font size.
pub const FONT_SIZE: f32 = 13.;
/// Cell size for SF Mono 13px: rows 15px apart, 0.6em advance.
pub const CELL_W: f32 = 7.8;
pub const CELL_H: f32 = 15.;
/// Scrollback lines.
pub const SCROLLBACK: usize = 1000;

pub const BACKGROUND: u32 = 0x0b1221;
pub const FOREGROUND: u32 = 0xe2e8f0;
/// ANSI 0–15 of Kore's terminal theme.
pub const PALETTE: [u32; 16] = [
    0x1e293b, 0xf87171, 0x4ade80, 0xfbbf24, 0x60a5fa, 0xc084fc, 0x22d3ee, 0xe2e8f0, // normal
    0x475569, 0xfca5a5, 0x86efac, 0xfde68a, 0x93c5fd, 0xd8b4fe, 0x67e8f9, 0xf8fafc, // bright
];

/// 256-color palette entry for `i` (theme for 0–15, cube, grays).
pub fn indexed_rgb(i: u8) -> u32 {
    match i {
        0..=15 => PALETTE[i as usize],
        16..=231 => {
            let i = i - 16;
            let v = |c: u8| if c == 0 { 0 } else { 55 + c as u32 * 40 };
            (v(i / 36) << 16) | (v((i / 6) % 6) << 8) | v(i % 6)
        }
        _ => {
            let g = 8 + (i as u32 - 232) * 10;
            (g << 16) | (g << 8) | g
        }
    }
}

fn rgb(c: Rgb) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// Resolve a cell color. `bold` brightens ANSI 0–7 foregrounds
/// (bold text in bright colors).
pub fn resolve_color(c: Color, colors: &Colors, bold: bool, is_fg: bool) -> u32 {
    let idx = |i: usize, default: u32| colors[i].map(rgb).unwrap_or(default);
    match c {
        Color::Spec(r) => rgb(r),
        Color::Indexed(i) => {
            let i = if bold && is_fg && i < 8 { i + 8 } else { i };
            idx(i as usize, indexed_rgb(i))
        }
        Color::Named(n) => {
            let n_us = n as usize;
            match n {
                NamedColor::Foreground | NamedColor::BrightForeground => idx(n_us, FOREGROUND),
                NamedColor::Background => idx(n_us, BACKGROUND),
                NamedColor::DimForeground => idx(NamedColor::Foreground as usize, FOREGROUND),
                NamedColor::Cursor => idx(n_us, theme::DEFAULT_ACCENT),
                _ if n_us < 16 => {
                    let i = if bold && is_fg && n_us < 8 { n_us + 8 } else { n_us };
                    idx(i, PALETTE[i])
                }
                _ => {
                    // Dim colors map to their normal counterparts.
                    let base = n_us - NamedColor::DimBlack as usize;
                    idx(base.min(7), PALETTE[base.min(7)])
                }
            }
        }
    }
}

/// FitAddon: whole cells that fit the host's content box (min 2×1).
pub fn grid_size(width: f32, height: f32) -> (u16, u16) {
    let cols = (width / CELL_W).floor().max(2.) as u16;
    let rows = (height / CELL_H).floor().max(1.) as u16;
    (cols, rows)
}

/// Bytes sent to the PTY for a key press. `None` = not handled by the terminal.
#[allow(clippy::too_many_arguments)]
pub fn key_bytes(key: &str, key_char: Option<&str>, ctrl: bool, alt: bool, shift: bool, cmd: bool, app_cursor: bool) -> Option<Vec<u8>> {
    if cmd {
        return None;
    }
    let csi = |s: &str| Some(format!("\x1b{}{s}", if app_cursor { "O" } else { "[" }).into_bytes());
    let mods = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
    let arrow = |c: char| {
        if mods > 1 {
            Some(format!("\x1b[1;{mods}{c}").into_bytes())
        } else {
            csi(&c.to_string())
        }
    };
    match key {
        "enter" => return Some(if alt { b"\x1b\r".to_vec() } else { b"\r".to_vec() }),
        "backspace" => {
            return Some(match (ctrl, alt) {
                (true, _) => b"\x08".to_vec(),
                (_, true) => b"\x1b\x7f".to_vec(),
                _ => b"\x7f".to_vec(),
            })
        }
        "tab" => return Some(if shift { b"\x1b[Z".to_vec() } else { b"\t".to_vec() }),
        "escape" => return Some(b"\x1b".to_vec()),
        "up" => return arrow('A'),
        "down" => return arrow('B'),
        "right" => return arrow('C'),
        "left" => return arrow('D'),
        "home" => return arrow('H'),
        "end" => return arrow('F'),
        "delete" => return Some(b"\x1b[3~".to_vec()),
        "pageup" => return Some(b"\x1b[5~".to_vec()),
        "pagedown" => return Some(b"\x1b[6~".to_vec()),
        "insert" => return Some(b"\x1b[2~".to_vec()),
        "space" if ctrl => return Some(vec![0]),
        _ => {}
    }
    if let Some(f) = key.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        let s = match f {
            1 => "\x1bOP",
            2 => "\x1bOQ",
            3 => "\x1bOR",
            4 => "\x1bOS",
            5 => "\x1b[15~",
            6 => "\x1b[17~",
            7 => "\x1b[18~",
            8 => "\x1b[19~",
            9 => "\x1b[20~",
            10 => "\x1b[21~",
            11 => "\x1b[23~",
            12 => "\x1b[24~",
            _ => return None,
        };
        return Some(s.as_bytes().to_vec());
    }
    if ctrl && !alt {
        let k = key.as_bytes();
        if k.len() == 1 {
            let c = k[0].to_ascii_lowercase();
            return match c {
                b'a'..=b'z' => Some(vec![c - b'a' + 1]),
                b'[' | b'3' => Some(vec![0x1b]),
                b'\\' | b'4' => Some(vec![0x1c]),
                b']' | b'5' => Some(vec![0x1d]),
                b'6' => Some(vec![0x1e]),
                b'/' | b'7' | b'-' => Some(vec![0x1f]),
                b'2' | b'@' => Some(vec![0]),
                b'8' => Some(vec![0x7f]),
                _ => None,
            };
        }
    }
    let ch = key_char.filter(|c| !c.is_empty())?;
    if ctrl {
        return None;
    }
    Some(ch.as_bytes().to_vec())
}

/// Printable text keys arrive through the platform text-input path
/// (`EntityInputHandler::replace_text_in_range`), not as keystrokes: the
/// key-down handler must leave them alone.
pub fn is_text_key(key: &str, key_char: Option<&str>, ctrl: bool, cmd: bool) -> bool {
    if ctrl || cmd {
        return false;
    }
    const NAMED: [&str; 15] =
        ["enter", "backspace", "tab", "escape", "up", "down", "left", "right", "home", "end", "delete", "pageup", "pagedown", "insert", "space"];
    if NAMED.contains(&key) && key != "space" {
        return false;
    }
    if key.len() > 1 && key.starts_with('f') && key[1..].parse::<u8>().is_ok() {
        return false;
    }
    key_char.is_some_and(|c| !c.is_empty() && c.chars().all(|ch| !ch.is_control()))
}

/// Pasted text (bracketed when the shell asked for it).
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let t = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{t}\x1b[201~").into_bytes()
    } else {
        t.into_bytes()
    }
}

#[derive(Clone, Default)]
pub struct Listener {
    writes: Rc<RefCell<Vec<String>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(s) = event {
            self.writes.borrow_mut().push(s);
        }
    }
}

/// One terminal instance.
pub struct TermModel {
    pub term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    pub cols: u16,
    pub rows: u16,
}

impl TermModel {
    pub fn new(cols: u16, rows: u16) -> Self {
        let listener = Listener::default();
        let config = Config { scrolling_history: SCROLLBACK, ..Default::default() };
        let term = Term::new(config, &TermSize::new(cols as usize, rows as usize), listener.clone());
        TermModel { term, parser: Processor::new(), listener, cols, rows }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> bool {
        if (cols, rows) == (self.cols, self.rows) {
            return false;
        }
        self.cols = cols;
        self.rows = rows;
        self.term.resize(TermSize::new(cols as usize, rows as usize));
        true
    }

    /// Replies the emulator wants written back to the pty (DSR/DA…).
    pub fn take_writes(&self) -> Vec<String> {
        std::mem::take(&mut *self.listener.writes.borrow_mut())
    }

    pub fn app_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// Viewport cell → grid point (accounts for scrollback offset).
    pub fn grid_point(&self, row: usize, col: usize) -> GridPoint {
        let off = self.term.grid().display_offset() as i32;
        GridPoint::new(Line(row as i32 - off), Column(col.min(self.cols.saturating_sub(1) as usize)))
    }

    pub fn start_selection(&mut self, row: usize, col: usize, side_right: bool, kind: SelectionType) {
        let p = self.grid_point(row, col);
        self.term.selection = Some(Selection::new(kind, p, if side_right { Side::Right } else { Side::Left }));
    }

    pub fn update_selection(&mut self, row: usize, col: usize, side_right: bool) {
        let p = self.grid_point(row, col);
        if let Some(sel) = self.term.selection.as_mut() {
            sel.update(p, if side_right { Side::Right } else { Side::Left });
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|s| !s.is_empty())
    }

    /// Visible text of a viewport row (for link detection and tests).
    pub fn row_text(&self, row: usize) -> String {
        let off = self.term.grid().display_offset() as i32;
        let line = Line(row as i32 - off);
        let grid = self.term.grid();
        let mut s = String::new();
        for c in 0..self.cols as usize {
            let cell = &grid[line][Column(c)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            s.push(cell.c);
        }
        s.trim_end().to_string()
    }
}

/// The URL under column `col` of `line` (for click-to-open links), if any.
pub fn url_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let text: String = chars.iter().collect();
    for scheme in ["https://", "http://"] {
        let mut from = 0;
        while let Some(i) = text[from..].find(scheme) {
            let start_b = from + i;
            let start = text[..start_b].chars().count();
            let mut end = start;
            while end < chars.len() && !chars[end].is_whitespace() && !"\"'<>`".contains(chars[end]) {
                end += 1;
            }
            // Trim trailing punctuation.
            while end > start && ".,;:!?)]}".contains(chars[end - 1]) {
                end -= 1;
            }
            if col >= start && col < end {
                return Some(chars[start..end].iter().collect());
            }
            from = start_b + scheme.len();
        }
    }
    None
}

/// Rgba of a packed color.
pub fn rgba(c: u32) -> Rgba {
    theme::hex(c)
}

#[cfg(test)]
mod tests {
    use super::{grid_size, indexed_rgb, key_bytes, paste_bytes, url_at, TermModel, CELL_H, CELL_W};

    #[test]
    fn sizing() {
        assert_eq!(grid_size(CELL_W * 100. + 3., CELL_H * 40. + 14.), (100, 40));
        assert_eq!(grid_size(1., 1.), (2, 1));
        // A ~1100×650 Shell tab gives the dozens-of-rows / 100+-columns `stty size`.
        let (c, r) = grid_size(1100., 650.);
        assert!(c >= 100 && (10..100).contains(&r));
    }

    #[test]
    fn keys() {
        assert_eq!(key_bytes("enter", None, false, false, false, false, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes("backspace", None, false, false, false, false, false), Some(vec![0x7f]));
        assert_eq!(key_bytes("up", None, false, false, false, false, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes("up", None, false, false, false, false, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes("c", Some("c"), true, false, false, false, false), Some(vec![3]));
        assert_eq!(key_bytes("d", Some("d"), true, false, false, false, false), Some(vec![4]));
        assert_eq!(key_bytes("4", Some("$"), false, false, true, false, false), Some(b"$".to_vec()));
        assert_eq!(key_bytes("v", Some("v"), false, false, false, true, false), None);
        assert_eq!(key_bytes("left", None, false, false, true, false, false), Some(b"\x1b[1;2D".to_vec()));
        assert_eq!(key_bytes("shift", None, false, false, true, false, false), None);
        assert!(super::is_text_key("a", Some("a"), false, false));
        assert!(super::is_text_key("space", Some(" "), false, false));
        assert!(!super::is_text_key("enter", Some("\n"), false, false));
        assert!(!super::is_text_key("c", Some("c"), true, false));
        assert!(!super::is_text_key("f5", None, false, false));
        assert_eq!(paste_bytes("a\nb", true), b"\x1b[200~a\rb\x1b[201~".to_vec());
    }

    #[test]
    fn colors() {
        assert_eq!(indexed_rgb(1), 0xf87171);
        assert_eq!(indexed_rgb(16), 0x000000);
        assert_eq!(indexed_rgb(196), 0xff0000);
        assert_eq!(indexed_rgb(232), 0x080808);
        assert_eq!(indexed_rgb(255), 0xeeeeee);
    }

    #[test]
    fn vt_handling() {
        let mut t = TermModel::new(20, 4);
        t.feed(b"/ # echo hi\r\nhi\r\n\x1b[33mSession ended.\x1b[0m\r\n");
        assert_eq!(t.row_text(0), "/ # echo hi");
        assert_eq!(t.row_text(1), "hi");
        assert_eq!(t.row_text(2), "Session ended.");
        // Cursor movement and erase.
        t.feed(b"\x1b[1;1H\x1b[2Kreplaced");
        assert_eq!(t.row_text(0), "replaced");
        // Device status report produces a reply for the pty.
        t.feed(b"\x1b[6n");
        assert_eq!(t.take_writes(), vec!["\x1b[1;9R".to_string()]);
        assert!(t.resize(30, 6));
        assert!(!t.resize(30, 6));
        assert_eq!(t.cols, 30);
    }

    #[test]
    fn links() {
        let l = "see https://example.com/a?b=1. ok";
        assert_eq!(url_at(l, 6).as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(url_at(l, 0), None);
    }
}
