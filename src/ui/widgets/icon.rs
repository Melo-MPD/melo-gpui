use gpui::{Hsla, Svg, prelude::*, px, svg};

/// An embedded Lucide icon tinted with `color`.
pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_shrink_0()
        .text_color(color)
}
