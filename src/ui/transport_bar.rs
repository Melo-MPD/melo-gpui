//! Bottom transport bar: seek slider, track info, shuffle/prev/play/next/repeat,
//! elapsed/remaining, volume, audio-format badge and server name.
//! Port of `MacTransportBar` including its local-first seek behaviour.

use crate::mpd::PlayState;
use crate::state::AppState;
use crate::state::library_index::format_time;
use crate::theme::Theme;
use crate::ui::audio_popovers::{render_outputs, render_signal_path, verdict_color};
use crate::ui::widgets::{icon, icon_button, popover, slider};
use gpui::{
    App, Bounds, Context, Corner, Entity, EventEmitter, ObjectFit, Pixels, Task, Window, canvas,
    div, img, prelude::*, px,
};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPopover {
    SignalPath,
    Outputs,
}

/// Events the bar raises for the window root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportEvent {
    /// User asked for a fix that lives in Settings › Local MPD.
    OpenLocalMpd,
}

pub struct TransportBar {
    state: Entity<AppState>,
    is_seeking: bool,
    seek_fraction: f32,
    /// Optimistic position held until the server confirms (or 3 s pass).
    seek_target: Option<f64>,
    seek_timeout: Option<Task<()>>,
    last_sent_volume: Option<i32>,
    popover: Option<BarPopover>,
    bar_bounds: Option<Bounds<Pixels>>,
}

impl EventEmitter<TransportEvent> for TransportBar {}

impl TransportBar {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |this, state, cx| {
            if let Some(target) = this.seek_target {
                if (state.read(cx).display_elapsed() - target).abs() < 2.0 {
                    this.seek_target = None;
                    this.seek_timeout = None;
                }
            }
            cx.notify();
        })
        .detach();
        TransportBar {
            state,
            is_seeking: false,
            seek_fraction: 0.,
            seek_target: None,
            seek_timeout: None,
            last_sent_volume: None,
            popover: None,
            bar_bounds: None,
        }
    }

    pub fn toggle_popover(&mut self, which: BarPopover, cx: &mut Context<Self>) {
        self.popover = if self.popover == Some(which) {
            None
        } else {
            Some(which)
        };
        cx.notify();
    }

    pub fn close_popover(&mut self, cx: &mut Context<Self>) {
        if self.popover.is_some() {
            self.popover = None;
            cx.notify();
        }
    }

    fn displayed_elapsed(&self, cx: &Context<Self>) -> f64 {
        let st = self.state.read(cx);
        if self.is_seeking {
            if let Some(d) = st.status.duration {
                return self.seek_fraction as f64 * d;
            }
        }
        self.seek_target.unwrap_or_else(|| st.display_elapsed())
    }
}

impl Render for TransportBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let st = self.state.read(cx);
        let status = st.status.clone();
        let song = st.current_song.clone();
        let art = st.cover_art.clone();
        let duration = status.duration.filter(|d| *d > 0.0);
        let displayed = self.displayed_elapsed(cx);
        let state = self.state.clone();

        let seek_row = if let Some(duration) = duration {
            let frac = if self.is_seeking {
                self.seek_fraction
            } else {
                (displayed / duration).clamp(0., 1.) as f32
            };
            div().px(px(20.)).pt(px(8.)).child(
                slider("seek", frac)
                    .thumb_size(px(12.))
                    .colors(theme.slider_track, theme.accent, theme.slider_thumb)
                    .on_change(cx.processor(|this, f: f32, _window, cx| {
                        this.is_seeking = true;
                        this.seek_fraction = f;
                        cx.notify();
                    }))
                    .on_release(cx.processor(move |this, f: f32, _window, cx| {
                        let target = f as f64 * duration;
                        this.is_seeking = false;
                        this.seek_target = Some(target);
                        this.state.read(cx).seek_to(target);
                        // Safety net: never hold an optimistic position > 3 s.
                        this.seek_timeout = Some(cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(Duration::from_secs(3)).await;
                            let _ = this.update(cx, |t, cx| {
                                t.seek_target = None;
                                cx.notify();
                            });
                        }));
                        cx.notify();
                    })),
            )
        } else {
            div().h(px(12.))
        };

        // Left: art + title/artist
        let left = div()
            .flex()
            .flex_1()
            .items_center()
            .gap(px(10.))
            .min_w(px(0.))
            .child(
                div()
                    .size(px(44.))
                    .flex_shrink_0()
                    .rounded(px(5.))
                    .overflow_hidden()
                    .bg(theme.placeholder_bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .map(|d| match art {
                        Some(image) => d.child(
                            img(image)
                                .size_full()
                                .rounded(px(5.))
                                .object_fit(ObjectFit::Cover),
                        ),
                        None => d.child(icon("music", 14., theme.text_secondary)),
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .truncate()
                            .child(
                                song.as_ref()
                                    .map(|s| s.display_title())
                                    .unwrap_or_else(|| "Not Playing".into()),
                            ),
                    )
                    .when_some(song.as_ref(), |d, s| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme.text_secondary)
                                .truncate()
                                .child(s.display_artist()),
                        )
                    }),
            );

        // Center: controls + time
        let is_playing = status.state == PlayState::Play;
        let random_color = if status.random_on {
            theme.accent
        } else {
            theme.text_secondary.opacity(0.7)
        };
        let repeat_color = if status.repeat_on {
            theme.accent
        } else {
            theme.text_secondary.opacity(0.7)
        };
        let repeat_icon = if status.repeat_on && status.single_mode != "0" {
            "repeat-1"
        } else {
            "repeat"
        };
        let s1 = state.clone();
        let s2 = state.clone();
        let s3 = state.clone();
        let s4 = state.clone();
        let s5 = state.clone();
        let center = div()
            .flex()
            .flex_1()
            .flex_col()
            .items_center()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        icon_button("shuffle", "shuffle", 14., random_color)
                            .on_click(move |_, _, cx| s1.read(cx).toggle_random()),
                    )
                    .child(
                        icon_button("prev", "skip-back", 17., theme.text)
                            .on_click(move |_, _, cx| s2.read(cx).previous()),
                    )
                    .child(
                        icon_button(
                            "play",
                            if is_playing {
                                "pause-circle"
                            } else {
                                "play-circle"
                            },
                            34.,
                            theme.text,
                        )
                        .on_click(move |_, _, cx| s3.read(cx).toggle_play_pause()),
                    )
                    .child(
                        icon_button("next", "skip-forward", 17., theme.text)
                            .on_click(move |_, _, cx| s4.read(cx).next()),
                    )
                    .child(
                        icon_button("repeat", repeat_icon, 14., repeat_color)
                            .on_click(move |_, _, cx| s5.read(cx).cycle_repeat()),
                    ),
            )
            .when_some(duration, |d, duration| {
                d.child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .text_size(px(10.))
                        .text_color(theme.text_tertiary)
                        .child(format_time(displayed))
                        .child("·")
                        .child(format!("-{}", format_time((duration - displayed).max(0.)))),
                )
            });

        // Right: Signal Path pill · Output button · Volume (or Fixed)
        let volume = status.volume;
        let bit_perfect = st.bit_perfect_mode;
        let drifted = st.bit_perfect_drifted();
        let sp = st.signal_path();
        let outputs = st.outputs.clone();
        let verdict_color = verdict_color(sp.verdict, &theme);
        let popover_open = self.popover;
        let s6 = state.clone();
        let enabled_outputs: Vec<String> = outputs
            .iter()
            .filter(|o| o.enabled)
            .map(|o| o.name.clone())
            .collect();
        let output_tooltip = if enabled_outputs.is_empty() {
            "No output".to_string()
        } else {
            enabled_outputs.join(" + ")
        };

        let volume_control = if bit_perfect {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .px(px(6.))
                .py(px(3.))
                .rounded(px(4.))
                .text_size(px(10.))
                .text_color(theme.text_tertiary)
                .child(icon("lock", 11., theme.text_tertiary))
                .child(if volume >= 0 { "Fixed 100 %" } else { "Fixed" })
                .into_any_element()
        } else if volume >= 0 {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon("volume", 12., theme.text_secondary))
                .child(
                    slider("volume", volume as f32 / 100.)
                        .w(px(88.))
                        .thumb_size(px(12.))
                        .colors(theme.slider_track, theme.accent, theme.slider_thumb)
                        .on_change(cx.processor(move |this, f: f32, _window, cx| {
                            let v = (f * 100.).round() as i32;
                            if this.last_sent_volume != Some(v) {
                                this.last_sent_volume = Some(v);
                                s6.read(cx).set_volume(v);
                            }
                        })),
                )
                .child(icon("volume-2", 12., theme.text_secondary))
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_size(px(10.))
                .text_color(theme.text_tertiary)
                .child(icon("lock", 11., theme.text_tertiary))
                .child("Fixed volume")
                .into_any_element()
        };

        let pill_active = popover_open == Some(BarPopover::SignalPath);
        let pill = div()
            .id("signal-path-pill")
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(if pill_active {
                theme.control_hover
            } else {
                theme.control_bg
            })
            .cursor_pointer()
            .hover(|d| d.bg(theme.control_hover))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_popover(BarPopover::SignalPath, cx)))
            .child(div().size(px(7.)).rounded_full().bg(verdict_color))
            .child(
                div()
                    .text_size(px(10.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .child(sp.pill.clone()),
            )
            .when(drifted, |d| {
                d.child(icon("triangle-alert", 11., theme.verdict_amber))
            });

        let out_active = popover_open == Some(BarPopover::Outputs);
        let output_label = st.output_label();
        let output_button = div()
            .id("outputs-button")
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(24.))
            .px(px(8.))
            .rounded(px(6.))
            .bg(if out_active {
                theme.control_hover
            } else {
                theme.control_bg
            })
            .cursor_pointer()
            .hover(|d| d.bg(theme.control_hover))
            .tooltip({
                let text = output_tooltip.clone();
                move |_, cx| {
                    let text = text.clone();
                    cx.new(|_| Tip(text)).into()
                }
            })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_popover(BarPopover::Outputs, cx)))
            .child(icon(
                "speaker",
                14.,
                if out_active {
                    theme.accent
                } else {
                    theme.text_secondary
                },
            ))
            .child(
                div()
                    .max_w(px(160.))
                    .text_size(px(10.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .truncate()
                    .child(output_label),
            )
            .child(icon("chevron-down", 10., theme.text_tertiary));

        let right = div()
            .flex()
            .flex_1()
            .items_center()
            .justify_end()
            .gap(px(10.))
            .child(volume_control)
            .child(pill)
            .child(output_button);

        // Popover (drawn above everything, anchored to the bar's top-right)
        let popover_el = self.popover.map(|which| {
            let bounds = self.bar_bounds.unwrap_or_else(|| {
                let vs = window.viewport_size();
                gpui::Bounds::new(
                    gpui::point(px(0.), vs.height - px(90.)),
                    gpui::size(vs.width, px(90.)),
                )
            });
            let position = gpui::point(bounds.right() - px(20.), bounds.top() - px(6.));
            let dismiss = cx.listener(|this, _: &(), _, cx| this.close_popover(cx));
            let content = match which {
                BarPopover::SignalPath => {
                    let open_local: std::rc::Rc<dyn Fn(&mut Window, &mut App)> = {
                        let l = cx.listener(|this, _: &(), _, cx| {
                            this.close_popover(cx);
                            cx.emit(TransportEvent::OpenLocalMpd);
                        });
                        std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                            l(&(), window, cx)
                        })
                    };
                    render_signal_path(&self.state, &sp, bit_perfect, drifted, &theme, open_local)
                }
                BarPopover::Outputs => {
                    let open_local: std::rc::Rc<dyn Fn(&mut Window, &mut App)> = {
                        let l = cx.listener(|this, _: &(), _, cx| {
                            this.close_popover(cx);
                            cx.emit(TransportEvent::OpenLocalMpd);
                        });
                        std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                            l(&(), window, cx)
                        })
                    };
                    render_outputs(&self.state, &outputs, &theme, Some(open_local), cx)
                }
            };
            popover(
                "bar-popover",
                position,
                Corner::BottomRight,
                px(if which == BarPopover::SignalPath {
                    440.
                } else {
                    340.
                }),
                &theme,
                move |window, cx| dismiss(&(), window, cx),
                content,
            )
        });

        let this = cx.entity().downgrade();
        let bounds_probe = canvas(
            move |bounds, _window, cx| {
                let _ = this.update(cx, |t, _| t.bar_bounds = Some(bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();

        div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .bg(theme.bar_bg)
            .border_t_1()
            .border_color(theme.separator)
            .child(bounds_probe)
            .child(seek_row)
            .child(
                div()
                    .flex()
                    .items_center()
                    .px(px(20.))
                    .pt(px(6.))
                    .pb(px(12.))
                    .child(left)
                    .child(center)
                    .child(right),
            )
            .children(popover_el)
    }
}

/// Minimal tooltip view.
struct Tip(String);

impl Render for Tip {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(theme.popover_bg)
            .border_1()
            .border_color(theme.separator)
            .shadow_md()
            .text_size(px(11.))
            .text_color(theme.text)
            .child(self.0.clone())
    }
}
