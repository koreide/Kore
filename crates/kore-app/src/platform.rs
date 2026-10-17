//! Small platform integrations.

/// Sets the Dock icon when running as a bare binary (bundled builds get it from Info.plist).
#[cfg(target_os = "macos")]
pub fn set_app_icon() {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    static ICON: &[u8] = include_bytes!("../assets/icons/icon.icns");
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(ICON);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        let app = NSApplication::sharedApplication(mtm);
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_app_icon() {}

/// Window chrome: the titlebar is transparent over the window background
/// (#0b1221) while the content stays below the titlebar.
#[cfg(target_os = "macos")]
pub fn style_window(window: &gpui_kit::Window) {
    use objc2_app_kit::{NSColor, NSView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: AppKit handles point at a live NSView owned by the window.
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let Some(ns_window) = view.window() else { return };
    let bg = crate::theme::BACKGROUND;
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(bg.r as f64, bg.g as f64, bg.b as f64, 1.0);
    ns_window.setBackgroundColor(Some(&color));
    ns_window.setTitlebarAppearsTransparent(true);
}

#[cfg(not(target_os = "macos"))]
pub fn style_window(_: &gpui_kit::Window) {}

/// Opens a URL in the system browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        tracing::warn!("failed to open {url}: {e}");
    }
}
