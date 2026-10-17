//! Banners (spec 01 §13): update banners, RBAC impersonation banner and the
//! AI config warning (used by the AI views).

use crate::automation::Automation;
use crate::state::AppState;
use crate::theme::{self, amber, emerald, red, slate, KoreStyled};
use crate::ui::icon::icon;
use crate::ui::widgets::spinning_icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

fn row() -> Div {
    div().w_full().flex().flex_row().items_center().gap(px(12.)).px(px(16.)).py(px(8.)).text_xs_tw().border_b_1()
}

/// Update success / error / available banner (top of window), if any.
pub fn update_banner(app: &Entity<AppState>, cx: &App) -> Option<AnyElement> {
    let accent = theme::accent(cx);
    let u = &app.read(cx).update;
    if let Some(v) = &u.success {
        return Some(
            row()
                .bg(theme::hexa(0x10b981, 0.15))
                .border_color(theme::hexa(0x10b981, 0.3))
                .child(icon("check-circle", 16.).text_color(emerald::E400))
                .child(div().text_color(emerald::E300).child(format!("Updated to {v} — restart Kore to apply.")))
                .auto_id("banner.update-success", "status", "Updated")
                .into_any_element(),
        );
    }
    if let Some(err) = &u.error {
        let url = u.info.as_ref().map(|i| i.release_url.clone()).filter(|s| !s.is_empty());
        let (a1, a2) = (app.clone(), app.clone());
        let _ = a1;
        return Some(
            row()
                .bg(theme::hexa(0xef4444, 0.1))
                .border_color(theme::hexa(0xef4444, 0.3))
                .child(icon("alert-triangle", 16.).text_color(red::R400))
                .child(div().text_color(red::R300).child(format!("Update failed: {err}")))
                .when_some(url, |d, url| {
                    d.child(
                        div()
                            .id("update-manual")
                            .group("update-manual")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .text_11()
                            .text_color(slate::S400)
                            .hover(|s| s.text_color(slate::S200))
                            .cursor_pointer()
                            .child(icon("external-link", 12.).text_color(slate::S400).group_hover("update-manual", |s| s.text_color(slate::S200)))
                            .child("Download manually")
                            .on_click(move |_, _, _| crate::platform::open_url(&url))
                            .auto_id("banner.update-manual", "button", "Download manually"),
                    )
                })
                .child(
                    div()
                        .id("update-err-x")
                        .group("update-err-x")
                        .ml_auto()
                        .cursor_pointer()
                        .child(icon("x", 14.).text_color(theme::hexa(0xf87171, 0.6)).group_hover("update-err-x", |s| s.text_color(red::R300)))
                        .on_click(move |_, _, cx| a2.update(cx, |s, cx| s.dismiss_update(cx)))
                        .auto_id("banner.update-dismiss", "button", "Dismiss"),
                )
                .auto_id("banner.update-error", "alert", "Update failed")
                .into_any_element(),
        );
    }
    let info = u.info.as_ref()?;
    if !info.has_update || u.dismissed {
        return None;
    }
    let installing = u.installing;
    let (a1, a2) = (app.clone(), app.clone());
    // In-app install is macOS-only; elsewhere the button opens the release page.
    let self_update = kore_core::SELF_UPDATE_SUPPORTED;
    let release_url = info.release_url.clone();
    let label: SharedString = format!("Kore {} is available", info.latest_version).into();
    Some(
        row()
            .border_color(theme::PREFLIGHT_BORDER)
            .justify_center()
            .child(div().text_color(accent).child(label.clone()))
            .child(
                div()
                    .id("update-btn")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .px(px(10.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .text_11()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(accent)
                    .border_1()
                    .border_color(theme::PREFLIGHT_BORDER)
                    .when(installing, |d| d.opacity(0.5))
                    .when(!installing, |d| d.cursor_pointer())
                    .child(if installing {
                        spinning_icon("update-spin", "loader-2", 12., accent).into_any_element()
                    } else {
                        icon(if self_update { "download" } else { "external-link" }, 12.).text_color(accent).into_any_element()
                    })
                    .child(if installing { "Installing..." } else if self_update { "Update" } else { "Download" })
                    .on_click(move |_, _, cx| match (installing, self_update) {
                        (true, _) => {}
                        (false, true) => a1.update(cx, |s, cx| s.perform_update(cx)),
                        (false, false) => crate::platform::open_url(&release_url),
                    })
                    .auto_id("banner.update-install", "button", if self_update { "Update" } else { "Download" }),
            )
            .child(
                div()
                    .id("update-x")
                    .cursor_pointer()
                    .child(icon("x", 14.).text_color(slate::S100))
                    .on_click(move |_, _, cx| a2.update(cx, |s, cx| s.dismiss_update(cx)))
                    .auto_id("banner.update-dismiss", "button", "Dismiss until next launch"),
            )
            .auto_id("banner.update", "status", label)
            .into_any_element(),
    )
}

/// RBAC impersonation banner (top of the main column).
pub fn impersonation_banner(app: &Entity<AppState>, cx: &App) -> Option<AnyElement> {
    let imp = app.read(cx).impersonation.clone()?;
    let a = app.clone();
    Some(
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(8.))
            .bg(theme::hexa(0xf59e0b, 0.1))
            .border_b_1()
            .border_color(theme::hexa(0xf59e0b, 0.2))
            .child(icon("shield-alert", 16.).text_color(amber::A400))
            .child(div().text_xs_tw().text_color(amber::A300).child("Impersonation Mode:"))
            .child(div().text_xs_tw().font_mono().text_color(amber::A200).child(format!("viewing as {}", imp.label)))
            .child(
                div()
                    .ml(px(4.))
                    .text_10()
                    .text_color(theme::hexa(0xf59e0b, 0.7))
                    .child("(simulated — actual API calls use your real credentials)"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("imp-exit")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .text_xs_tw()
                    .text_color(amber::A400)
                    .hover(|s| s.bg(theme::hexa(0xf59e0b, 0.1)))
                    .cursor_pointer()
                    .child(icon("x", 12.).text_color(amber::A400))
                    .child("Exit")
                    .on_click(move |_, _, cx| a.update(cx, |s, cx| s.set_impersonation(None, cx)))
                    .auto_id("banner.impersonation.exit", "button", "Exit"),
            )
            .auto_id("banner.impersonation", "status", "Impersonation Mode")
            .into_any_element(),
    )
}

/// AIConfigWarning (§13.4).
pub fn ai_config_warning() -> impl IntoElement {
    div()
        .px(px(16.))
        .py(px(10.))
        .border_b_1()
        .border_color(theme::hexa(0x1e293b, 0.5))
        .bg(theme::hexa(0xf59e0b, 0.05))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .text_xs_tw()
        .child(icon("alert-circle", 14.).text_color(amber::A400))
        .child(div().text_color(theme::hexa(0xfcd34d, 0.9)).child("Configure an AI provider in Settings to use this feature"))
        .auto_id("banner.ai-config", "alert", "Configure an AI provider")
}
