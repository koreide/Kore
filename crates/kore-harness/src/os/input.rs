//! Synthesized OS-level mouse and keyboard input via CGEvent.
//!
//! These are real HID events: the app under test must be frontmost, and the
//! harness takes over the user's pointer/keyboard while a scenario runs.
//!
//! Layer 3 only. Every event goes through [`source`], which refuses unless the
//! OS layer turned input on with [`enable`]. A Layer 2 run therefore cannot
//! touch the user's mouse or keyboard, even by mistake.

use anyhow::{anyhow, bail, Result};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton, EventField,
};
use core_graphics::sys::{CGEventRef, CGEventSourceRef};
use foreign_types::{ForeignType};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use std::thread::sleep;
use std::time::Duration;

static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static POSTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Allow real OS input (Layer 3 runs only).
pub fn enable() {
    ENABLED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Number of OS events posted by this process (a Layer 2 run asserts 0).
pub fn posted() -> u64 {
    POSTED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Fails unless OS input was enabled (also guards window activation/placement,
/// which steal focus).
pub fn require_enabled() -> Result<()> {
    if !ENABLED.load(std::sync::atomic::Ordering::SeqCst) {
        bail!("OS input is disabled: only Layer 3 (`--layer os`) may send real mouse/keyboard events or activate windows");
    }
    Ok(())
}

fn source() -> Result<CGEventSource> {
    require_enabled()?;
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow!("failed to create CGEventSource"))
}

fn post(ev: CGEvent) {
    POSTED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    ev.post(CGEventTapLocation::HID);
    // Give the app's run loop a moment; back-to-back events get coalesced otherwise.
    sleep(Duration::from_millis(12));
}

pub fn move_to(x: f64, y: f64) -> Result<()> {
    let ev = CGEvent::new_mouse_event(
        source()?,
        CGEventType::MouseMoved,
        CGPoint::new(x, y),
        CGMouseButton::Left,
    )
    .map_err(|_| anyhow!("mouse move event"))?;
    post(ev);
    Ok(())
}

pub fn click(x: f64, y: f64, count: i64) -> Result<()> {
    move_to(x, y)?;
    sleep(Duration::from_millis(40));
    for n in 1..=count {
        for kind in [CGEventType::LeftMouseDown, CGEventType::LeftMouseUp] {
            let ev = CGEvent::new_mouse_event(source()?, kind, CGPoint::new(x, y), CGMouseButton::Left)
                .map_err(|_| anyhow!("mouse button event"))?;
            ev.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, n);
            post(ev);
        }
    }
    Ok(())
}

pub fn right_click(x: f64, y: f64) -> Result<()> {
    move_to(x, y)?;
    for kind in [CGEventType::RightMouseDown, CGEventType::RightMouseUp] {
        let ev = CGEvent::new_mouse_event(source()?, kind, CGPoint::new(x, y), CGMouseButton::Right)
            .map_err(|_| anyhow!("mouse button event"))?;
        post(ev);
    }
    Ok(())
}

pub fn drag(from: (f64, f64), to: (f64, f64)) -> Result<()> {
    move_to(from.0, from.1)?;
    let down = CGEvent::new_mouse_event(
        source()?,
        CGEventType::LeftMouseDown,
        CGPoint::new(from.0, from.1),
        CGMouseButton::Left,
    )
    .map_err(|_| anyhow!("mouse down"))?;
    post(down);
    let steps = 12;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let p = CGPoint::new(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        let ev = CGEvent::new_mouse_event(source()?, CGEventType::LeftMouseDragged, p, CGMouseButton::Left)
            .map_err(|_| anyhow!("mouse drag"))?;
        post(ev);
    }
    let up = CGEvent::new_mouse_event(
        source()?,
        CGEventType::LeftMouseUp,
        CGPoint::new(to.0, to.1),
        CGMouseButton::Left,
    )
    .map_err(|_| anyhow!("mouse up"))?;
    post(up);
    Ok(())
}

/// Scrolls by `dy` pixels at the given point (positive = content moves up / scroll down).
pub fn scroll(x: f64, y: f64, dy: i32) -> Result<()> {
    move_to(x, y)?;
    // Deliver in chunks like a trackpad would, so apps see a stream of wheel events.
    let chunk = 40;
    let mut remaining = dy;
    while remaining != 0 {
        let step = remaining.signum() * remaining.abs().min(chunk);
        post(scroll_event(-step)?);
        remaining -= step;
    }
    Ok(())
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    // Non-variadic variant of CGEventCreateScrollWheelEvent (macOS 10.13+).
    fn CGEventCreateScrollWheelEvent2(
        source: CGEventSourceRef,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> CGEventRef;
}

const SCROLL_UNIT_PIXEL: u32 = 0;

fn scroll_event(dy: i32) -> Result<CGEvent> {
    let src = source()?;
    let raw = unsafe { CGEventCreateScrollWheelEvent2(src.as_ptr(), SCROLL_UNIT_PIXEL, 1, dy, 0, 0) };
    if raw.is_null() {
        bail!("CGEventCreateScrollWheelEvent2 returned null");
    }
    Ok(unsafe { CGEvent::from_ptr(raw) })
}

/// Presses a chord such as `cmd+k`, `shift+tab`, `escape`, `j`, `?`.
pub fn key(chord: &str) -> Result<()> {
    let mut flags = CGEventFlags::empty();
    let parts: Vec<&str> = chord.split('+').collect();
    let (mods, key) = parts.split_at(parts.len() - 1);
    for m in mods {
        flags |= match m.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "meta" => CGEventFlags::CGEventFlagCommand,
            "shift" => CGEventFlags::CGEventFlagShift,
            "alt" | "option" | "opt" => CGEventFlags::CGEventFlagAlternate,
            "ctrl" | "control" => CGEventFlags::CGEventFlagControl,
            other => bail!("unknown modifier {other:?} in {chord:?}"),
        };
    }
    let key = key[0];
    let (code, needs_shift) = keycode(key).ok_or_else(|| anyhow!("unknown key {key:?}"))?;
    if needs_shift {
        flags |= CGEventFlags::CGEventFlagShift;
    }
    for down in [true, false] {
        let ev = CGEvent::new_keyboard_event(source()?, code, down)
            .map_err(|_| anyhow!("keyboard event"))?;
        ev.set_flags(flags);
        post(ev);
    }
    if !flags.is_empty() {
        release_modifiers()?;
    }
    Ok(())
}

/// Posts key-up events for Command/Shift/Option/Control with no flags. Posting a chord's
/// flags through the HID tap can leave the system-wide modifier state latched (observed:
/// `NSEvent.modifierFlags` stuck on Command), which turns later typed text into shortcuts.
pub fn release_modifiers() -> Result<()> {
    for code in [0x37u16, 0x38, 0x3A, 0x3B] {
        let ev = CGEvent::new_keyboard_event(source()?, code, false)
            .map_err(|_| anyhow!("keyboard event"))?;
        ev.set_flags(CGEventFlags::empty());
        post(ev);
    }
    Ok(())
}

/// Types literal text (unicode-safe, independent of keyboard layout).
pub fn type_text(text: &str) -> Result<()> {
    release_modifiers()?;
    for ch in text.chars() {
        if ch == '\n' {
            key("enter")?;
            continue;
        }
        for down in [true, false] {
            let ev = CGEvent::new_keyboard_event(source()?, 0, down)
                .map_err(|_| anyhow!("keyboard event"))?;
            ev.set_string(&ch.to_string());
            // Without explicit flags the event inherits modifier state from a preceding chord
            // (e.g. after `cmd+k`), and keycode 0 is then delivered as cmd+a (select all).
            ev.set_flags(CGEventFlags::empty());
            post(ev);
        }
    }
    Ok(())
}

/// US ANSI virtual key codes. Returns (code, requires_shift).
fn keycode(name: &str) -> Option<(CGKeyCode, bool)> {
    let named = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(0x24),
        "tab" => Some(0x30),
        "space" => Some(0x31),
        "backspace" | "delete" => Some(0x33),
        "escape" | "esc" => Some(0x35),
        "forwarddelete" => Some(0x75),
        "home" => Some(0x73),
        "end" => Some(0x77),
        "pageup" => Some(0x74),
        "pagedown" => Some(0x79),
        "left" => Some(0x7B),
        "right" => Some(0x7C),
        "down" => Some(0x7D),
        "up" => Some(0x7E),
        "f1" => Some(0x7A),
        _ => None,
    };
    if let Some(c) = named {
        return Some((c, false));
    }
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let shifted = |base: char| -> Option<(CGKeyCode, bool)> { keycode(&base.to_string()).map(|(k, _)| (k, true)) };
    let code = match c.to_ascii_lowercase() {
        'a' => 0x00, 's' => 0x01, 'd' => 0x02, 'f' => 0x03, 'h' => 0x04, 'g' => 0x05,
        'z' => 0x06, 'x' => 0x07, 'c' => 0x08, 'v' => 0x09, 'b' => 0x0B, 'q' => 0x0C,
        'w' => 0x0D, 'e' => 0x0E, 'r' => 0x0F, 'y' => 0x10, 't' => 0x11, '1' => 0x12,
        '2' => 0x13, '3' => 0x14, '4' => 0x15, '6' => 0x16, '5' => 0x17, '=' => 0x18,
        '9' => 0x19, '7' => 0x1A, '-' => 0x1B, '8' => 0x1C, '0' => 0x1D, ']' => 0x1E,
        'o' => 0x1F, 'u' => 0x20, '[' => 0x21, 'i' => 0x22, 'p' => 0x23, 'l' => 0x25,
        'j' => 0x26, '\'' => 0x27, 'k' => 0x28, ';' => 0x29, '\\' => 0x2A, ',' => 0x2B,
        '/' => 0x2C, 'n' => 0x2D, 'm' => 0x2E, '.' => 0x2F, '`' => 0x32,
        '?' => return shifted('/'),
        '!' => return shifted('1'),
        ':' => return shifted(';'),
        _ => return None,
    };
    Some((code, c.is_ascii_uppercase()))
}

/// Leave the user's input devices in a clean state: modifier key-ups (all four,
/// left and right) and left/right mouse-ups at the current pointer position.
/// Safe to call any number of times; a no-op unless OS input was enabled.
pub fn release_all() {
    if require_enabled().is_err() {
        return;
    }
    let _ = release_modifiers();
    let Ok(src) = source() else { return };
    let at = CGEvent::new(src).map(|e| e.location()).unwrap_or(CGPoint::new(0.0, 0.0));
    for (kind, button) in [(CGEventType::LeftMouseUp, CGMouseButton::Left), (CGEventType::RightMouseUp, CGMouseButton::Right)] {
        if let Ok(src) = source() {
            if let Ok(ev) = CGEvent::new_mouse_event(src, kind, at, button) {
                post(ev);
            }
        }
    }
    // Right-hand modifiers (right ⌘ ⇧ ⌥ ⌃) too, in case a chord used them.
    for code in [0x36u16, 0x3C, 0x3D, 0x3E] {
        if let Ok(src) = source() {
            if let Ok(ev) = CGEvent::new_keyboard_event(src, code, false) {
                ev.set_flags(CGEventFlags::empty());
                post(ev);
            }
        }
    }
}
