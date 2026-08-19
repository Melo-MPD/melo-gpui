//! Segmented control (macOS-style), e.g. ReplayGain Off / Track / Album / Auto.

use crate::theme::Theme;
use gpui::{App, Div, ElementId, SharedString, Window, div, prelude::*, px};
use std::rc::Rc;

pub fn segmented(
    id: impl Into<ElementId>,
    options: &[(&'static str, SharedString)],
    selected: &str,
    theme: &Theme,
    on_select: impl Fn(&'static str, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let base: ElementId = id.into();
    let mut root = div()
        .flex()
        .p(px(2.))
        .rounded(px(7.))
        .bg(theme.control_bg)
        .gap(px(1.));
    for (value, label) in options {
        let is_sel = *value == selected;
        let cb = on_select.clone();
        let value = *value;
        root = root.child(
            div()
                .id((base.clone(), value))
                .px(px(10.))
                .py(px(2.))
                .rounded(px(5.))
                .text_size(px(12.))
                .cursor_pointer()
                .when(is_sel, |d| {
                    d.bg(theme.segment_selected)
                        .shadow_sm()
                        .text_color(theme.text)
                })
                .when(!is_sel, |d| {
                    d.text_color(theme.text_secondary)
                        .hover(|d| d.bg(theme.control_hover))
                })
                .on_click(move |_, window, cx| cb(value, window, cx))
                .child(label.clone()),
        );
    }
    root
}
