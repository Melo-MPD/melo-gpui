//! Library: album grid (left) + album detail with tracks (right).
//! Port of `MacLibraryView`.

use crate::mpd::Song;
use crate::state::AppState;
use crate::state::library_index::{self, LibraryAlbum, format_time};
use crate::theme::Theme;
use crate::ui::widgets::{InputEvent, TextInput, icon, icon_button, text_button};
use gpui::{
    Context, Entity, ObjectFit, UniformListScrollHandle, Window, div, img, prelude::*, px,
    uniform_list,
};

const PANE_WIDTH: f32 = 360.;
const PANE_PADDING: f32 = 16.;
const CELL_GAP: f32 = 16.;
const COLUMNS: usize = 2;
const CELL_WIDTH: f32 =
    (PANE_WIDTH - 2. * PANE_PADDING - CELL_GAP * (COLUMNS as f32 - 1.)) / COLUMNS as f32;
const ROW_HEIGHT: f32 = CELL_WIDTH + 72.;

pub struct LibraryView {
    state: Entity<AppState>,
    search: Entity<TextInput>,
    filter: String,
    selected_album: Option<String>,
    album_songs: Vec<Song>,
    songs_version: u64,
    grid_scroll: UniformListScrollHandle,
    tracks_scroll: UniformListScrollHandle,
}

impl LibraryView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new(cx, "Search albums"));
        cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
            if *event == InputEvent::Change {
                this.filter = input.read(cx).text().trim().to_lowercase();
                cx.notify();
            }
        })
        .detach();
        cx.observe(&state, |this, state, cx| {
            this.ensure_loaded(cx);
            let version = state.read(cx).library_version;
            if version != this.songs_version {
                this.recompute_songs(cx);
            }
            cx.notify();
        })
        .detach();
        let mut view = LibraryView {
            state,
            search,
            filter: String::new(),
            selected_album: None,
            album_songs: Vec::new(),
            songs_version: 0,
            grid_scroll: UniformListScrollHandle::new(),
            tracks_scroll: UniformListScrollHandle::new(),
        };
        view.ensure_loaded(cx);
        view
    }

    fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        let needs = {
            let s = self.state.read(cx);
            s.is_connected() && !s.library_loaded && !s.library_loading
        };
        if needs {
            self.state.update(cx, |s, cx| s.load_library(cx));
        }
    }

    fn recompute_songs(&mut self, cx: &mut Context<Self>) {
        let st = self.state.read(cx);
        self.songs_version = st.library_version;
        self.album_songs = match &self.selected_album {
            Some(name) => library_index::songs_in_album(name, &st.library)
                .into_iter()
                .cloned()
                .collect(),
            None => Vec::new(),
        };
    }

    fn select_album(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.selected_album = name;
        self.recompute_songs(cx);
        self.tracks_scroll = UniformListScrollHandle::new();
        cx.notify();
    }

    fn filtered_albums(&self, cx: &Context<Self>) -> Vec<LibraryAlbum> {
        let st = self.state.read(cx);
        if self.filter.is_empty() {
            return st.albums.clone();
        }
        st.albums
            .iter()
            .filter(|a| {
                a.name.to_lowercase().contains(&self.filter)
                    || a.artist
                        .as_deref()
                        .map(|x| x.to_lowercase().contains(&self.filter))
                        .unwrap_or(false)
            })
            .cloned()
            .collect()
    }
}

fn art_box(
    image: Option<Option<std::sync::Arc<gpui::Image>>>,
    size: f32,
    radius: f32,
    icon_size: f32,
    theme: &Theme,
) -> gpui::Div {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(radius))
        .overflow_hidden()
        .bg(theme.placeholder_bg)
        .flex()
        .items_center()
        .justify_center()
        .map(|d| match image {
            Some(Some(img_)) => d.child(
                img(img_)
                    .size_full()
                    .rounded(px(radius))
                    .object_fit(ObjectFit::Cover),
            ),
            _ => d.child(icon("music", icon_size, theme.text_tertiary)),
        })
}

impl Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let albums = self.filtered_albums(cx);
        let (loading, loaded, connected) = {
            let s = self.state.read(cx);
            (s.library_loading, s.library_loaded, s.is_connected())
        };
        let rows = albums.len().div_ceil(COLUMNS);
        let no_albums = albums.is_empty();
        let selected = self.selected_album.clone();
        let state = self.state.clone();

        // ---- Left pane -----------------------------------------------------
        let search_row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(PANE_PADDING))
            .h(px(44.))
            .border_b_1()
            .border_color(theme.separator)
            .child(div().flex_1().child(self.search.clone()))
            .child({
                let s = state.clone();
                icon_button("refresh-library", "refresh-cw", 14., theme.text_secondary)
                    .on_click(move |_, _, cx| s.update(cx, |s, cx| s.load_library(cx)))
            });

        let grid: gpui::AnyElement = if rows == 0 {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .px(px(24.))
                .child(icon("library", 36., theme.text_tertiary))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.text_secondary)
                        .text_center()
                        .child(if !connected {
                            "Connect to a server to browse its library.".to_string()
                        } else if loading {
                            "Loading library…".to_string()
                        } else if loaded && !self.filter.is_empty() {
                            "No albums match your search.".to_string()
                        } else if loaded {
                            "The library is empty.".to_string()
                        } else {
                            "Loading library…".to_string()
                        }),
                )
                .into_any_element()
        } else {
            let albums = std::rc::Rc::new(albums);
            uniform_list(
                "album-grid",
                rows,
                cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                    let theme = Theme::for_window(window);
                    let selected = this.selected_album.clone();
                    let mut out = Vec::new();
                    for row in range {
                        let start = row * COLUMNS;
                        let end = (start + COLUMNS).min(albums.len());
                        let cells = albums[start..end].to_vec();
                        // Ensure art is requested for visible cells.
                        this.state.update(cx, |s, _| {
                            for a in &cells {
                                s.ensure_album_art(a);
                            }
                        });
                        let art_map = &this.state.read(cx).album_art;
                        let mut row_div = div()
                            .id(row)
                            .flex()
                            .gap(px(CELL_GAP))
                            .px(px(PANE_PADDING))
                            .h(px(ROW_HEIGHT));
                        for album in cells {
                            let is_selected = selected.as_deref() == Some(album.name.as_str());
                            let image = art_map.get(&album.name).cloned();
                            let name = album.name.clone();
                            let artist = album
                                .artist
                                .clone()
                                .unwrap_or_else(|| "Unknown Artist".into());
                            let name_for_click = name.clone();
                            row_div = row_div.child(
                                div()
                                    .id(gpui::SharedString::from(name.clone()))
                                    .flex()
                                    .flex_col()
                                    .w(px(CELL_WIDTH))
                                    .gap(px(6.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_album(Some(name_for_click.clone()), cx)
                                    }))
                                    .child(
                                        div()
                                            .rounded(px(10.))
                                            .p(px(2.5))
                                            .when(is_selected, |d| {
                                                d.border_2().border_color(theme.accent).p(px(0.5))
                                            })
                                            .child(art_box(
                                                image,
                                                CELL_WIDTH - 5.,
                                                8.,
                                                32.,
                                                &theme,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .line_clamp(2)
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(theme.text_secondary)
                                            .truncate()
                                            .child(artist),
                                    ),
                            );
                        }
                        out.push(row_div);
                    }
                    out
                }),
            )
            .track_scroll(self.grid_scroll.clone())
            .flex_1()
            .pt(px(PANE_PADDING))
            .into_any_element()
        };

        let left = div()
            .flex()
            .flex_col()
            .w(px(PANE_WIDTH))
            .flex_shrink_0()
            .h_full()
            .border_r_1()
            .border_color(theme.separator)
            .child(search_row)
            .child(grid);

        // ---- Right pane ----------------------------------------------------
        let right = match selected {
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(px(15.))
                        .text_color(theme.text_tertiary)
                        .child(if no_albums && !loaded {
                            ""
                        } else {
                            "Select an album"
                        }),
                )
                .into_any_element(),
            Some(name) => {
                let album = self
                    .state
                    .read(cx)
                    .albums
                    .iter()
                    .find(|a| a.name == name)
                    .cloned();
                let image = self.state.read(cx).album_art.get(&name).cloned();
                let songs = &self.album_songs;
                let song_refs: Vec<&Song> = songs.iter().collect();
                let summary = library_index::album_summary(&song_refs);
                let artist = album
                    .as_ref()
                    .and_then(|a| a.artist.clone())
                    .or_else(|| songs.first().map(|s| s.display_artist()))
                    .unwrap_or_default();
                let n_play = name.clone();
                let n_add = name.clone();
                let s_play = state.clone();
                let s_add = state.clone();
                let header = div()
                    .flex()
                    .gap(px(20.))
                    .p(px(24.))
                    .border_b_1()
                    .border_color(theme.separator)
                    .child(art_box(image, 160., 10., 40., &theme).shadow_md())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .justify_end()
                            .gap(px(4.))
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .line_clamp(2)
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .text_color(theme.text_secondary)
                                    .truncate()
                                    .child(artist),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.text_tertiary)
                                    .child(summary),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .mt(px(10.))
                                    .child(
                                        text_button("play-album", "Play", &theme, true).on_click(
                                            move |_, _, cx| s_play.read(cx).play_album(&n_play),
                                        ),
                                    )
                                    .child(
                                        text_button("add-album", "Add to Queue", &theme, false)
                                            .on_click(move |_, _, cx| {
                                                s_add.read(cx).add_album(&n_add)
                                            }),
                                    ),
                            ),
                    );

                let count = songs.len();
                let tracks = uniform_list(
                    "album-tracks",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, window, _cx| {
                        let theme = Theme::for_window(window);
                        let state = this.state.clone();
                        this.album_songs[range.clone()]
                            .iter()
                            .cloned()
                            .zip(range)
                            .map(|(song, ix)| {
                                let uri = song.uri.clone();
                                let s = state.clone();
                                div()
                                    .id(ix)
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .h(px(40.))
                                    .px(px(24.))
                                    .gap(px(12.))
                                    .cursor_pointer()
                                    .hover(|d| d.bg(theme.sidebar_hover))
                                    .on_click(move |_, _, cx| s.read(cx).play_uri(&uri))
                                    .child(
                                        div()
                                            .w(px(22.))
                                            .text_size(px(12.))
                                            .text_color(theme.text_tertiary)
                                            .text_right()
                                            .child(
                                                song.track
                                                    .as_deref()
                                                    .and_then(|t| t.split('/').next())
                                                    .map(str::to_owned)
                                                    .unwrap_or_else(|| (ix + 1).to_string()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .text_size(px(13.))
                                            .text_color(theme.text)
                                            .truncate()
                                            .child(song.display_title()),
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
                .track_scroll(self.tracks_scroll.clone())
                .flex_1()
                .py(px(8.));

                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(header)
                    .child(tracks)
                    .into_any_element()
            }
        };

        div()
            .flex()
            .size_full()
            .bg(theme.window_bg)
            .child(left)
            .child(right)
    }
}
