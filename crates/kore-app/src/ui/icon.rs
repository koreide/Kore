//! Lucide icons (v0.424).

use gpui_kit::*;

/// `name` is the kebab-case Lucide name, e.g. `message-circle`. Colored via `text_color`.
pub fn icon(name: &str, size: f32) -> Svg {
    svg().path(SharedString::from(format!("lucide/{name}.svg"))).size(px(size)).flex_none()
}
