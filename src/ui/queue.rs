//! Queue list (port of `MacQueueView`): click a row to play it, Clear button.

use crate::state::AppState;
use crate::state::library_index::format_time;
use crate::theme::Theme;
use crate::ui::widgets::{icon, icon_text_button};
use gpui::{Context, Entity, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list};

pub struct QueueView {
    state: Entity<AppState>,
    scroll: UniformListScrollHandle,
}

impl QueueView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        QueueView {
            state,
            scroll: UniformListScrollHandle::new(),
        }
    }
}

impl Render for QueueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let st = self.state.read(cx);
        let count = st.queue.len();
        let loaded = st.queue_loaded;
        let connected = st.is_connected();
        let state = self.state.clone();

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(44.))
            .px(px(16.))
            .border_b_1()
            .border_color(theme.separator)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("Queue"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.text_tertiary)
                            .child(format!(
                                "{count} {}",
                                if count == 1 { "song" } else { "songs" }
                            )),
                    ),
            )
            .child({
                let s = state.clone();
                icon_text_button("clear-queue", "trash-2", "Clear", &theme, true)
                    .on_click(move |_, _, cx| s.read(cx).clear_queue())
            });

        let body = if count == 0 {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(icon("list-music", 40., theme.text_tertiary))
                .child(
                    div()
                        .text_size(px(17.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child("Queue is Empty"),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.text_secondary)
                        .child(if !connected {
                            "Connect to an MPD server to see its queue."
                        } else if loaded {
                            "Add songs from the Library to start playing."
                        } else {
                            "Loading…"
                        }),
                )
                .into_any_element()
        } else {
            uniform_list(
                "queue-rows",
                count,
                cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                    let theme = Theme::for_window(window);
                    let st = this.state.read(cx);
                    let current_id = st.status.current_song_id;
                    let songs: Vec<_> = st.queue[range.clone()].to_vec();
                    let state = this.state.clone();
                    songs
                        .into_iter()
                        .zip(range)
                        .map(|(song, ix)| {
                            let is_current = song.queue_id.is_some() && song.queue_id == current_id;
                            let queue_id = song.queue_id;
                            let s = state.clone();
                            div()
                                .id(ix)
                                .w_full()
                                .flex()
                                .items_center()
                                .h(px(46.))
                                .px(px(16.))
                                .gap(px(12.))
                                .cursor_pointer()
                                .when(is_current, |d| d.bg(theme.accent_soft))
                                .hover(|d| d.bg(theme.sidebar_hover))
                                .on_click(move |_, _, cx| {
                                    if let Some(id) = queue_id {
                                        s.read(cx).play_id(id);
                                    }
                                })
                                .child(
                                    div()
                                        .w(px(22.))
                                        .flex()
                                        .justify_center()
                                        .flex_shrink_0()
                                        .map(|d| {
                                            if is_current {
                                                d.child(icon("audio-lines", 14., theme.accent))
                                            } else {
                                                d.text_size(px(12.))
                                                    .text_color(theme.text_tertiary)
                                                    .child(
                                                        song.track
                                                            .as_deref()
                                                            .and_then(|t| t.split('/').next())
                                                            .map(str::to_owned)
                                                            .unwrap_or_else(|| {
                                                                (ix + 1).to_string()
                                                            }),
                                                    )
                                            }
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .text_color(if is_current {
                                                    theme.accent
                                                } else {
                                                    theme.text
                                                })
                                                .font_weight(if is_current {
                                                    gpui::FontWeight::SEMIBOLD
                                                } else {
                                                    gpui::FontWeight::NORMAL
                                                })
                                                .truncate()
                                                .child(song.display_title()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(theme.text_secondary)
                                                .truncate()
                                                .child(song.display_artist()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.text_tertiary)
                                        .child(format_time(song.duration)),
                                )
                        })
                        .collect()
                }),
            )
            .track_scroll(self.scroll.clone())
            .flex_1()
            .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.window_bg)
            .child(header)
            .child(body)
    }
}
