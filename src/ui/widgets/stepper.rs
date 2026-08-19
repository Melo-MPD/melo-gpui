use crate::theme::Theme;
use crate::ui::widgets::icon;
use gpui::{App, Div, ElementId, SharedString, Window, div, prelude::*, px};
use std::rc::Rc;

/// `[-] value [+]` control.
pub fn stepper(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    can_decrement: bool,
    can_increment: bool,
    on_step: impl Fn(i32, &mut Window, &mut App) + 'static,
) -> Div {
    let on_step = Rc::new(on_step);
    let dec = on_step.clone();
    let inc = on_step;
    let base_id: ElementId = id.into();
    let dec_color = if can_decrement {
        theme.text
    } else {
        theme.text_tertiary
    };
    let inc_color = if can_increment {
        theme.text
    } else {
        theme.text_tertiary
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .min_w(px(36.))
                .text_size(px(13.))
                .text_color(theme.text)
                .child(label.into()),
        )
        .child(
            div()
                .flex()
                .rounded(px(6.))
                .bg(theme.control_bg)
                .child(
                    div()
                        .id((base_id.clone(), "dec"))
                        .px(px(6.))
                        .py(px(3.))
                        .cursor_pointer()
                        .rounded_l(px(6.))
                        .hover(|s| s.bg(gpui::opaque_grey(0.5, 0.15)))
                        .on_click(move |_, window, cx| {
                            if can_decrement {
                                dec(-1, window, cx)
                            }
                        })
                        .child(icon("minus", 12., dec_color)),
                )
                .child(div().w(px(1.)).bg(theme.control_border))
                .child(
                    div()
                        .id((base_id, "inc"))
                        .px(px(6.))
                        .py(px(3.))
                        .cursor_pointer()
                        .rounded_r(px(6.))
                        .hover(|s| s.bg(gpui::opaque_grey(0.5, 0.15)))
                        .on_click(move |_, window, cx| {
                            if can_increment {
                                inc(1, window, cx)
                            }
                        })
                        .child(icon("plus", 12., inc_color)),
                ),
        )
}
