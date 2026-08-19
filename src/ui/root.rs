//! Window root: sidebar | detail, transport bar below (port of `MacContentView`).

use crate::actions::*;
use crate::mpd::ConnectionState;
use crate::state::AppState;
use crate::theme::{Theme, UI_FONT};
use crate::ui::library::LibraryView;
use crate::ui::now_playing::NowPlayingView;
use crate::ui::queue::QueueView;
use crate::ui::settings::SettingsView;
use crate::ui::transport_bar::{TransportBar, TransportEvent};
use crate::ui::widgets::icon;
use gpui::{
    AnyView, Context, Entity, FocusHandle, Focusable, MouseButton, Window, div, prelude::*, px,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarItem {
    NowPlaying,
    Queue,
    Library,
    Settings,
}

impl SidebarItem {
    const ALL: [SidebarItem; 4] = [
        SidebarItem::NowPlaying,
        SidebarItem::Queue,
        SidebarItem::Library,
        SidebarItem::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            SidebarItem::NowPlaying => "Now Playing",
            SidebarItem::Queue => "Queue",
            SidebarItem::Library => "Library",
            SidebarItem::Settings => "Settings",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            SidebarItem::NowPlaying => "music",
            SidebarItem::Queue => "list-music",
            SidebarItem::Library => "library",
            SidebarItem::Settings => "settings",
        }
    }
}

pub struct MeloRoot {
    state: Entity<AppState>,
    focus_handle: FocusHandle,
    selection: SidebarItem,
    transport: Entity<TransportBar>,
    now_playing: Entity<NowPlayingView>,
    queue: Entity<QueueView>,
    library: Entity<LibraryView>,
    settings: Entity<SettingsView>,
}

impl MeloRoot {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        cx.observe_window_appearance(window, |this, _, cx| {
            // Children re-render on state changes only; poke them so a
            // light/dark switch repaints everything immediately.
            this.transport.update(cx, |_, cx| cx.notify());
            this.now_playing.update(cx, |_, cx| cx.notify());
            this.queue.update(cx, |_, cx| cx.notify());
            this.library.update(cx, |_, cx| cx.notify());
            this.settings.update(cx, |_, cx| cx.notify());
            cx.notify();
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.state.read(cx).refresh_all();
            }
        })
        .detach();
        let transport = cx.new(|cx| TransportBar::new(state.clone(), cx));
        cx.subscribe(
            &transport,
            |this, _, event: &TransportEvent, cx| match event {
                TransportEvent::OpenLocalMpd => {
                    this.settings.update(cx, |s, cx| s.reveal_local_mpd(cx));
                    this.select(SidebarItem::Settings, cx);
                }
            },
        )
        .detach();
        let now_playing = cx.new(|cx| NowPlayingView::new(state.clone(), cx));
        let queue = cx.new(|cx| QueueView::new(state.clone(), cx));
        let library = cx.new(|cx| LibraryView::new(state.clone(), cx));
        let settings = cx.new(|cx| SettingsView::new(state.clone(), cx));
        MeloRoot {
            state,
            focus_handle: cx.focus_handle(),
            // Dev aid: MELO_SCREEN=queue|library|settings opens on that screen.
            selection: match std::env::var("MELO_SCREEN").as_deref() {
                Ok("queue") => SidebarItem::Queue,
                Ok("library") => SidebarItem::Library,
                Ok("settings") => SidebarItem::Settings,
                _ => SidebarItem::NowPlaying,
            },
            transport,
            now_playing,
            queue,
            library,
            settings,
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    fn select(&mut self, item: SidebarItem, cx: &mut Context<Self>) {
        self.selection = item;
        cx.notify();
    }

    fn detail(&self) -> AnyView {
        match self.selection {
            SidebarItem::NowPlaying => self.now_playing.clone().into(),
            SidebarItem::Queue => self.queue.clone().into(),
            SidebarItem::Library => self.library.clone().into(),
            SidebarItem::Settings => self.settings.clone().into(),
        }
    }
}

impl Focusable for MeloRoot {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MeloRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::mark("first_window");
        let theme = Theme::for_window(window);
        let st = self.state.read(cx);
        let queue_count = st.queue.len();
        let last_error = st.last_error.clone();
        let wake_notice = st.wake_notice.clone();
        let (dot, conn_label) = match (&st.active_profile, &st.connection_state) {
            (Some(p), ConnectionState::Connected) => (theme.success, p.name.clone()),
            (Some(_), ConnectionState::Connecting) => (theme.warning, "Connecting…".to_string()),
            (Some(_), ConnectionState::Error(e)) => (theme.danger, e.clone()),
            (Some(p), ConnectionState::Disconnected) => {
                (theme.text_tertiary, format!("{} (offline)", p.name))
            }
            (None, _) => (theme.text_tertiary, "Not connected".to_string()),
        };
        let (dot, conn_label) = match wake_notice {
            Some(n) => (theme.accent, n),
            None => (dot, conn_label),
        };
        let selection = self.selection;
        let focus_handle = self.focus_handle.clone();

        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(200.))
            .flex_shrink_0()
            .h_full()
            .bg(theme.sidebar_bg)
            .border_r_1()
            .border_color(theme.separator)
            // Title / drag region under the transparent titlebar.
            .child(
                div()
                    .id("sidebar-titlebar")
                    .h(px(84.))
                    .flex()
                    .items_end()
                    .gap(px(9.))
                    .pb(px(14.))
                    .pl(px(18.))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                    .child(
                        gpui::svg()
                            .path("icons/melo-logo.svg")
                            .w(px(28.))
                            .h(px(21.))
                            .flex_shrink_0()
                            .text_color(theme.text_tertiary),
                    )
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text_tertiary)
                            .line_height(px(21.))
                            .child("melo"),
                    ),
            );

        let mut items = div().flex().flex_col().gap(px(2.)).px(px(10.));
        for item in SidebarItem::ALL {
            let is_selected = item == selection;
            items = items.child(
                div()
                    .id(item.label())
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(is_selected, |d| d.bg(theme.accent))
                    .when(!is_selected, |d| d.hover(|d| d.bg(theme.sidebar_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(item, cx)))
                    .child(icon(
                        item.icon(),
                        15.,
                        if is_selected {
                            gpui::white()
                        } else {
                            theme.accent
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(if is_selected {
                                gpui::white()
                            } else {
                                theme.text
                            })
                            .child(item.label()),
                    )
                    .when(item == SidebarItem::Queue && queue_count > 0, |d| {
                        d.child(
                            div()
                                .px(px(6.))
                                .py(px(1.))
                                .rounded_full()
                                .bg(if is_selected {
                                    gpui::white().opacity(0.25)
                                } else {
                                    theme.badge_bg
                                })
                                .text_size(px(10.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(if is_selected {
                                    gpui::white()
                                } else {
                                    theme.badge_text
                                })
                                .child(queue_count.to_string()),
                        )
                    }),
            );
        }
        sidebar = sidebar.child(items).child(div().flex_1()).child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(16.))
                .py(px(10.))
                .border_t_1()
                .border_color(theme.separator)
                .child(div().size(px(7.)).rounded_full().bg(dot).flex_shrink_0())
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.text_secondary)
                        .truncate()
                        .child(conn_label),
                ),
        );

        let detail = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .when_some(last_error, |d, err| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(16.))
                        .py(px(6.))
                        .bg(theme.danger.opacity(0.12))
                        .border_b_1()
                        .border_color(theme.danger.opacity(0.3))
                        .text_size(px(12.))
                        .text_color(theme.danger)
                        .child(icon("x", 12., theme.danger))
                        .child(err),
                )
            })
            .child(div().flex_1().min_h(px(0.)).child(self.detail()));

        div()
            .key_context("Melo")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &TogglePlayPause, _, cx| {
                this.state.read(cx).toggle_play_pause()
            }))
            .on_action(cx.listener(|this, _: &NextTrack, _, cx| this.state.read(cx).next()))
            .on_action(cx.listener(|this, _: &PrevTrack, _, cx| this.state.read(cx).previous()))
            .on_action(cx.listener(|this, _: &VolumeUp, _, cx| this.state.read(cx).volume_delta(5)))
            .on_action(
                cx.listener(|this, _: &VolumeDown, _, cx| this.state.read(cx).volume_delta(-5)),
            )
            .on_action(cx.listener(|this, _: &ShowNowPlaying, _, cx| {
                this.select(SidebarItem::NowPlaying, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowQueue, _, cx| this.select(SidebarItem::Queue, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowLibrary, _, cx| this.select(SidebarItem::Library, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowSettings, _, cx| this.select(SidebarItem::Settings, cx)),
            )
            .on_action(cx.listener(|this, _: &Escape, _, cx| {
                this.transport.update(cx, |t, cx| t.close_popover(cx))
            }))
            // Clicking anywhere that isn't a text field returns keyboard focus
            // to the root so Space / ⌘← / ⌘→ keep working.
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                if !focus_handle.is_focused(window) {
                    window.focus(&focus_handle);
                }
            })
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.window_bg)
            .font_family(UI_FONT)
            .text_color(theme.text)
            .text_size(px(13.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(sidebar)
                    .child(detail),
            )
            .child(self.transport.clone())
    }
}
