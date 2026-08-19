//! Light/dark palette approximating macOS system colours. Resolved from the
//! window appearance at render time.

use gpui::{Hsla, Rgba, Window, WindowAppearance, rgb, rgba};

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct Theme {
    pub is_dark: bool,
    pub window_bg: Hsla,
    pub sidebar_bg: Hsla,
    pub sidebar_selected: Hsla,
    pub sidebar_hover: Hsla,
    pub bar_bg: Hsla,
    pub separator: Hsla,
    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_tertiary: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub control_bg: Hsla,
    pub control_hover: Hsla,
    pub control_border: Hsla,
    pub badge_bg: Hsla,
    pub badge_text: Hsla,
    pub placeholder_bg: Hsla,
    pub slider_track: Hsla,
    pub slider_thumb: Hsla,
    pub danger: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub input_bg: Hsla,
    pub button_primary_bg: Hsla,
    pub button_primary_text: Hsla,
    pub scrim: Hsla,
    pub popover_bg: Hsla,
    pub segment_selected: Hsla,
    pub verdict_green: Hsla,
    pub verdict_blue: Hsla,
    pub verdict_amber: Hsla,
}

fn c(v: u32) -> Hsla {
    rgb(v).into()
}
fn ca(v: u32) -> Hsla {
    let r: Rgba = rgba(v);
    r.into()
}

impl Theme {
    pub fn light() -> Self {
        Theme {
            is_dark: false,
            window_bg: c(0xffffff),
            sidebar_bg: c(0xf2f2f4),
            sidebar_selected: ca(0x0000001a),
            sidebar_hover: ca(0x0000000c),
            bar_bg: c(0xf6f6f7),
            separator: ca(0x0000001f),
            text: c(0x1d1d1f),
            text_secondary: ca(0x00000099),
            text_tertiary: ca(0x00000059),
            accent: c(0x4617cc),
            accent_soft: ca(0x4617cc18),
            control_bg: ca(0x00000010),
            control_hover: ca(0x0000001c),
            control_border: ca(0x00000026),
            badge_bg: ca(0x00000026),
            badge_text: c(0x1d1d1f),
            placeholder_bg: ca(0x00000014),
            slider_track: ca(0x00000026),
            slider_thumb: c(0xffffff),
            danger: c(0xff3b30),
            success: c(0x34c759),
            warning: c(0xff9500),
            input_bg: c(0xffffff),
            button_primary_bg: c(0x4617cc),
            button_primary_text: c(0xffffff),
            scrim: ca(0x00000014),
            popover_bg: c(0xf7f7f9),
            segment_selected: c(0xffffff),
            verdict_green: c(0x28a745),
            verdict_blue: c(0x4617cc),
            verdict_amber: c(0xff9500),
        }
    }

    pub fn dark() -> Self {
        Theme {
            is_dark: true,
            window_bg: c(0x1e1e1e),
            sidebar_bg: c(0x28282a),
            sidebar_selected: ca(0xffffff1f),
            sidebar_hover: ca(0xffffff0f),
            bar_bg: c(0x252527),
            separator: ca(0xffffff1a),
            text: c(0xf5f5f7),
            text_secondary: ca(0xffffff99),
            text_tertiary: ca(0xffffff59),
            accent: c(0x7452ee),
            accent_soft: ca(0x7452ee2a),
            control_bg: ca(0xffffff14),
            control_hover: ca(0xffffff24),
            control_border: ca(0xffffff2a),
            badge_bg: ca(0xffffff2e),
            badge_text: c(0xf5f5f7),
            placeholder_bg: ca(0xffffff14),
            slider_track: ca(0xffffff2e),
            slider_thumb: c(0xe8e8ea),
            danger: c(0xff453a),
            success: c(0x30d158),
            warning: c(0xff9f0a),
            input_bg: ca(0xffffff0f),
            button_primary_bg: c(0x6039e6),
            button_primary_text: c(0xffffff),
            scrim: ca(0x00000040),
            popover_bg: c(0x2b2b2e),
            segment_selected: ca(0xffffff33),
            verdict_green: c(0x30d158),
            verdict_blue: c(0x7452ee),
            verdict_amber: c(0xff9f0a),
        }
    }

    pub fn for_window(window: &Window) -> Self {
        match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::dark(),
            _ => Theme::light(),
        }
    }
}

pub const UI_FONT: &str = ".SystemUIFont";
