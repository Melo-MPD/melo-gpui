use crate::theme::Theme;
use crate::ui::widgets::icon;
use gpui::{Div, ElementId, Hsla, SharedString, Stateful, div, prelude::*, px};

/// Borderless icon button (SwiftUI `.buttonStyle(.plain)`).
pub fn icon_button(id: impl Into<ElementId>, name: &str, size: f32, color: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .p(px(4.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.opacity(0.75))
        .active(|s| s.opacity(0.55))
        .child(icon(name, size, color))
}

/// Standard push button; `primary` = filled accent, otherwise bezel.
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    primary: bool,
) -> Stateful<Div> {
    let (bg, fg, hover) = if primary {
        (
            theme.button_primary_bg,
            theme.button_primary_text,
            theme.button_primary_bg.opacity(0.85),
        )
    } else {
        (theme.control_bg, theme.text, theme.control_hover)
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(24.))
        .px(px(12.))
        .rounded(px(6.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(13.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.7))
        .child(label.into())
}

/// Small button with an icon and a label (toolbar style).
pub fn icon_text_button(
    id: impl Into<ElementId>,
    icon_name: &str,
    label: impl Into<SharedString>,
    theme: &Theme,
    danger: bool,
) -> Stateful<Div> {
    let fg = if danger { theme.danger } else { theme.text };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(24.))
        .px(px(10.))
        .rounded(px(6.))
        .bg(theme.control_bg)
        .text_color(fg)
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| s.bg(theme.control_hover))
        .active(|s| s.opacity(0.7))
        .child(icon(icon_name, 13., fg))
        .child(label.into())
}
