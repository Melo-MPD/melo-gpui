use crate::theme::Theme;
use gpui::{App, Div, ElementId, Stateful, Window, div, prelude::*, px};

/// macOS-style switch.
pub fn toggle(
    id: impl Into<ElementId>,
    on: bool,
    theme: &Theme,
    on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let bg = if on {
        theme.accent
    } else {
        theme.control_border
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .w(px(38.))
        .h(px(22.))
        .p(px(2.))
        .rounded_full()
        .bg(bg)
        .cursor_pointer()
        .when(on, |d| d.justify_end())
        .when(!on, |d| d.justify_start())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_toggle(!on, window, cx)
        })
        .child(
            div()
                .size(px(18.))
                .rounded_full()
                .bg(gpui::white())
                .shadow_sm(),
        )
}
