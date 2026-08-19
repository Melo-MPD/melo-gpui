//! Read-only Now Playing screen: blurred art backdrop, large art, titles.

use crate::state::AppState;
use crate::theme::Theme;
use crate::ui::widgets::icon;
use gpui::{Context, Entity, ObjectFit, Window, div, img, prelude::*, px};

pub struct NowPlayingView {
    state: Entity<AppState>,
}

impl NowPlayingView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        NowPlayingView { state }
    }
}

impl Render for NowPlayingView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let st = self.state.read(cx);
        let song = st.current_song.clone();
        let art = st.cover_art.clone();
        let backdrop = st.backdrop.clone();

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme.window_bg)
            // Backdrop
            .when_some(backdrop, |d, bd| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .opacity(0.4)
                        .child(img(bd).size_full().object_fit(ObjectFit::Cover)),
                )
            })
            .child(div().absolute().inset_0().bg(theme.scrim))
            // Content
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(20.))
                    .px(px(40.))
                    .child(
                        div()
                            .size(px(272.))
                            .rounded(px(12.))
                            .overflow_hidden()
                            .bg(theme.placeholder_bg)
                            .shadow_2xl()
                            .flex()
                            .items_center()
                            .justify_center()
                            .map(|d| match art {
                                Some(image) => d.child(
                                    img(image)
                                        .size_full()
                                        .rounded(px(12.))
                                        .object_fit(ObjectFit::Cover),
                                ),
                                None => d.child(icon("music", 64., theme.text_tertiary)),
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .w(px(520.))
                            .child(
                                div()
                                    .w(px(520.))
                                    .text_size(px(20.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .text_center()
                                    .line_clamp(2)
                                    .child(
                                        song.as_ref()
                                            .map(|s| s.display_title())
                                            .unwrap_or_else(|| "Not Playing".into()),
                                    ),
                            )
                            .when_some(song.as_ref(), |d, s| {
                                d.child(
                                    div()
                                        .w(px(520.))
                                        .text_size(px(15.))
                                        .text_color(theme.text_secondary)
                                        .text_center()
                                        .truncate()
                                        .child(s.display_artist()),
                                )
                                .when_some(
                                    s.album.clone(),
                                    |d, album| {
                                        d.child(
                                            div()
                                                .w(px(520.))
                                                .text_size(px(13.))
                                                .text_color(theme.text_tertiary)
                                                .text_center()
                                                .truncate()
                                                .child(album),
                                        )
                                    },
                                )
                            }),
                    ),
            )
    }
}
