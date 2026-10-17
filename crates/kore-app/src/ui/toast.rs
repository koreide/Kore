//! Toasts (spec 01 §11): bottom-right stack, glass, auto-dismiss after 3s,
//! click to dismiss. Call [`toast`] from anywhere with `&mut App`.

use crate::automation::Automation;
use crate::theme::{self, emerald, red, slate, KoreStyled};
use crate::ui::icon::icon;
use gpui_kit::*;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
}

pub struct ToastStore {
    pub toasts: Vec<Toast>,
    next: u64,
}

struct ToastGlobal(Entity<ToastStore>);
impl Global for ToastGlobal {}

pub fn init(cx: &mut App) -> Entity<ToastStore> {
    let e = cx.new(|_| ToastStore { toasts: Vec::new(), next: 1 });
    cx.set_global(ToastGlobal(e.clone()));
    e
}

/// Show a toast notification.
pub fn toast(cx: &mut App, message: impl Into<String>, kind: ToastKind) {
    let Some(store) = cx.try_global::<ToastGlobal>().map(|g| g.0.clone()) else { return };
    store.update(cx, |s, cx| s.push(message.into(), kind, cx));
}

impl ToastStore {
    pub fn push(&mut self, message: String, kind: ToastKind, cx: &mut Context<Self>) {
        let id = self.next;
        self.next += 1;
        self.toasts.push(Toast { id, message, kind });
        cx.notify();
        let t = cx.background_executor().timer(Duration::from_millis(3000));
        cx.spawn(async move |this, cx| {
            t.await;
            this.update(cx, |s, cx| s.dismiss(id, cx)).ok();
        })
        .detach();
    }

    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|t| t.id != id);
        cx.notify();
    }
}

impl Render for ToastStore {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = theme::accent(cx);
        div()
            .absolute()
            .bottom(px(16.))
            .right(px(16.))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.))
            .children(self.toasts.iter().map(|t| {
                let (ic, color) = match t.kind {
                    ToastKind::Success => ("check-circle-2", emerald::E400),
                    ToastKind::Error => ("x-circle", red::R400),
                    ToastKind::Info => ("info", accent),
                };
                let id = t.id;
                div()
                    .id(("toast", id))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .px(px(16.))
                    .py(px(10.))
                    .rounded(px(8.))
                    .glass()
                    .text_sm_tw()
                    .text_color(slate::S200)
                    .max_w(px(320.))
                    .cursor_pointer()
                    .occlude()
                    .on_click(cx.listener(move |this, _, _, cx| this.dismiss(id, cx)))
                    .child(icon(ic, 16.).text_color(color))
                    // Shrink-to-fit text width (max-content, capped by the 320px toast:
                    // 320 - border 2 - px 32 - icons 16+14 - gaps 20 = 236). GPUI would
                    // otherwise lay long messages out at min-content (one word per line).
                    .child(div().flex_1().min_w_0().w(px((crate::ui::text::measure(
                        window,
                        &t.message,
                        crate::theme::SANS,
                        14.,
                        FontWeight::NORMAL,
                    )
                    .ceil()
                        + 1.)
                        .min(236.)))
                    .child(crate::ui::text::hyphen_wrap(&t.message)))
                    .child(icon("x", 14.).text_color(slate::S500))
                    .with_animation(
                        ("toast-in", id),
                        Animation::new(Duration::from_millis(200)).with_easing(ease_out_quint()),
                        |el, t| el.opacity(t).mt(px(16. * (1. - t))),
                    )
                    .auto_id(format!("toast.{id}"), "toast", t.message.clone())
            }))
    }
}
