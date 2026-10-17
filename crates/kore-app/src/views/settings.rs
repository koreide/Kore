//! Settings page (spec 01 §17). Esc (`AppEvent::Escape`) returns to the table.

use crate::automation::Automation;
use crate::keys;
use crate::state::types::AppView;
use crate::state::{AppEvent, AppState};
use crate::theme::{self, slate, KoreStyled, ACCENT_CHOICES};
use crate::ui::icon::icon;
use crate::ui::input::{InputEvent, InputStyle, TextInput};
use crate::ui::native_controls::{NativeSelect, SelectEvent, SelectStyle};
use crate::ui::scrollbar;
use crate::ui::text::caps;
use crate::views::ai_settings::{text_box, AiSettings};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

const UP_TO_DATE_MODAL: &str = "settings-up-to-date";

const DEFAULT_VIEW_OPTIONS: [(&str, &str); 3] = [("chat", "AI Chat"), ("table", "Resource Table"), ("dashboard", "Dashboard")];
const RETENTION_OPTIONS: [(&str, &str); 5] =
    [("1d", "1 day"), ("3d", "3 days"), ("7d", "7 days"), ("14d", "14 days"), ("30d", "30 days")];

const SHORTCUTS: [(&str, &[(&str, &str)]); 3] = [
    ("Navigation", &[("j / k", "Navigate up / down"), ("l", "Enter detail view"), ("h", "Go back"), ("1 - 5", "Switch tabs")]),
    (
        "Actions",
        &[
            (crate::keys::MOD_K, "Command palette"),
            (crate::keys::MOD_R, "Refresh resources"),
            ("/", "Search / filter"),
            (crate::keys::MOD_F, "Search logs"),
            ("D", "Delete resource"),
        ],
    ),
    ("General", &[("Esc", "Close / go back"), ("Enter", "Select / confirm"), ("?", "Show shortcut overlay")]),
];

pub struct SettingsView {
    app: Entity<AppState>,
    ai: Entity<AiSettings>,
    default_view: Entity<NativeSelect>,
    retention: Entity<NativeSelect>,
    ctx_input: Entity<TextInput>,
    ns_input: Entity<TextInput>,
    checking: bool,
    up_to_date: bool,
    /// Set when the last "Check for Updates" failed: the modal shows this error
    /// instead of "You're up to date!".
    update_error: Option<String>,
    scroll: ScrollHandle,
    gutter: scrollbar::Gutter,
    _subs: Vec<Subscription>,
}

fn select_style() -> SelectStyle {
    SelectStyle {
        padding: (12., 8.),
        radius: 6.,
        font_size: 14.,
        line_height: 20.,
        text_color: slate::S100,
        bg: theme::BACKGROUND,
        border: slate::S800,
        full_width: true,
    }
}

fn opts(o: &[(&str, &str)]) -> Vec<(String, String)> {
    o.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect()
}

impl SettingsView {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = app.read(cx).settings.clone();
        let weak = app.downgrade();
        let default_view = cx.new(|cx| {
            let mut n = NativeSelect::new("settings.default-view", opts(&DEFAULT_VIEW_OPTIONS), s.default_view.clone(), select_style(), weak.clone(), cx);
            n.show_arrow = false;
            n
        });
        let retention = cx.new(|cx| {
            let mut n = NativeSelect::new("settings.retention", opts(&RETENTION_OPTIONS), s.event_retention.clone(), select_style(), weak.clone(), cx);
            n.show_arrow = false;
            n
        });
        let style = InputStyle { placeholder_color: slate::S600, padding: (12., 12., 8., 8.), ..Default::default() };
        let ctx_input = cx.new(|cx| TextInput::new("Context name", style.clone(), window, cx));
        let ns_input = cx.new(|cx| TextInput::new("Namespace", style, window, cx));
        let ai = cx.new(|cx| AiSettings::new(app.clone(), window, cx));
        let subs = vec![
            cx.subscribe(&app, |this, app, ev: &AppEvent, cx| {
                if matches!(ev, AppEvent::Escape) && !this.up_to_date {
                    app.update(cx, |s, cx| s.set_view(AppView::Table, cx));
                }
            }),
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.subscribe(&default_view, |this, _, ev: &SelectEvent, cx| {
                let SelectEvent::Change(v) = ev;
                let v = v.clone();
                this.app.update(cx, |s, cx| s.update_settings(|st| st.default_view = v, cx));
            }),
            cx.subscribe(&retention, |this, _, ev: &SelectEvent, cx| {
                let SelectEvent::Change(v) = ev;
                let v = v.clone();
                this.app.update(cx, |s, cx| s.update_settings(|st| st.event_retention = v, cx));
            }),
            cx.observe(&default_view, |_, _, cx| cx.notify()),
            cx.observe(&retention, |_, _, cx| cx.notify()),
            cx.subscribe(&ctx_input, |_, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change(_) | InputEvent::Focus | InputEvent::Blur) {
                    cx.notify()
                }
            }),
            cx.subscribe(&ns_input, |this, _, ev: &InputEvent, cx| match ev {
                InputEvent::Enter { .. } => this.add_default_ns(cx),
                InputEvent::Change(_) | InputEvent::Focus | InputEvent::Blur => cx.notify(),
                _ => {}
            }),
            cx.on_release(|this: &mut Self, cx| {
                if this.up_to_date {
                    this.app.update(cx, |s, cx| s.set_modal(UP_TO_DATE_MODAL, false, cx));
                }
            }),
            // Esc closes the up-to-date modal (Fix 4: it does not navigate back).
            keys::on_window_key(cx, |this: &mut Self, k, _, cx| {
                if this.up_to_date && k.key == "escape" && k.plain() {
                    this.set_up_to_date(false, cx);
                    return true;
                }
                false
            }),
        ];
        SettingsView {
            app,
            ai,
            default_view,
            retention,
            ctx_input,
            ns_input,
            checking: false,
            up_to_date: false,
            update_error: None,
            scroll: ScrollHandle::new(),
            gutter: scrollbar::Gutter::default(),
            _subs: subs,
        }
    }

    fn set_up_to_date(&mut self, open: bool, cx: &mut Context<Self>) {
        self.up_to_date = open;
        self.app.update(cx, |s, cx| s.set_modal(UP_TO_DATE_MODAL, open, cx));
        cx.notify();
    }

    fn add_default_ns(&mut self, cx: &mut Context<Self>) {
        let c = self.ctx_input.read(cx).text().trim().to_string();
        let n = self.ns_input.read(cx).text().trim().to_string();
        if c.is_empty() || n.is_empty() {
            return;
        }
        self.app.update(cx, |s, cx| s.update_settings(|st| {
            st.default_namespaces.insert(c, n);
        }, cx));
        self.ctx_input.update(cx, |i, cx| i.set_text("", cx));
        self.ns_input.update(cx, |i, cx| i.set_text("", cx));
        cx.notify();
    }

    fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        let this = cx.entity().downgrade();
        self.app.update(cx, |s, cx| {
            s.check_updates_now(cx, move |res, cx| {
                let has_update = res.as_ref().map(|i| i.has_update).unwrap_or(false);
                let error = res.as_ref().err().cloned();
                // A fresh result re-shows a dismissed banner.
                if res.is_ok() {
                    s_undismiss(cx);
                }
                // Leave the AppState update before touching the view (it updates AppState too).
                cx.defer(move |cx| {
                    let _ = this.update(cx, |v, cx| {
                        v.checking = false;
                        v.update_error = error;
                        // Failure → error modal; no update → "up to date" modal.
                        if !has_update {
                            v.set_up_to_date(true, cx);
                        }
                        cx.notify();
                    });
                });
            })
        });
        cx.notify();
    }
}

fn s_undismiss(cx: &mut Context<AppState>) {
    let e = cx.entity();
    cx.defer(move |cx| e.update(cx, |s, cx| {
        s.update.dismissed = false;
        cx.notify();
    }));
}

/// Settings `Kbd`: min-w 22, px 6, py 2, 10px mono slate-400 (line height inherited).
fn kbd(text: &str) -> Div {
    div()
        .flex_none()
        .min_w(px(22.))
        .px(px(6.))
        .py(px(2.))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::hexa(0x1e293b, 0.8))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::hexa(0x334155, 0.5))
        .text_10()
        .font_mono()
        .text_color(slate::S400)
        .child(text.to_string())
}

fn card(id: &str) -> Stateful<Div> {
    div().id(SharedString::from(format!("settings-card-{id}"))).w_full().glass().rounded(px(12.)).p(px(20.))
}

fn card_title(ic: &str, title: &str, accent: Rgba) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .mb(px(16.))
        .child(icon(ic, 16.).text_color(accent))
        .child(div().text_sm_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child(title.to_string()))
        .auto_id(format!("settings.card.{}", title.to_lowercase().replace(' ', "-")), "heading", title.to_string())
}

fn label(ic: Option<&str>, text: &str, mb: f32) -> Div {
    div()
        .mb(px(mb))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .text_xs_tw()
        .text_color(slate::S400)
        .children(ic.map(|i| icon(i, 12.).text_color(slate::S400)))
        .child(text.to_string())
}

fn helper(text: &str) -> Div {
    div().mt(px(4.)).text_10().text_color(slate::S500).child(text.to_string())
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        let settings = self.app.read(cx).settings.clone();
        let version = self.app.read(cx).update.info.as_ref().map(|i| i.current_version.clone()).filter(|v| !v.is_empty());

        let header = div()
            .flex_none()
            .border_b_1()
            .border_color(slate::S800)
            .p(px(16.))
            .bg(theme::hexa(0x101828, 0.5))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .id("settings-back")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(12.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(slate::S800)
                            .text_sm_tw()
                            .text_color(slate::S300)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hexa(0x1d2939, 0.3)))
                            .child(icon("arrow-left", 16.).text_color(slate::S300))
                            .child("Back")
                            .child(kbd("Esc"))
                            .on_click(cx.listener(|this, _, _, cx| this.app.update(cx, |s, cx| s.set_view(AppView::Table, cx))))
                            .auto_id("settings.back", "button", "Back"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("settings", 20.).text_color(accent))
                            .child(div().text_lg_tw().font_weight(FontWeight::SEMIBOLD).text_color(slate::S100).child("Settings")),
                    ),
            );

        // General
        let general = card("general").child(card_title("clock", "General", accent)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .child(label(Some("layout"), "Default View", 6.))
                        .child(div().max_w(px(320.)).child(self.default_view.clone()))
                        .child(helper("Which view to show when the app starts.")),
                )
                .child(
                    div()
                        .child(label(None, "Event Retention Period", 6.))
                        .child(div().max_w(px(320.)).child(self.retention.clone()))
                        .child(helper("How long to keep event history in the timeline view.")),
                ),
        );

        let ai = card("ai").child(self.ai.clone());

        // Display
        let entries: Vec<(String, String)> = settings.default_namespaces.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let can_add = !self.ctx_input.read(cx).text().trim().is_empty() && !self.ns_input.read(cx).text().trim().is_empty();
        let ctx_f = self.ctx_input.read(cx).is_focused(window);
        let ns_f = self.ns_input.read(cx).is_focused(window);
        let ns_field = div()
            .child(label(Some("globe"), "Default Namespace per Context", 6.))
            .when(!entries.is_empty(), |d| {
                d.child(div().mb(px(12.)).flex().flex_col().gap(px(6.)).children(entries.into_iter().map(|(c, n)| {
                    let key = c.clone();
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .px(px(12.))
                        .py(px(6.))
                        .bg(theme::hexa(0x0b1221, 0.5))
                        .border_1()
                        .border_color(theme::hexa(0x1e293b, 0.5))
                        .rounded(px(4.))
                        .text_xs_tw()
                        .child(div().flex_1().min_w_0().truncate().font_mono().text_color(slate::S300).child(c.clone()))
                        .child(div().text_color(slate::S600).child("→"))
                        .child(div().truncate().font_mono().text_color(accent).child(n))
                        .child(
                            div()
                                .id(SharedString::from(format!("settings-ns-rm-{c}")))
                                .ml(px(4.))
                                .text_color(slate::S500)
                                .cursor_pointer()
                                .hover(|s| s.text_color(theme::red::R400))
                                .child("×")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let k = key.clone();
                                    this.app.update(cx, |s, cx| s.update_settings(|st| {
                                        st.default_namespaces.remove(&k);
                                    }, cx));
                                }))
                                .auto_id(format!("settings.default-ns.remove.{c}"), "button", "Remove"),
                        )
                })))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(div().flex_1().child(text_box("settings-ctx", self.ctx_input.clone(), ctx_f, accent, theme::BACKGROUND, 6.)).auto_id("settings.default-ns.context", "textbox", "Context name"))
                    .child(div().flex_1().child(text_box("settings-ns", self.ns_input.clone(), ns_f, accent, theme::BACKGROUND, 6.)).auto_id("settings.default-ns.namespace", "textbox", "Namespace"))
                    .child(
                        div()
                            .id("settings-ns-add")
                            .flex_none()
                            .px(px(16.))
                            .py(px(8.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme::PREFLIGHT_BORDER)
                            .text_xs_tw()
                            .line_height(px(20.))
                            .text_color(accent)
                            .when(!can_add, |d| d.opacity(0.4).cursor_not_allowed())
                            .when(can_add, |d| d.cursor_pointer().on_click(cx.listener(|this, _, _, cx| this.add_default_ns(cx))))
                            .child("Add")
                            .auto_id("settings.default-ns.add", "button", "Add")
                            .disabled(!can_add),
                    ),
            )
            .child(helper("Automatically select a namespace when switching to a context."));

        let current_accent = settings.accent_color.to_lowercase();
        let accent_field = div().child(label(Some("palette"), "Theme Accent Color", 8.)).child(
            div().flex().flex_row().gap(px(12.)).children(ACCENT_CHOICES.iter().map(|(name, hex)| {
                let value = format!("#{hex:06x}");
                let selected = current_accent == value;
                let v2 = value.clone();
                div()
                    .id(SharedString::from(format!("settings-accent-{name}")))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .cursor_pointer()
                    .map(|d| {
                        if selected {
                            d.border_color(slate::S600).bg(theme::hexa(0x1e293b, 0.5))
                        } else {
                            d.border_color(theme::hexa(0, 0.)).hover(|s| s.border_color(slate::S700).bg(theme::hexa(0x1e293b, 0.3)))
                        }
                    })
                    .child(
                        // ring-2 ring-offset-2 (offset color = background), scale 1.1 when selected
                        div()
                            .size(px(if selected { 35.2 } else { 32. }))
                            .m(px(if selected { -1.6 } else { 0. }))
                            .rounded_full()
                            .bg(theme::hex(*hex))
                            .when(selected, |d| {
                                d.shadow(vec![
                                    BoxShadow { color: slate::S400.into(), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(4.4), inset: false },
                                    BoxShadow { color: theme::BACKGROUND.into(), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(2.2), inset: false },
                                ])
                            }),
                    )
                    .child(div().text_10().text_color(slate::S500).child(name.to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let v = v2.clone();
                        this.app.update(cx, |s, cx| s.update_settings(|st| st.accent_color = v, cx));
                    }))
                    .auto_id(format!("settings.accent.{name}"), "radio", name.to_string())
                    .selected(selected)
            })),
        );
        let display = card("display")
            .child(card_title("monitor", "Display", accent))
            .child(div().flex().flex_col().gap(px(20.)).child(ns_field).child(accent_field));

        // Keyboard shortcuts
        let shortcuts = card("shortcuts").child(card_title("keyboard", "Keyboard Shortcuts", accent)).child(
            div().flex().flex_col().gap(px(16.)).children(SHORTCUTS.iter().map(|(cat, items)| {
                div()
                    .child(div().mb(px(8.)).text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S500).child(caps(cat, 10.)))
                    .child(div().flex().flex_col().gap(px(2.)).children(items.iter().map(|(keys, desc)| {
                        let parts: Vec<&str> = keys.split(" / ").collect();
                        div()
                            .id(SharedString::from(format!("settings-sc-{desc}")))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(4.))
                            .hover(|s| s.bg(theme::hexa(0xffffff, 0.02)))
                            .child(div().text_xs_tw().text_color(slate::S300).child(desc.to_string()))
                            .child(div().flex().flex_row().items_center().gap(px(4.)).children(parts.iter().enumerate().map(|(i, k)| {
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.))
                                    .when(i > 0, |d| d.child(div().mx(px(2.)).text_10().text_color(slate::S600).child("/")))
                                    .child(kbd(k.trim()))
                            })))
                            .auto_id(format!("settings.shortcut.{desc}"), "listitem", keys.to_string())
                    })))
            })),
        );

        // About
        let checking = self.checking;
        let about = card("about").child(card_title("info", "About", accent)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_xs_tw().text_color(slate::S400).child("Current Version"))
                        .child(
                            div()
                                .text_xs_tw()
                                .font_mono()
                                .text_color(slate::S200)
                                .child(version.clone().unwrap_or_else(|| "unknown".into()))
                                .auto_id("settings.version", "text", version.clone().unwrap_or_else(|| "unknown".into())),
                        ),
                )
                .child(
                    div().flex().child(
                        div()
                            .id("settings-check-updates")
                            .group("settings-check-updates")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(12.))
                            .py(px(8.))
                            .bg(theme::BACKGROUND)
                            .border_1()
                            .border_color(slate::S800)
                            .rounded(px(6.))
                            .text_xs_tw()
                            .text_color(slate::S300)
                            .when(checking, |d| d.opacity(0.5))
                            .when(!checking, |d| d.cursor_pointer().hover(|s| s.text_color(slate::S100)))
                            .child(if checking {
                                crate::ui::widgets::spinning_icon("settings-check-spin", "refresh-cw", 14., slate::S300).into_any_element()
                            } else {
                                icon("refresh-cw", 14.)
                                    .text_color(slate::S300)
                                    .group_hover("settings-check-updates", |s| s.text_color(slate::S100))
                                    .into_any_element()
                            })
                            .child(if checking { "Checking..." } else { "Check for Updates" })
                            .on_click(cx.listener(|this, _, _, cx| this.check_updates(cx)))
                            .auto_id("settings.check-updates", "button", "Check for Updates"),
                    ),
                ),
        );

        let gutter = self.gutter.overflows();
        let content = div()
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_row()
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p(px(24.))
                    .child(div().flex().flex_col().gap(px(24.)).child(general).child(ai).child(display).child(shortcuts).child(about)),
            )
            .when(gutter, |d| d.child(div().w(px(scrollbar::WIDTH)).flex_none()))
            .child(scrollbar::vertical(self.scroll.clone(), self.gutter.clone()));

        let update_error = self.update_error.clone();
        let modal = self.up_to_date.then(|| {
            let vp = window.viewport_size();
            deferred(
                anchored().position_mode(AnchoredPositionMode::Window).position(point(px(0.), px(0.))).child(
                    div()
                        .id("settings-up-to-date-backdrop")
                        .w(vp.width)
                        .h(vp.height)
                        .occlude()
                        .bg(theme::hexa(0, 0.85))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| this.set_up_to_date(false, cx)))
                        .child(
                            div()
                                .id("settings-up-to-date")
                                .w_full()
                                .max_w(px(384.))
                                .mx(px(16.))
                                .glass()
                                .rounded(px(12.))
                                .p(px(24.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .text_center()
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(match &update_error {
                                    Some(_) => icon("alert-circle", 40.).text_color(theme::red::R400).mb(px(12.)),
                                    None => icon("check-circle", 40.).text_color(theme::emerald::E400).mb(px(12.)),
                                })
                                .child(
                                    div()
                                        .text_sm_tw()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(slate::S100)
                                        .mb(px(4.))
                                        .child(if update_error.is_some() { "Couldn't check for updates" } else { "You're up to date!" }),
                                )
                                .child(
                                    div()
                                        .text_xs_tw()
                                        .text_color(slate::S400)
                                        .mb(px(16.))
                                        .child(match &update_error {
                                            Some(e) => crate::ui::text::prose_text(e.clone()).into_any_element(),
                                            None => format!("Kore {} is the latest version.", version.clone().unwrap_or_default()).into_any_element(),
                                        }),
                                )
                                .child(
                                    div()
                                        .id("settings-up-to-date-ok")
                                        .px(px(16.))
                                        .py(px(8.))
                                        .rounded(px(6.))
                                        .border_1()
                                        .border_color(theme::PREFLIGHT_BORDER)
                                        .text_xs_tw()
                                        .text_color(accent)
                                        .cursor_pointer()
                                        .child("OK")
                                        .on_click(cx.listener(|this, _, _, cx| this.set_up_to_date(false, cx)))
                                        .auto_id("settings.up-to-date.ok", "button", "OK"),
                                )
                                .auto_id(
                                    if update_error.is_some() { "settings.update-error" } else { "settings.up-to-date" },
                                    "dialog",
                                    match &update_error {
                                        Some(e) => format!("Couldn't check for updates: {e}"),
                                        None => "You're up to date!".to_string(),
                                    },
                                ),
                        ),
                ),
            )
            .with_priority(3)
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BACKGROUND)
            .child(header)
            .child(content)
            .children(modal)
            .auto_id("view.settings", "region", "Settings")
    }
}
