//! About window: icon, name + version, commit, full version string, OK / Copy.

use crate::dock_icon::rounded_icon_png;
use crate::theme::{Theme, UI_FONT};
use gpui::{
    App, Bounds, ClipboardItem, Context, Global, Image, ImageFormat, ObjectFit, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, div, img, prelude::*, px,
    size,
};
use std::sync::Arc;

pub const APP_NAME: &str = "Melo";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_SHA: &str = env!("MELO_GIT_SHA");

pub fn full_version() -> String {
    let short: String = GIT_SHA.chars().take(9).collect();
    format!("{VERSION}+gpui.{short}")
}

fn about_text() -> String {
    format!(
        "{APP_NAME} {VERSION}\nCommit: {GIT_SHA}\nVersion: {}",
        full_version()
    )
}

struct AboutWindow(Option<WindowHandle<AboutView>>);
impl Global for AboutWindow {}

pub struct AboutView {
    icon: Arc<Image>,
}

/// Opens (or brings forward) the single About window.
pub fn open(cx: &mut App) {
    if let Some(existing) = cx.try_global::<AboutWindow>().and_then(|w| w.0) {
        if existing
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
    }
    let bounds = Bounds::centered(None, size(px(420.), px(400.)), cx);
    let handle = cx
        .open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from(format!("About {APP_NAME}"))),
                    appears_transparent: true,
                    traffic_light_position: Some(gpui::point(px(12.), px(12.))),
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                is_resizable: false,
                is_minimizable: false,
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| AboutView {
                    icon: Arc::new(Image::from_bytes(
                        ImageFormat::Png,
                        rounded_icon_png().to_vec(),
                    )),
                })
            },
        )
        .ok();
    cx.set_global(AboutWindow(handle));
}

fn button(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .h(px(32.))
        .rounded(px(7.))
        .bg(theme.control_bg)
        .border_1()
        .border_color(theme.control_border)
        .text_size(px(14.))
        .text_color(theme.text)
        .cursor_pointer()
        .hover(|d| d.bg(theme.control_hover))
        .active(|d| d.opacity(0.7))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(label)
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let label = |t: &str| {
            div()
                .text_size(px(12.))
                .text_color(theme.text_tertiary)
                .child(t.to_owned())
        };
        let value = |t: String| div().text_size(px(14.)).text_color(theme.text).child(t);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(theme.window_bg)
            .font_family(UI_FONT)
            .text_color(theme.text)
            .pt(px(56.))
            .pb(px(20.))
            .px(px(28.))
            .gap(px(10.))
            .child(
                div().size(px(96.)).child(
                    img(self.icon.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                ),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(22.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("{APP_NAME} {VERSION}")),
            )
            .child(label("Commit"))
            .child(value(GIT_SHA.to_owned()))
            .child(label("Version"))
            .child(value(full_version()))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .w_full()
                    .gap(px(8.))
                    .child(button("about-ok", "OK", &theme, |window, _| {
                        window.remove_window()
                    }))
                    .child(button("about-copy", "Copy", &theme, |_, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(about_text()))
                    })),
            )
    }
}
