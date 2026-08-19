//! Contents of the two transport-bar popovers: Signal Path and Outputs.

use crate::mpd::Output;
use crate::state::AppState;
use crate::state::signal_path::{SignalPath, Verdict};
use crate::theme::Theme;
use crate::ui::widgets::{icon, text_button, toggle};
use gpui::{AnyElement, App, Div, Entity, Hsla, SharedString, Window, div, prelude::*, px};
use std::rc::Rc;

/// Popover is 440 px wide with 14 px padding; leave room for the icon column.
const REASON_TEXT_WIDTH: f32 = 440. - 28. - 17. - 16.;

pub fn verdict_color(v: Verdict, theme: &Theme) -> Hsla {
    match v {
        Verdict::BitPerfect => theme.verdict_green,
        Verdict::Lossless => theme.verdict_blue,
        Verdict::Altered => theme.verdict_amber,
        Verdict::Unknown => theme.text_tertiary,
    }
}

fn row(label: &str, value: impl IntoElement, theme: &Theme) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .w(px(76.))
                .flex_shrink_0()
                .pt(px(1.))
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_tertiary)
                .child(label.to_uppercase()),
        )
        .child(div().flex_1().min_w(px(0.)).child(value))
}

fn value_text(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(px(12.))
        .text_color(theme.text)
        .child(text.into())
}

/// Signal Path card body.
pub fn render_signal_path(
    state: &Entity<AppState>,
    sp: &SignalPath,
    bit_perfect: bool,
    drifted: bool,
    theme: &Theme,
    open_local_mpd: Rc<dyn Fn(&mut Window, &mut App)>,
) -> AnyElement {
    let color = verdict_color(sp.verdict, theme);

    // Processing line: chips, altering ones tinted amber
    let mut processing = div().flex().flex_wrap().gap(px(6.));
    for stage in &sp.processing {
        let (bg, fg) = if stage.altering {
            (theme.verdict_amber.opacity(0.18), theme.verdict_amber)
        } else {
            (theme.control_bg, theme.text_secondary)
        };
        processing = processing.child(
            div()
                .px(px(6.))
                .py(px(2.))
                .rounded(px(4.))
                .bg(bg)
                .text_size(px(11.))
                .text_color(fg)
                .child(stage.label.clone()),
        );
    }

    let mut card = div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(14.))
        // Header
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_secondary)
                        .child("SIGNAL PATH"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(div().size(px(9.)).rounded_full().bg(color))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(color)
                                .child(sp.verdict.label()),
                        ),
                ),
        )
        .child(div().h(px(1.)).bg(theme.separator))
        .child(row(
            "Source",
            value_text(sp.source.clone().unwrap_or_else(|| "—".into()), theme),
            theme,
        ))
        .child(row(
            "Decoder",
            value_text(sp.decoder.clone().unwrap_or_else(|| "—".into()), theme),
            theme,
        ))
        .child(row("Processing", processing, theme))
        .child(row(
            "Output",
            value_text(
                sp.output
                    .clone()
                    .unwrap_or_else(|| "No outputs reported".into()),
                theme,
            ),
            theme,
        ));

    if !sp.reasons.is_empty() {
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .w_full()
            .p(px(8.))
            .rounded(px(6.))
            .bg(theme.verdict_amber.opacity(0.10));
        for r in &sp.reasons {
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .child(
                        div()
                            .pt(px(1.))
                            .child(icon("triangle-alert", 11., theme.verdict_amber)),
                    )
                    // Fixed width so the text measurer wraps instead of overflowing.
                    .child(
                        div()
                            .w(px(REASON_TEXT_WIDTH))
                            .text_size(px(11.))
                            .text_color(theme.text)
                            .child(r.clone()),
                    ),
            );
        }
        if sp.fixes.is_empty() && sp.verdict == Verdict::Altered {
            list = list.child(
                div()
                    .pl(px(17.))
                    .w(px(REASON_TEXT_WIDTH))
                    .text_size(px(10.))
                    .text_color(theme.text_tertiary)
                    .child("Nothing to fix on the MPD side — this is a hardware or transport limit (try a file the device can play natively, or another output)."),
            );
        }
        card = card.child(list);
    }
    if !sp.unknowns.is_empty() {
        let mut list = div().flex().flex_col().gap(px(2.));
        for u in &sp.unknowns {
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.))
                    .child(icon("info", 11., theme.text_tertiary))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .text_color(theme.text_tertiary)
                            .child(u.clone()),
                    ),
            );
        }
        card = card.child(list);
    }

    if !sp.fixes.is_empty() {
        let mut fixes = div().flex().flex_wrap().gap(px(6.));
        for (i, fix) in sp.fixes.iter().enumerate() {
            let action = fix.action.clone();
            let s = state.clone();
            let open = open_local_mpd.clone();
            fixes = fixes.child(
                text_button(("fix", i), fix.label.clone(), theme, false).on_click(
                    move |_, window, cx| {
                        let open_local = s.read(cx).apply_fix(&action);
                        if open_local {
                            open(window, cx);
                        }
                    },
                ),
            );
        }
        card = card.child(div().h(px(1.)).bg(theme.separator)).child(fixes);
    }

    // Bit-perfect mode row
    let s = state.clone();
    let s2 = state.clone();
    card = card.child(div().h(px(1.)).bg(theme.separator)).child(
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(icon("lock", 13., if bit_perfect { theme.accent } else { theme.text_tertiary }))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(12.)).text_color(theme.text).child("Bit-perfect mode"))
                    .child(div().w(px(260.)).text_size(px(10.)).text_color(theme.text_tertiary).child(
                        if bit_perfect && drifted {
                            "Server drifted — ReplayGain, crossfade, MixRamp or volume changed. Re-apply to lock them again."
                        } else if bit_perfect && sp.verdict == Verdict::Altered {
                            "MPD's side is clean (no DSP, fixed volume). What's left is outside MPD — see Why above."
                        } else if bit_perfect {
                            "Keeps ReplayGain, crossfade and MixRamp off and the volume at 100 %."
                        } else {
                            "Turn on to keep ReplayGain, crossfade and MixRamp off and the volume at 100 %."
                        },
                    )),
            )
            .when(drifted, |d| {
                d.child(
                    text_button("reapply-bp", "Re-apply", theme, true)
                        .on_click(move |_, _, cx| s2.read(cx).apply_bit_perfect()),
                )
            })
            .child(toggle("bp-toggle", bit_perfect, theme, move |on, _, cx| {
                s.update(cx, |s, cx| s.set_bit_perfect(on, cx))
            })),
    );

    card.into_any_element()
}

/// Outputs card body: click a row = route-to (exclusive), switch = enable/disable.
pub fn render_outputs(
    state: &Entity<AppState>,
    outputs: &[Output],
    theme: &Theme,
    open_local_mpd: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    cx: &App,
) -> AnyElement {
    let st = state.read(cx);
    let has_local = st.has_local_osx_outputs();
    let infos: Vec<(String, bool)> = outputs
        .iter()
        .map(|o| (st.output_display_name(o), st.osx_block_for(o).is_some()))
        .collect();
    let mut card = div().flex().flex_col().py(px(6.));
    card = card.child(
        div()
            .px(px(14.))
            .py(px(6.))
            .text_size(px(11.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text_secondary)
            .child("OUTPUTS"),
    );
    if outputs.is_empty() {
        card = card.child(
            div()
                .px(px(14.))
                .py(px(8.))
                .text_size(px(12.))
                .text_color(theme.text_tertiary)
                .child("The server reported no outputs."),
        );
    }
    let multi = outputs.iter().filter(|o| o.enabled).count() > 1;
    for (ix, o) in outputs.iter().enumerate() {
        let id = o.id;
        let (display_name, is_local_osx) = infos[ix].clone();
        let s_route = state.clone();
        let s_toggle = state.clone();
        let is_alsa = o.plugin == "alsa";
        let dop = o.attribute("dop");
        let formats = o
            .attribute("allowed_formats")
            .filter(|f| !f.is_empty())
            .map(str::to_owned);
        let mut sub = o.plugin_label().to_string();
        if is_local_osx {
            sub = format!("{} · MPD output \"{}\"", sub, o.name);
        }
        if let Some(f) = &formats {
            sub.push_str(&format!(" · formats {f}"));
        }
        let mut row = div()
            .id(("output-row", id as usize))
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .py(px(8.))
            .cursor_pointer()
            .hover(|d| d.bg(theme.sidebar_hover))
            .on_click(move |_, _, cx| s_route.read(cx).route_to_output(id))
            .child(icon(
                if o.enabled { "radio-on" } else { "radio-off" },
                14.,
                if o.enabled {
                    theme.accent
                } else {
                    theme.text_tertiary
                },
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme.text)
                            .font_weight(if o.enabled {
                                gpui::FontWeight::SEMIBOLD
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .truncate()
                            .child(display_name),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.text_tertiary)
                            .truncate()
                            .child(sub),
                    ),
            );
        if is_alsa {
            if let Some(dop_val) = dop {
                let on = dop_val == "1";
                let s_dop = state.clone();
                row = row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme.text_tertiary)
                                .child("DoP"),
                        )
                        .child(toggle(("dop", id as usize), on, theme, move |v, _, cx| {
                            s_dop.read(cx).set_output_attribute(
                                id,
                                "dop",
                                if v { "1" } else { "0" },
                            )
                        })),
                );
            }
        }
        let enabled = o.enabled;
        row = row.child(toggle(
            ("out-enabled", id as usize),
            enabled,
            theme,
            move |v, _, cx| s_toggle.read(cx).set_output_enabled(id, v),
        ));
        card = card.child(row);
    }
    if multi {
        card = card.child(
            div()
                .px(px(14.))
                .py(px(6.))
                .text_size(px(10.))
                .text_color(theme.text_tertiary)
                .child("Several outputs are enabled — click a row to make it the only one."),
        );
    }
    if let (Some(open), true) = (open_local_mpd.as_ref(), has_local) {
        let open = open.clone();
        card = card.child(
            div()
                .id("change-device")
                .mx(px(8.))
                .mt(px(4.))
                .px(px(6.))
                .py(px(6.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .hover(|d| d.bg(theme.sidebar_hover))
                .on_click(move |_, window, cx| open(window, cx))
                .child(icon("hard-drive", 12., theme.accent))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.accent)
                        .child("Choose the CoreAudio device (DAC)…"),
                ),
        );
    }
    card.child(
        div()
            .px(px(14.))
            .pt(px(6.))
            .text_size(px(10.))
            .text_color(theme.text_tertiary)
            .child("Click a row to route playback there · switch adds/removes"),
    )
    .into_any_element()
}
