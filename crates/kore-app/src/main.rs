//! Kore — native Kubernetes desktop IDE.
// Release builds on Windows are GUI-subsystem (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod automation;
mod backend;
// Shared API surfaces used by feature views (Tasks 4–8); not all of it is
// referenced yet.
#[allow(dead_code)]
mod keys;
mod platform;
mod root;
#[allow(dead_code)]
mod state;
#[allow(dead_code)]
mod theme;
#[allow(dead_code)]
mod ui;
#[allow(dead_code)]
mod views;

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

use gpui_kit::*;

fn main() {
    // PATH enrichment (so `kubectl`/`helm`/exec-credential plugins resolve when
    // launched from Finder) mutates the process environment, so it must run
    // first, while the process is still single-threaded (A21): before the
    // tracing subscriber, GPUI or the kore-core runtime start any threads.
    kore_core::enrich_path();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            platform::set_app_icon();
            backend::init(cx);
            let settings = state::settings::Settings::load();
            theme::Theme::init(cx, settings.accent_color());
            if std::env::var("KORE_FONT_PROBE").is_ok() {
                font_probe(cx);
                cx.quit();
                return;
            }
            root::init_keymap(cx);

            // Layer 2 runs open the window without focusing or activating the
            // app, so the person at the machine keeps their foreground app.
            #[cfg(feature = "test-harness")]
            let background = std::env::var_os(kore_automation::BACKGROUND_ENV).is_some();
            #[cfg(not(feature = "test-harness"))]
            let background = false;

            // Main window: "Kore", 1400×900, resizable, native decorations.
            let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Kore".into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                focus: !background,
                show: true,
                is_resizable: true,
                app_id: Some("dev.kore.desktop".into()),
                ..Default::default()
            };

            let (window, view) = gpui_kit::open_window(options, cx, |window, cx| {
                platform::style_window(window);
                cx.new(|cx| root::KoreApp::new(settings, window, cx))
            })
            .expect("failed to open main window");

            #[cfg(feature = "test-harness")]
            if let Some(socket) = std::env::var_os(kore_automation::SOCKET_ENV) {
                automation::server::start(socket.into(), window, view, cx);
            }
            #[cfg(not(feature = "test-harness"))]
            let _ = (window, view);

            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            if !background {
                cx.activate(true);
            }
        });
}

/// `KORE_FONT_PROBE=1 kore`: print the 12px advance of candidate mono fonts.
fn font_probe(cx: &mut App) {
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNSMono.ttf") {
        let _ = cx.text_system().add_fonts(vec![std::borrow::Cow::Owned(bytes)]);
    }
    for fam in [".AppleSystemUIFontMonospaced", "SF Mono", "SFMono-Regular", ".SF NS Mono", ".SFNSMono", "Menlo"] {
        for w in [FontWeight::NORMAL, FontWeight::MEDIUM] {
            let f = Font { weight: w, ..font(SharedString::from(fam)) };
            let id = cx.text_system().resolve_font(&f);
            let adv = cx.text_system().advance(id, px(12.), '0').map(|s| f32::from(s.width)).unwrap_or(-1.);
            println!("{fam:32} w={:?} advance={adv:.3} font_id={id:?}", w.0);
        }
    }
}
