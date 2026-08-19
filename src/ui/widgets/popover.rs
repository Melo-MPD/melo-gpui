//! Popover container: a floating card anchored to a window point, drawn above
//! everything via `deferred(anchored(..))`, dismissed on outside click.

use crate::theme::Theme;
use gpui::{
    AnyElement, App, Corner, Deferred, Pixels, Point, Window, anchored, deferred, div, prelude::*,
    px,
};

pub fn popover(
    id: &'static str,
    position: Point<Pixels>,
    anchor: Corner,
    width: Pixels,
    theme: &Theme,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    content: AnyElement,
) -> Deferred {
    deferred(
        anchored()
            .position(position)
            .anchor(anchor)
            .snap_to_window_with_margin(px(8.))
            .child(
                div()
                    .id(id)
                    .occlude()
                    .w(width)
                    .flex()
                    .flex_col()
                    .rounded(px(10.))
                    .bg(theme.popover_bg)
                    .border_1()
                    .border_color(theme.separator)
                    .shadow_lg()
                    .text_color(theme.text)
                    .text_size(px(12.))
                    .on_mouse_down_out(move |_, window, cx| on_dismiss(window, cx))
                    .child(content),
            ),
    )
    .with_priority(1)
}
