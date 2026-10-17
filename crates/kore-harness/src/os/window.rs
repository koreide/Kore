//! Window discovery, placement and capture for an app identified by pid.

use accessibility_sys::*;
use anyhow::{anyhow, bail, Context, Result};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFRelease, CFType, CFTypeRef, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGSize};
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly,
};
use serde::Serialize;
use std::path::Path;
use std::process::Command;
use std::ptr;

/// Window frame in global screen points (origin top-left of main display).
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    pub id: u32,
    pub frame: Frame,
    pub title: String,
}

/// Returns the app's largest normal-layer on-screen window.
pub fn main_window(pid: i32) -> Result<WindowInfo> {
    let list = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )
    .ok_or_else(|| anyhow!("CGWindowListCopyWindowInfo returned null"))?;
    let mut best: Option<WindowInfo> = None;
    for item in list.iter() {
        let dict: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(*item as _) };
        let num = |k: &str| -> Option<f64> {
            dict.find(CFString::new(k))
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|n| n.to_f64())
        };
        if num("kCGWindowOwnerPID") != Some(pid as f64) || num("kCGWindowLayer") != Some(0.0) {
            continue;
        }
        let Some(bounds) = dict
            .find(CFString::new("kCGWindowBounds"))
            .and_then(|v| v.downcast::<CFDictionary>())
        else {
            continue;
        };
        let bounds: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(bounds.as_concrete_TypeRef()) };
        let b = |k: &str| {
            bounds
                .find(CFString::new(k))
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|n| n.to_f64())
                .unwrap_or(0.0)
        };
        let frame = Frame { x: b("X"), y: b("Y"), w: b("Width"), h: b("Height") };
        let title = dict
            .find(CFString::new("kCGWindowName"))
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let id = num("kCGWindowNumber").unwrap_or(0.0) as u32;
        if best.as_ref().is_none_or(|w| frame.w * frame.h > w.frame.w * w.frame.h) {
            best = Some(WindowInfo { id, frame, title });
        }
    }
    best.ok_or_else(|| anyhow!("no on-screen window for pid {pid}"))
}

/// Captures exactly the window (without shadow), even if partially covered.
pub fn capture(window_id: u32, out: &Path) -> Result<()> {
    let status = Command::new("screencapture")
        .args(["-x", "-o", &format!("-l{window_id}")])
        .arg(out)
        .status()
        .context("running screencapture")?;
    if !status.success() {
        bail!("screencapture failed for window {window_id}");
    }
    Ok(())
}

unsafe fn ax_attr(e: AXUIElementRef, name: &str) -> Option<CFTypeRef> {
    let mut v: CFTypeRef = ptr::null();
    let key = CFString::new(name);
    (AXUIElementCopyAttributeValue(e, key.as_concrete_TypeRef(), &mut v) == kAXErrorSuccess
        && !v.is_null())
    .then_some(v)
}

/// The app's AX windows. The returned array owns the element refs; keep it alive while using them.
unsafe fn ax_windows(pid: i32) -> Result<CFArray<CFTypeRef>> {
    let app = AXUIElementCreateApplication(pid);
    let v = ax_attr(app, "AXWindows");
    CFRelease(app as CFTypeRef);
    let v = v.ok_or_else(|| {
        anyhow!("no AX windows for pid {pid} (is Accessibility permission granted to the terminal?)")
    })?;
    Ok(CFArray::wrap_under_create_rule(v as CFArrayRef))
}

unsafe fn first_window(wins: &CFArray<CFTypeRef>) -> Result<AXUIElementRef> {
    wins.get(0).map(|w| *w as AXUIElementRef).ok_or_else(|| anyhow!("app has no windows"))
}

/// Moves/resizes the app's first window so screenshots are comparable across runs and apps.
pub fn place(pid: i32, frame: Frame) -> Result<()> {
    super::input::require_enabled()?;
    unsafe {
        let wins = ax_windows(pid)?;
        let win = first_window(&wins)?;
        let pos = CGPoint::new(frame.x, frame.y);
        let size = CGSize::new(frame.w, frame.h);
        let pos_v = AXValueCreate(kAXValueTypeCGPoint, &pos as *const _ as *const _);
        let size_v = AXValueCreate(kAXValueTypeCGSize, &size as *const _ as *const _);
        let pos_k = CFString::new("AXPosition");
        let size_k = CFString::new("AXSize");
        AXUIElementSetAttributeValue(win, pos_k.as_concrete_TypeRef(), pos_v as CFTypeRef);
        AXUIElementSetAttributeValue(win, size_k.as_concrete_TypeRef(), size_v as CFTypeRef);
        CFRelease(pos_v as CFTypeRef);
        CFRelease(size_v as CFTypeRef);
        let raise = CFString::new("AXRaise");
        AXUIElementPerformAction(win, raise.as_concrete_TypeRef());
    }
    Ok(())
}

/// Brings the app to the front so synthesized OS events reach it.
pub fn activate(pid: i32) -> Result<()> {
    super::input::require_enabled()?;
    let script = format!(
        "tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true"
    );
    let status = Command::new("osascript").args(["-e", &script]).status()?;
    if !status.success() {
        bail!("failed to activate pid {pid}");
    }
    Ok(())
}

/// Title of the app's focused window via AX (used to verify window title parity).
pub fn ax_window_title(pid: i32) -> Result<String> {
    unsafe {
        let wins = ax_windows(pid)?;
        let win = first_window(&wins)?;
        Ok(match ax_attr(win, "AXTitle") {
            Some(v) => CFString::wrap_under_create_rule(v as CFStringRef).to_string(),
            None => String::new(),
        })
    }
}
