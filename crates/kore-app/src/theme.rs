//! Design tokens, resolved to the values of the original design
//! (see docs/gpui-port/spec/01-shell-navigation.md §0).
//!
//! In that design, "accent tint" (`accent/<opacity>`) borders render as the
//! default gray (`PREFLIGHT_BORDER`) and tint backgrounds are absent; the tokens
//! keep that look.

use gpui_kit::*;

/// `0xRRGGBB` → opaque color.
pub const fn hex(c: u32) -> Rgba {
    Rgba {
        r: ((c >> 16) & 0xff) as f32 / 255.0,
        g: ((c >> 8) & 0xff) as f32 / 255.0,
        b: (c & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// `0xRRGGBB` with alpha 0..1.
pub const fn hexa(c: u32, a: f32) -> Rgba {
    let mut x = hex(c);
    x.a = a;
    x
}

// Core palette.
pub const BACKGROUND: Rgba = hex(0x0b1221);
pub const SURFACE: Rgba = hex(0x101828);
pub const MUTED: Rgba = hex(0x1d2939);
pub const DEFAULT_ACCENT: u32 = 0x58d0ff;
/// Settings → accent choices, in display order.
pub const ACCENT_CHOICES: [(&str, u32); 5] = [
    ("Cyan", 0x58d0ff),
    ("Violet", 0xa78bfa),
    ("Emerald", 0x34d399),
    ("Orange", 0xfb923c),
    ("Red", 0xf87171),
];
/// Default border color; what accent-tinted (`border-accent/NN`) borders use.
pub const PREFLIGHT_BORDER: Rgba = hex(0xe5e7eb);
pub const PREFLIGHT_PLACEHOLDER: Rgba = hex(0x9ca3af);

// Color scales (slate, gray, …).
pub mod slate {
    use super::*;
    pub const S100: Rgba = hex(0xf1f5f9);
    pub const S200: Rgba = hex(0xe2e8f0);
    pub const S300: Rgba = hex(0xcbd5e1);
    pub const S400: Rgba = hex(0x94a3b8);
    pub const S500: Rgba = hex(0x64748b);
    pub const S600: Rgba = hex(0x475569);
    pub const S700: Rgba = hex(0x334155);
    pub const S800: Rgba = hex(0x1e293b);
    pub const S900: Rgba = hex(0x0f172a);
}
pub mod emerald {
    use super::*;
    pub const E300: Rgba = hex(0x6ee7b7);
    pub const E400: Rgba = hex(0x34d399);
    pub const E500: Rgba = hex(0x10b981);
}
pub mod amber {
    use super::*;
    pub const A200: Rgba = hex(0xfde68a);
    pub const A300: Rgba = hex(0xfcd34d);
    pub const A400: Rgba = hex(0xfbbf24);
    pub const A500: Rgba = hex(0xf59e0b);
}
pub mod red {
    use super::*;
    pub const R300: Rgba = hex(0xfca5a5);
    pub const R400: Rgba = hex(0xf87171);
    pub const R500: Rgba = hex(0xef4444);
    pub const R600: Rgba = hex(0xdc2626);
    pub const R700: Rgba = hex(0xb91c1c);
}
pub mod misc {
    use super::*;
    pub const ORANGE400: Rgba = hex(0xfb923c);
    pub const ORANGE500: Rgba = hex(0xf97316);
    pub const BLUE400: Rgba = hex(0x60a5fa);
    pub const YELLOW500: Rgba = hex(0xeab308);
    pub const PINK400: Rgba = hex(0xf472b6);
    pub const PINK500: Rgba = hex(0xec4899);
}

/// `.glass` (rendered): bg rgba(16,24,40,.7), 1px border white@6%, layered shadow.
pub const GLASS_BG: Rgba = hexa(0x101828, 0.7);
/// `.glass` composited over the black/60 overlay backdrop (`#0b1221` behind).
/// GPUI has no backdrop blur, so overlay panels use this opaque fill to keep
/// the content underneath from showing through (a blur would hide it).
pub const GLASS_OVER_DIM: Rgba = hex(0x0d1320);
pub const GLASS_BORDER: Rgba = hexa(0xffffff, 0.06);

// Typography (spec 01 §0.3).
#[cfg(target_os = "macos")]
pub const SANS: &str = ".SystemUIFont";
#[cfg(target_os = "windows")]
pub const SANS: &str = "Segoe UI";
#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub const SANS: &str = "Inter";

#[cfg(target_os = "macos")]
pub const MONO: &str = ".AppleSystemUIFontMonospaced";
#[cfg(target_os = "windows")]
pub const MONO: &str = "Consolas";
#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub const MONO: &str = "DejaVu Sans Mono";

/// Monospace advance: one SF Mono glyph is 0.601em wide (7.21px at 12px). The
/// font's own advance is 1266/2048 = 0.618em, which is what GPUI/CoreText
/// renders for both `.AppleSystemUIFontMonospaced` and SFNSMono.ttf ("SF Mono" /
/// "SFMono-Regular" don't resolve as family names). The design uses the tighter
/// 0.601em pitch, which GPUI's text layout can't express, so mono text uses
/// `ui::mono_text::mono_text` (paints glyphs at this pitch) and layout math
/// uses this constant.
pub const MONO_ADVANCE_EM: f32 = 0.601;

/// Global, user-configurable parts of the theme.
pub struct Theme {
    pub accent: Rgba,
}

impl Global for Theme {}

impl Theme {
    pub fn init(cx: &mut App, accent: u32) {
        cx.set_global(Theme { accent: hex(accent) });
    }

    pub fn set_accent(cx: &mut App, accent: u32) {
        cx.set_global(Theme { accent: hex(accent) });
    }

    pub fn get(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    /// `color-mix(in srgb, accent 40%, transparent)` — the global focus ring color.
    pub fn focus_ring(&self) -> Rgba {
        Rgba { a: 0.4, ..self.accent }
    }
}

pub fn accent(cx: &App) -> Rgba {
    Theme::get(cx).accent
}

/// Shadow stack of `.glass`: `0 0 0 1px white@2%, 0 4px 24px black@30%`.
pub fn glass_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla_of(hexa(0xffffff, 0.02)),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: false,
        },
        BoxShadow {
            color: hsla_of(hexa(0x000000, 0.3)),
            offset: point(px(0.), px(4.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

pub fn hsla_of(c: Rgba) -> Hsla {
    c.into()
}

/// Style extension for recurring style idioms.
pub trait KoreStyled: Styled + Sized {
    /// `.glass` as rendered: overrides bg/border/shadow of the element.
    fn glass(self) -> Self {
        self.bg(GLASS_BG).border_1().border_color(GLASS_BORDER).shadow(glass_shadow())
    }
    fn font_mono(self) -> Self {
        self.font_family(MONO)
    }
    fn font_sans(self) -> Self {
        self.font_family(SANS)
    }
    /// Text sizes with their line heights.
    fn text_9(self) -> Self {
        self.text_size(px(9.))
    }
    fn text_10(self) -> Self {
        self.text_size(px(10.))
    }
    fn text_11(self) -> Self {
        self.text_size(px(11.))
    }
    fn text_xs_tw(self) -> Self {
        self.text_size(px(12.)).line_height(px(16.))
    }
    fn text_sm_tw(self) -> Self {
        self.text_size(px(14.)).line_height(px(20.))
    }
    fn text_base_tw(self) -> Self {
        self.text_size(px(16.)).line_height(px(24.))
    }
    fn text_lg_tw(self) -> Self {
        self.text_size(px(18.)).line_height(px(28.))
    }
    fn text_xl_tw(self) -> Self {
        self.text_size(px(20.)).line_height(px(28.))
    }
    fn text_2xl_tw(self) -> Self {
        self.text_size(px(24.)).line_height(px(32.))
    }
    /// `uppercase tracking-wider` section headings.
    fn heading_caps(self) -> Self {
        self.text_10().font_weight(FontWeight::MEDIUM).text_color(slate::S500)
    }
}

impl<T: Styled> KoreStyled for T {}
