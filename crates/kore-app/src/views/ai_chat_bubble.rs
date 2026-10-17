//! `ChatMessageBubble` (spec 03 §6.4): user messages as right-aligned plain
//! text, assistant messages as markdown in a bubble with a Bot avatar and a
//! hover copy button. Shared by AI Chat (comfortable), the AI Panel and
//! Network Policies (compact).

use crate::automation::Automation;
use crate::theme::{self, slate};
use crate::ui::icon::icon;
use crate::ui::markdown;
use crate::ui::toast::{toast, ToastKind};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleVariant {
    /// AI Panel / Network Policies.
    Compact,
    /// AI Chat view.
    Comfortable,
}

/// A chat message. `role` is `"user"` or `"assistant"`.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayMessage {
    pub role: String,
    pub content: String,
}

impl DisplayMessage {
    pub fn user(content: impl Into<String>) -> Self {
        DisplayMessage { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        DisplayMessage { role: "assistant".into(), content: content.into() }
    }
    pub fn is_user(&self) -> bool {
        self.role == "user"
    }
}

/// Render one bubble. `id` must be unique in the list (e.g. `"chat-msg-3"`).
/// `copy`: assistant messages get the copy button (toast "Copied to clipboard").
/// Prefer [`chat_bubble_sized`] when the row width is known.
pub fn chat_bubble(
    id: impl Into<SharedString>,
    msg: &DisplayMessage,
    variant: BubbleVariant,
    copy: bool,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    chat_bubble_sized(id, msg, variant, copy, None, window, cx)
}

/// Like [`chat_bubble`], with the width of the message row (the bubble's
/// container): the bubble is then sized like CSS shrink-to-fit, i.e. its
/// max-content width capped at 85% of the row. Without it long text can
/// collapse to min-content width in GPUI.
pub fn chat_bubble_sized(
    id: impl Into<SharedString>,
    msg: &DisplayMessage,
    variant: BubbleVariant,
    copy: bool,
    row_width: Option<f32>,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    let id: SharedString = id.into();
    let user = msg.is_user();
    let compact = variant == BubbleVariant::Compact;
    let accent = theme::accent(cx);
    let (gap, avatar, avatar_r, icon_size, radius, padx, pady) =
        if compact { (10., 24., 6., 14., 8., 12., 8.) } else { (12., 28., 8., 16., 12., 16., 12.) };
    let group: SharedString = format!("{id}-bubble").into();

    let bubble_w = row_width.map(|rw| {
        let content = if user {
            msg.content
                .split('\n')
                .map(|l| crate::ui::text::measure(window, l, crate::theme::SANS, 14., FontWeight::NORMAL))
                .fold(0., f32::max)
        } else {
            markdown::max_content_width(&msg.content, window)
        };
        let chrome = 2. * padx + if user { 0. } else { 2. };
        (content.ceil() + 1. + chrome).min(rw * 0.85)
    });

    let body: AnyElement = if user {
        div().whitespace_normal().child(crate::ui::text::hyphen_wrap(&msg.content)).into_any_element()
    } else {
        markdown::render(format!("{id}-md"), &msg.content, window, cx)
    };

    let content = msg.content.clone();
    let copy_btn = (!user && copy).then(|| {
        div()
            .id(ElementId::Name(format!("{id}-copy").into()))
            .absolute()
            .top(px(-8.))
            .right(px(-8.))
            .p(px(4.))
            .rounded(px(4.))
            .bg(theme::SURFACE)
            .border_1()
            .border_color(slate::S700)
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .cursor_pointer()
            .child(icon("copy", 12.).text_color(slate::S400))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(content.clone()));
                toast(cx, "Copied to clipboard", ToastKind::Success);
            })
            .auto_id(format!("{id}.copy"), "button", "Copy message")
    });

    // The bubble clips its content; the copy button is a sibling in an
    // unclipped wrapper so it shows in full at the top-right corner (quirk
    // 10.13, fixed: it used to be clipped to its bottom-left 14×14).
    let bubble = div()
        .id(ElementId::Name(format!("{id}-b").into()))
        .w_full()
        .overflow_hidden()
        .text_size(px(14.))
        .line_height(px(14. * 1.625))
        .px(px(padx))
        .py(px(pady))
        .rounded(px(radius))
        .map(|d| {
            if user {
                d.rounded_br(px(2.)).text_color(slate::S100)
            } else {
                d.rounded_bl(px(2.))
                    .bg(if compact { theme::hexa(0x1d2939, 0.6) } else { theme::SURFACE })
                    .border_1()
                    .border_color(theme::hexa(0x1e293b, 0.5))
                    .text_color(slate::S200)
            }
        })
        .child(body.auto_id(format!("{id}.text"), "text", msg.content.clone()));
    let bubble = div()
        .group(group)
        .relative()
        // Shrink-to-fit: max-content width capped at 85%.
        .flex_shrink_0()
        .max_w(relative(0.85))
        .map(|d| match bubble_w {
            Some(w) => d.w(px(w)),
            None => d,
        })
        .child(bubble)
        .children(copy_btn);

    div()
        .w_full()
        .flex()
        .flex_row()
        .gap(px(gap))
        .when(user, |d| d.justify_end())
        .when(!user, |d| {
            d.child(
                div()
                    .flex_none()
                    .mt(px(2.))
                    .size(px(avatar))
                    .rounded(px(avatar_r))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("bot", icon_size).text_color(accent)),
            )
        })
        .child(bubble)
        .auto_id(id.to_string(), "listitem", msg.role.clone())
        .into_any_element()
}
