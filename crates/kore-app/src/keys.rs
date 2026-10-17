//! Keyboard focus bookkeeping shared by the global shortcut handler and views.
//!
//! Global shortcuts must not fire while the user is typing. Every text-entry
//! surface (our `TextInput`, the terminal, the YAML editor) registers its
//! `FocusHandle`, and [`text_entry_focused`] says whether one of them has focus.

use gpui_kit::*;
use std::cell::RefCell;

thread_local! {
    static ENTRIES: RefCell<Vec<WeakFocusHandle>> = const { RefCell::new(Vec::new()) };
}

/// Mark a focus handle as a text-entry surface (global single-key shortcuts
/// such as `/`, `?`, `j`, `k`, `h`, Esc are not handled while it is focused).
pub fn register_text_entry(handle: &FocusHandle) {
    ENTRIES.with(|e| {
        let mut e = e.borrow_mut();
        e.retain(|h| h.upgrade().is_some());
        e.push(handle.downgrade());
    });
}

/// True when a registered text-entry surface has focus.
pub fn text_entry_focused(window: &Window) -> bool {
    ENTRIES.with(|e| e.borrow().iter().any(|h| h.upgrade().map(|h| h.is_focused(window)).unwrap_or(false)))
}

/// Shortcut hint labels: macOS glyphs, `Ctrl`/`Shift` elsewhere (the bindings accept either).
#[cfg(target_os = "macos")]
mod labels {
    pub const MOD: &str = "⌘";
    pub const SHIFT: &str = "⇧";
    pub const MOD_K: &str = "⌘K";
    pub const MOD_R: &str = "⌘R";
    pub const MOD_F: &str = "⌘F";
}
#[cfg(not(target_os = "macos"))]
mod labels {
    pub const MOD: &str = "Ctrl";
    pub const SHIFT: &str = "Shift";
    pub const MOD_K: &str = "Ctrl+K";
    pub const MOD_R: &str = "Ctrl+R";
    pub const MOD_F: &str = "Ctrl+F";
}
pub use labels::*;

/// Normalised view of a keystroke for window-level handlers.
#[derive(Debug, Clone)]
pub struct Key {
    /// Lowercase key name (`"j"`, `"/"`, `"escape"`, `"up"`, `"enter"`, `"r"` …).
    pub key: String,
    /// The character produced, if any (`"?"` for shift-/).
    pub ch: Option<String>,
    pub cmd: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Key {
    pub fn from(ks: &Keystroke) -> Key {
        Key {
            key: ks.key.to_lowercase(),
            ch: ks.key_char.clone(),
            cmd: ks.modifiers.platform,
            ctrl: ks.modifiers.control,
            shift: ks.modifiers.shift,
            alt: ks.modifiers.alt,
        }
    }

    /// No command/control/alt modifier.
    pub fn plain(&self) -> bool {
        !self.cmd && !self.ctrl && !self.alt
    }

    /// `⌘X` on macOS or `Ctrl+X` elsewhere (either is accepted).
    pub fn mod_key(&self, k: &str) -> bool {
        (self.cmd || self.ctrl) && self.key == k
    }

    pub fn is_question(&self) -> bool {
        self.ch.as_deref() == Some("?") || self.key == "?" || (self.key == "/" && self.shift)
    }

    pub fn is_slash(&self) -> bool {
        self.plain() && !self.shift && (self.key == "/" || self.ch.as_deref() == Some("/"))
    }

    /// Plain letter/digit key without shift (`j`, `1`, …).
    pub fn is(&self, k: &str) -> bool {
        self.plain() && !self.shift && self.key == k
    }
}

/// Register a window-level key listener (the `window.addEventListener("keydown")`
/// equivalent). It runs **before** focused elements see the key; return `true`
/// to consume it (like `preventDefault` + stop). Keep the returned
/// `Subscription` alive for as long as the view wants keys (usually a field).
pub fn on_window_key<V: 'static>(
    cx: &mut Context<V>,
    f: impl Fn(&mut V, &Key, &mut Window, &mut Context<V>) -> bool + 'static,
) -> Subscription {
    let this = cx.entity().downgrade();
    cx.intercept_keystrokes(move |ev, window, cx| {
        let key = Key::from(&ev.keystroke);
        let handled = this.update(cx, |v, cx| f(v, &key, window, cx)).unwrap_or(false);
        if handled {
            cx.stop_propagation();
        }
    })
}
