//! Settings: nearby (Bonjour) servers, saved connections, add-server form,
//! Audio (bit-perfect, ReplayGain, crossfade, MixRamp, outputs), Playback,
//! and — for the Homebrew MPD on this Mac — the Local MPD editor with the
//! CoreAudio output card.

use crate::mpd::{ConnectionState, ServerProfile};
use crate::state::AppState;
use crate::state::local_mpd::{self, LocalConf, OsxOutput};
use crate::theme::Theme;
use crate::ui::audio_popovers::{render_outputs, verdict_color};
use crate::ui::widgets::{
    InputEvent, TextInput, icon, icon_button, segmented, stepper, text_button, toggle,
};
use gpui::{
    AnyElement, Context, Div, Entity, ScrollHandle, SharedString, Task, Window, div, prelude::*, px,
};

#[derive(Clone, Debug, PartialEq)]
enum LocalSaveStatus {
    Idle,
    Saving,
    Saved(String),
    Failed(String),
}

/// Editable copy of the local mpd.conf values.
struct LocalForm {
    network_access: bool,
    osx_name: String,
    /// `None` = system default output.
    device: Option<String>,
    hog: bool,
    dop: bool,
    mixer: String,
    replaygain: String,
    normalization: bool,
    drop_audio_output_format: bool,
    /// The conf snapshot the form was filled from (to detect external changes).
    from: LocalConf,
}

impl LocalForm {
    fn from_conf(c: &LocalConf) -> Self {
        let osx = c.osx().cloned().unwrap_or_default();
        LocalForm {
            network_access: c.bind_to_address != "127.0.0.1" && c.bind_to_address != "localhost",
            osx_name: if osx.name.is_empty() {
                "CoreAudio".into()
            } else {
                osx.name.clone()
            },
            device: osx.device.clone(),
            hog: osx.hog_device,
            dop: osx.dop,
            mixer: osx.mixer_type.clone().unwrap_or_else(|| "none".into()),
            replaygain: c.replaygain.clone(),
            normalization: c.volume_normalization,
            drop_audio_output_format: false,
            from: c.clone(),
        }
    }
}

pub struct SettingsView {
    state: Entity<AppState>,
    scroll: ScrollHandle,
    name_input: Entity<TextInput>,
    host_input: Entity<TextInput>,
    port_input: Entity<TextInput>,
    password_input: Entity<TextInput>,
    show_add_form: bool,
    add_error: Option<String>,
    // Local MPD editor
    music_dir_input: Entity<TextInput>,
    local_port_input: Entity<TextInput>,
    local_password_input: Entity<TextInput>,
    local_form: Option<LocalForm>,
    local_status: LocalSaveStatus,
    reveal_local: bool,
    _save_task: Option<Task<()>>,
}

impl SettingsView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |this, state, cx| {
            this.sync_local_form(&state, cx);
            cx.notify();
        })
        .detach();
        let name_input = cx.new(|cx| TextInput::new(cx, "Name (optional)"));
        let host_input = cx.new(|cx| TextInput::new(cx, "Host or IP address"));
        let port_input = cx.new(|cx| TextInput::new(cx, "6600"));
        let password_input = cx.new(|cx| TextInput::new(cx, "Password (optional)").password(true));
        for input in [&name_input, &host_input, &port_input, &password_input] {
            cx.subscribe(input, |this, _, event: &InputEvent, cx| match event {
                InputEvent::Submit => this.submit_add(cx),
                InputEvent::Escape => this.cancel_add(cx),
                InputEvent::Change => {}
            })
            .detach();
        }
        let music_dir_input = cx.new(|cx| TextInput::new(cx, "~/Music"));
        let local_port_input = cx.new(|cx| TextInput::new(cx, "6600"));
        let local_password_input = cx.new(|cx| {
            TextInput::new(cx, "secret@read,add,control,admin (optional)").password(true)
        });
        let mut view = SettingsView {
            state,
            scroll: ScrollHandle::new(),
            name_input,
            host_input,
            port_input,
            password_input,
            show_add_form: false,
            add_error: None,
            music_dir_input,
            local_port_input,
            local_password_input,
            local_form: None,
            local_status: LocalSaveStatus::Idle,
            reveal_local: false,
            _save_task: None,
        };
        let s = view.state.clone();
        view.sync_local_form(&s, cx);
        view
    }

    /// Fill the local editor from the detected config (first time, or when
    /// the config on disk changed under us).
    fn sync_local_form(&mut self, state: &Entity<AppState>, cx: &mut Context<Self>) {
        let conf = state
            .read(cx)
            .local_mpd
            .as_ref()
            .and_then(|l| l.conf.clone());
        match conf {
            Some(c) => {
                let stale = self
                    .local_form
                    .as_ref()
                    .map(|f| f.from != c)
                    .unwrap_or(true);
                if stale {
                    self.local_form = Some(LocalForm::from_conf(&c));
                    self.music_dir_input
                        .update(cx, |i, cx| i.set_text(c.music_directory.clone(), cx));
                    self.local_port_input
                        .update(cx, |i, cx| i.set_text(c.port.clone(), cx));
                    self.local_password_input
                        .update(cx, |i, cx| i.set_text(c.password.clone(), cx));
                }
            }
            None => self.local_form = None,
        }
    }

    pub fn reveal_local_mpd(&mut self, cx: &mut Context<Self>) {
        self.reveal_local = true;
        self.state.update(cx, |s, cx| {
            s.refresh_local_mpd(cx);
            s.load_audio_devices(cx);
        });
        // Scroll to the bottom where the Local MPD section lives.
        self.scroll.set_offset(gpui::point(px(0.), px(-100000.)));
        cx.notify();
    }

    fn cancel_add(&mut self, cx: &mut Context<Self>) {
        self.show_add_form = false;
        self.add_error = None;
        cx.notify();
    }

    fn submit_add(&mut self, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text().to_owned();
        let host = self.host_input.read(cx).text().trim().to_owned();
        let port_text = self.port_input.read(cx).text().trim().to_owned();
        let password = self.password_input.read(cx).text().to_owned();
        if host.is_empty() {
            self.add_error = Some("Host is required.".into());
            cx.notify();
            return;
        }
        let port: u16 = if port_text.is_empty() {
            6600
        } else {
            match port_text.parse() {
                Ok(p) => p,
                Err(_) => {
                    self.add_error = Some("Port must be a number between 1 and 65535.".into());
                    cx.notify();
                    return;
                }
            }
        };
        let profile = ServerProfile::manual(&name, &host, port, Some(password));
        self.state.update(cx, |s, cx| {
            s.add_profile(profile.clone());
            s.connect(profile, cx);
        });
        for input in [
            &self.name_input,
            &self.host_input,
            &self.port_input,
            &self.password_input,
        ] {
            input.update(cx, |i, cx| i.set_text("", cx));
        }
        self.show_add_form = false;
        self.add_error = None;
        cx.notify();
    }

    /// Writes mpd.conf (top-level keys + osx block) and restarts the daemon.
    fn save_local(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.local_form.as_ref() else {
            return;
        };
        let Some(path) = self
            .state
            .read(cx)
            .local_mpd
            .as_ref()
            .and_then(|l| l.conf_path.clone())
        else {
            return;
        };
        let music_dir = self.music_dir_input.read(cx).text().trim().to_owned();
        let port = self.local_port_input.read(cx).text().trim().to_owned();
        let password = self.local_password_input.read(cx).text().to_owned();
        let bind = if form.network_access {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        }
        .to_owned();
        // Bluetooth/AirPlay: no hog (CoreAudio refuses) and a pinned `format` at
        // the device's 44.1/48 kHz because the osx plugin won't convert on its
        // own. Wired DACs: no format (bit-perfect).
        let devices = &self.state.read(cx).audio_devices;
        let dev = match &form.device {
            Some(name) => devices.iter().find(|d| &d.name == name),
            None => devices.iter().find(|d| d.is_default),
        };
        let lossy = dev.map(|d| d.is_lossy_transport()).unwrap_or(false);
        // hog needs an explicit device: the system-default output unit is mixed by
        // coreaudiod, so MPD's own exclusive claim locks it out (!hog).
        let hog_ok = form.device.is_some() && !lossy;
        let format = if lossy {
            let rate = dev
                .and_then(|d| d.sample_rate)
                .filter(|r| *r == 44100 || *r == 48000)
                .unwrap_or(44100);
            Some(format!("{rate}:24:2"))
        } else {
            None
        };
        let osx = OsxOutput {
            name: form.osx_name.clone(),
            device: form.device.clone(),
            hog_device: form.hog && hog_ok,
            dop: form.dop && !lossy,
            mixer_type: Some(form.mixer.clone()),
            format,
        };
        let mut updates: Vec<(&'static str, Option<String>)> = vec![
            ("music_directory", Some(music_dir).filter(|s| !s.is_empty())),
            ("bind_to_address", Some(bind)),
            (
                "port",
                Some(if port.is_empty() { "6600".into() } else { port }),
            ),
            (
                "password",
                if password.is_empty() {
                    None
                } else {
                    Some(password)
                },
            ),
        ];
        updates.extend(local_mpd::bit_perfect_top_level(
            &form.replaygain,
            form.normalization,
            form.drop_audio_output_format,
        ));
        self.local_status = LocalSaveStatus::Saving;
        cx.notify();
        let work = cx.background_executor().spawn(async move {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let text = local_mpd::apply_top_level(&text, &updates);
            let text = local_mpd::apply_osx_output(&text, &osx);
            local_mpd::write_conf(&path, &text).map_err(|e| e.to_string())?;
            local_mpd::restart(&path)
        });
        self._save_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |t, cx| {
                t.local_status = match result {
                    Ok(msg) => LocalSaveStatus::Saved(format!("Saved · {msg}")),
                    Err(e) => LocalSaveStatus::Failed(e),
                };
                t.state.update(cx, |s, cx| s.refresh_local_mpd(cx));
                cx.notify();
            });
        }));
    }
}

fn section(title: &str, theme: &Theme, rows: Vec<AnyElement>) -> Div {
    let mut list = div()
        .flex()
        .flex_col()
        .rounded(px(10.))
        .bg(theme.control_bg.opacity(0.6))
        .border_1()
        .border_color(theme.separator);
    let n = rows.len();
    for (i, row) in rows.into_iter().enumerate() {
        list = list.child(
            div()
                .flex()
                .items_center()
                .min_h(px(40.))
                .px(px(14.))
                .py(px(8.))
                .when(i + 1 < n, |d| d.border_b_1().border_color(theme.separator))
                .child(row),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .px(px(4.))
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_secondary)
                .child(title.to_uppercase()),
        )
        .child(list)
}

fn labeled(label: impl Into<SharedString>, sub: Option<String>, theme: &Theme) -> Div {
    // flex_grow (basis auto) rather than flex_1 (basis 0): with basis 0 a
    // parent *column* sizes itself as if this block were 0 px tall.
    div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_w(px(0.))
        .gap(px(1.))
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme.text)
                .child(label.into()),
        )
        .when_some(sub, |d, s| {
            d.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.text_tertiary)
                    .child(s),
            )
        })
}

fn muted(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(px(13.))
        .text_color(theme.text_tertiary)
        .child(text.into())
}

fn field(label: &str, input: &Entity<TextInput>, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .w_full()
        .child(
            div()
                .w(px(120.))
                .text_size(px(13.))
                .text_color(theme.text_secondary)
                .child(label.to_owned()),
        )
        .child(div().flex_1().child(input.clone()))
}

const REPLAY_GAIN_OPTIONS: [(&str, &str); 4] = [
    ("off", "Off"),
    ("track", "Track"),
    ("album", "Album"),
    ("auto", "Auto"),
];

fn rg_options() -> Vec<(&'static str, SharedString)> {
    REPLAY_GAIN_OPTIONS
        .iter()
        .map(|(v, l)| (*v, SharedString::from(*l)))
        .collect()
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let st = self.state.read(cx);
        let discovered = st.discovered.clone();
        let profiles = st.profiles.clone();
        let active = st.active_profile.clone();
        let conn = st.connection_state.clone();
        let status = st.status.clone();

        let outputs = st.outputs.clone();
        let replay_gain = st.replay_gain_mode.clone();
        let bit_perfect = st.bit_perfect_mode;
        let drifted = st.bit_perfect_drifted();
        let sp = st.signal_path();
        let local = st.local_mpd.clone();
        let is_local = st.is_local_server();
        let devices = st.audio_devices.clone();
        let devices_loading = st.audio_devices_loading;
        let state = self.state.clone();

        // ---- Servers: one unified list (nearby ∪ saved), active row inline ----
        struct Entry {
            name: String,
            host: String,
            port: u16,
            nearby: bool,
            saved: Option<ServerProfile>,
        }
        let mut entries: Vec<Entry> = Vec::new();
        for d in &discovered {
            entries.push(Entry {
                name: d.name.clone(),
                host: d.host.clone(),
                port: d.port,
                nearby: true,
                saved: None,
            });
        }
        for p in &profiles {
            if let Some(e) = entries
                .iter_mut()
                .find(|e| e.host == p.host && e.port == p.port)
            {
                // Same server seen via Bonjour and saved manually: one row, saved name wins.
                e.name = p.name.clone();
                e.saved = Some(p.clone());
            } else {
                entries.push(Entry {
                    name: p.name.clone(),
                    host: p.host.clone(),
                    port: p.port,
                    nearby: false,
                    saved: Some(p.clone()),
                });
            }
        }
        let active_matches = |e: &Entry| {
            active
                .as_ref()
                .map(|a| {
                    a.id == e.saved.as_ref().map(|p| p.id.clone()).unwrap_or_default()
                        || (a.host == e.host && a.port == e.port)
                })
                .unwrap_or(false)
        };
        let status_of = |conn: &ConnectionState| -> (gpui::Hsla, &'static str) {
            match conn {
                ConnectionState::Connected => (theme.success, "Connected"),
                ConnectionState::Connecting => (theme.warning, "Connecting…"),
                ConnectionState::Error(_) => (theme.danger, "Reconnecting…"),
                ConnectionState::Disconnected => (theme.text_tertiary, "Disconnected"),
            }
        };

        let mut conn_rows: Vec<AnyElement> = Vec::new();
        // The active server isn't in the list (vanished from Bonjour / removed): show it on top.
        if let Some(a) = active
            .as_ref()
            .filter(|_| !entries.iter().any(|e| active_matches(e)))
        {
            let (dot, text) = status_of(&conn);
            let s = state.clone();
            let sub = match &conn {
                ConnectionState::Error(e) => format!("{}:{} · {e}", a.host, a.port),
                _ => format!("{}:{}", a.host, a.port),
            };
            conn_rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .w_full()
                    .child(div().size(px(9.)).rounded_full().bg(dot).flex_shrink_0())
                    .child(labeled(a.name.clone(), Some(sub), &theme))
                    .child(muted(text, &theme))
                    .child(
                        text_button("disconnect-orphan", "Disconnect", &theme, false)
                            .on_click(move |_, _, cx| s.update(cx, |s, cx| s.disconnect(cx))),
                    )
                    .into_any_element(),
            );
        }
        if entries.is_empty() && active.is_none() {
            conn_rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("wifi", 14., theme.text_tertiary))
                    .child(muted(
                        "Searching for MPD servers on the local network…",
                        &theme,
                    ))
                    .into_any_element(),
            );
        }
        for (i, e) in entries.iter().enumerate() {
            let is_active = active_matches(e);
            let mut tags: Vec<&str> = Vec::new();
            if e.nearby {
                tags.push("nearby");
            }
            if e.saved.is_some() {
                tags.push("saved");
            }
            if e.saved
                .as_ref()
                .map(|p| p.password.is_some())
                .unwrap_or(false)
            {
                tags.push("password");
            }
            let mut sub = format!("{}:{}", e.host, e.port);
            for t in tags {
                sub.push_str(" · ");
                sub.push_str(t);
            }
            if is_active {
                if let ConnectionState::Error(err) = &conn {
                    sub.push_str(" · ");
                    sub.push_str(err);
                }
            }
            let profile = e
                .saved
                .clone()
                .unwrap_or_else(|| ServerProfile::discovered(&e.name, &e.host, e.port));
            let s_conn = state.clone();
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(12.))
                .w_full()
                .child(if is_active {
                    let (dot, _) = status_of(&conn);
                    div()
                        .size(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().size(px(9.)).rounded_full().bg(dot))
                        .into_any_element()
                } else {
                    icon(
                        if e.saved.is_some() { "server" } else { "wifi" },
                        14.,
                        theme.text_secondary,
                    )
                    .into_any_element()
                })
                .child(labeled(e.name.clone(), Some(sub), &theme));
            if is_active {
                let (_, text) = status_of(&conn);
                let s_dis = state.clone();
                row = row.child(muted(text, &theme)).child(
                    text_button(("disconnect", i), "Disconnect", &theme, false)
                        .on_click(move |_, _, cx| s_dis.update(cx, |s, cx| s.disconnect(cx))),
                );
            } else {
                row = row.child(
                    text_button(("connect", i), "Connect", &theme, false).on_click(
                        move |_, _, cx| s_conn.update(cx, |s, cx| s.connect(profile.clone(), cx)),
                    ),
                );
            }
            if let Some(p) = &e.saved {
                let id = p.id.clone();
                let s_rm = state.clone();
                row = row.child(
                    icon_button(("remove", i), "trash-2", 14., theme.danger).on_click(
                        move |_, _, cx| s_rm.update(cx, |s, cx| s.remove_profile(&id, cx)),
                    ),
                );
            }
            conn_rows.push(row.into_any_element());
        }
        if self.show_add_form {
            conn_rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .w_full()
                    .py(px(4.))
                    .child(field("Name", &self.name_input, &theme))
                    .child(field("Host", &self.host_input, &theme))
                    .child(field("Port", &self.port_input, &theme))
                    .child(field("Password", &self.password_input, &theme))
                    .when_some(self.add_error.clone(), |d, e| {
                        d.child(div().text_size(px(12.)).text_color(theme.danger).child(e))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                text_button("cancel-add", "Cancel", &theme, false)
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_add(cx))),
                            )
                            .child(
                                text_button("confirm-add", "Add & Connect", &theme, true)
                                    .on_click(cx.listener(|this, _, _, cx| this.submit_add(cx))),
                            ),
                    )
                    .into_any_element(),
            );
        } else {
            conn_rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .id("add-server")
                    .cursor_pointer()
                    .text_color(theme.accent)
                    .text_size(px(13.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_add_form = true;
                        this.host_input.read(cx).focus(window);
                        cx.notify();
                    }))
                    .child(icon("plus", 13., theme.accent))
                    .child("Add Server…")
                    .into_any_element(),
            );
        }

        // ---- Audio ---------------------------------------------------------
        let mut audio_rows: Vec<AnyElement> = Vec::new();
        {
            let s = state.clone();
            let s2 = state.clone();
            let color = verdict_color(sp.verdict, &theme);
            audio_rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .w_full()
                    .child(icon("lock", 14., if bit_perfect { theme.accent } else { theme.text_secondary }))
                    .child(labeled(
                        "Bit-perfect mode",
                        Some("Turns off ReplayGain, crossfade and MixRamp and pins the volume at 100 % so MPD passes samples through untouched.".into()),
                        &theme,
                    ))
                    .when(drifted, |d| {
                        d.child(
                            text_button("reapply-bp-settings", "Re-apply", &theme, true)
                                .on_click(move |_, _, cx| s2.read(cx).apply_bit_perfect()),
                        )
                    })
                    .child(toggle("bp-settings", bit_perfect, &theme, move |on, _, cx| {
                        s.update(cx, |s, cx| s.set_bit_perfect(on, cx))
                    }))
                    .into_any_element(),
            );
            audio_rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .w_full()
                    .child(div().size(px(9.)).rounded_full().bg(color).flex_shrink_0())
                    .child(labeled(
                        format!("Signal path: {}", sp.verdict.label()),
                        Some(if sp.reasons.is_empty() {
                            sp.output.clone().unwrap_or_else(|| {
                                "See the pill in the transport bar for the live path.".into()
                            })
                        } else {
                            sp.reasons.join(" · ")
                        }),
                        &theme,
                    ))
                    .into_any_element(),
            );
        }
        {
            let s = state.clone();
            audio_rows.push(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .child(labeled("ReplayGain", Some("Loudness normalisation from tags — any mode other than Off scales samples".into()), &theme))
                    .child(segmented("rg", &rg_options(), &replay_gain, &theme, move |v, _, cx| {
                        s.read(cx).set_replay_gain(v)
                    }))
                    .into_any_element(),
            );
        }
        {
            let s_x = state.clone();
            let crossfade = status.crossfade;
            audio_rows.push(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .child(labeled(
                        "Crossfade",
                        Some("Seconds of overlap between tracks".into()),
                        &theme,
                    ))
                    .child(stepper(
                        "crossfade",
                        format!("{crossfade} s"),
                        &theme,
                        crossfade > 0,
                        crossfade < 30,
                        move |delta, _, cx| {
                            let next = (crossfade as i32 + delta).clamp(0, 30) as u32;
                            s_x.read(cx).set_crossfade(next);
                        },
                    ))
                    .into_any_element(),
            );
        }
        {
            let s = state.clone();
            let mix = match status.mixrampdelay {
                Some(d) => format!("{} dB threshold · {d} s delay", status.mixrampdb),
                None => "Off".to_string(),
            };
            let on = status.mixrampdelay.is_some();
            audio_rows.push(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .child(labeled("MixRamp", Some(mix), &theme))
                    .when(on, |d| {
                        d.child(
                            text_button("mixramp-off", "Disable", &theme, false)
                                .on_click(move |_, _, cx| s.read(cx).disable_mixramp()),
                        )
                    })
                    .into_any_element(),
            );
        }
        audio_rows.push(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(labeled(
                    "Outputs",
                    Some(if outputs.is_empty() {
                        "Connect to a server to see its outputs.".into()
                    } else {
                        "Click a row to route playback there; the switch adds or removes an output."
                            .into()
                    }),
                    &theme,
                ))
                .when(!outputs.is_empty(), |d| {
                    d.child(
                        div()
                            .rounded(px(8.))
                            .bg(theme.window_bg)
                            .border_1()
                            .border_color(theme.separator)
                            .child(render_outputs(&state, &outputs, &theme, None, cx)),
                    )
                })
                .into_any_element(),
        );

        // ---- Playback ------------------------------------------------------
        let consume_on = status.consume_mode != "0";
        let s = state.clone();
        let playback_rows: Vec<AnyElement> = vec![
            div()
                .flex()
                .items_center()
                .w_full()
                .child(labeled(
                    "Consume",
                    Some("Remove songs from the queue after they play".into()),
                    &theme,
                ))
                .child(toggle("consume", consume_on, &theme, move |on, _, cx| {
                    s.read(cx).set_consume(on)
                }))
                .into_any_element(),
        ];

        // ---- Local MPD (macOS, when the active server is this machine) -----
        let mut local_rows: Vec<AnyElement> = Vec::new();
        match (&local, self.local_form.as_ref()) {
            (Some(l), Some(form)) => {
                let running = l.running;
                let path_str = l
                    .conf_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let s_refresh = state.clone();
                local_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .w_full()
                        .child(div().size(px(9.)).rounded_full().bg(if running {
                            theme.success
                        } else {
                            theme.text_tertiary
                        }))
                        .child(labeled(
                            if running {
                                "MPD is running on this Mac"
                            } else {
                                "MPD is installed but not running"
                            },
                            Some(path_str),
                            &theme,
                        ))
                        .child(
                            icon_button("refresh-local", "refresh-cw", 14., theme.text_secondary)
                                .on_click(move |_, _, cx| {
                                    s_refresh.update(cx, |s, cx| {
                                        s.refresh_local_mpd(cx);
                                        s.load_audio_devices(cx);
                                    })
                                }),
                        )
                        .into_any_element(),
                );
                // Server basics
                local_rows.push(
                    field("Music directory", &self.music_dir_input, &theme).into_any_element(),
                );
                local_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .w_full()
                        .child(labeled(
                            "Allow network access",
                            Some(
                                if form.network_access {
                                    "bind_to_address 0.0.0.0 — other devices can connect"
                                } else {
                                    "bind_to_address 127.0.0.1 — this Mac only"
                                }
                                .into(),
                            ),
                            &theme,
                        ))
                        .child(toggle(
                            "net-access",
                            form.network_access,
                            &theme,
                            cx.listener(|this, on: &bool, _, cx| {
                                if let Some(f) = this.local_form.as_mut() {
                                    f.network_access = *on;
                                }
                                cx.notify();
                            })
                            .into_toggle(),
                        ))
                        .into_any_element(),
                );
                local_rows.push(field("Port", &self.local_port_input, &theme).into_any_element());
                local_rows
                    .push(field("Password", &self.local_password_input, &theme).into_any_element());

                // ---- Audio output card: pick the CoreAudio device MPD opens ----
                let mut device_list = div().flex().flex_col().gap(px(2.)).mt(px(6.)).w_full();
                let default_dev = devices.iter().find(|d| d.is_default);
                let mut options: Vec<(Option<String>, String, String, bool)> = vec![(
                    None,
                    "System default".into(),
                    match default_dev {
                        Some(d) => format!("follows Sound settings — currently {}", d.name),
                        None => "follows Sound settings".into(),
                    },
                    default_dev.map(|d| d.is_lossy_transport()).unwrap_or(false),
                )];
                for d in &devices {
                    let mut sub = d.transport.clone();
                    if let Some(r) = d.sample_rate {
                        if !sub.is_empty() {
                            sub.push_str(" · ");
                        }
                        sub.push_str(&format!("{} kHz now", r as f64 / 1000.0));
                    }
                    if let Some(m) = d.max_sample_rate {
                        sub.push_str(&format!(" · up to {} kHz", m as f64 / 1000.0));
                    }
                    if d.hog_pid.is_some() {
                        sub.push_str(" · exclusive (in use)");
                    }
                    if d.is_lossy_transport() {
                        sub.push_str(" · lossy transport, no exclusive mode");
                    }
                    if d.is_default {
                        sub.push_str(" · system default");
                    }
                    options.push((
                        Some(d.name.clone()),
                        d.name.clone(),
                        sub,
                        d.is_lossy_transport(),
                    ));
                }
                if let Some(cur) = &form.device {
                    if !devices.iter().any(|d| &d.name == cur) {
                        options.push((
                            Some(cur.clone()),
                            cur.clone(),
                            "not currently connected".into(),
                            false,
                        ));
                    }
                }
                let selected_lossy = options
                    .iter()
                    .find(|(v, ..)| *v == form.device)
                    .map(|o| o.3)
                    .unwrap_or(false);
                let selected_default = form.device.is_none();
                let hog_blocked = selected_lossy || selected_default;
                for (i, (value, label, sub, lossy)) in options.into_iter().enumerate() {
                    let selected = form.device == value;
                    let v = value.clone();
                    device_list = device_list.child(
                        div()
                            .id(("device", i))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(8.))
                            .py(px(5.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|d| d.bg(theme.sidebar_hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(f) = this.local_form.as_mut() {
                                    f.device = v.clone();
                                    if lossy || v.is_none() {
                                        // hog needs an explicit wired device.
                                        f.hog = false;
                                    }
                                }
                                cx.notify();
                            }))
                            .child(icon(
                                if selected { "radio-on" } else { "radio-off" },
                                14.,
                                if selected {
                                    theme.accent
                                } else {
                                    theme.text_tertiary
                                },
                            ))
                            .child(labeled(label, Some(sub), &theme)),
                    );
                }
                if devices_loading {
                    device_list = device_list.child(muted("Scanning CoreAudio devices…", &theme));
                }
                let s_dev = state.clone();
                local_rows.push(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .w_full()
                                .child(labeled(
                                    "Audio output (CoreAudio)",
                                    Some("The DAC MPD opens on this Mac. Bit-perfect chain: your DAC · Exclusive on · Volume control None. Changing it restarts MPD.".into()),
                                    &theme,
                                ))
                                .child(
                                    text_button("scan-devices", "Rescan", &theme, false)
                                        .on_click(move |_, _, cx| s_dev.update(cx, |s, cx| s.load_audio_devices(cx))),
                                ),
                        )
                        .child(device_list)
                        .into_any_element(),
                );
                local_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .w_full()
                        .child(labeled(
                            "Exclusive access",
                            Some(if selected_lossy {
                                "Not available on Bluetooth/AirPlay — MPD will instead convert to the device's 44.1/48 kHz (format …) so playback works".into()
                            } else if selected_default {
                                "Needs a specific device — the system-default output is mixed by macOS, so an exclusive claim would lock MPD out (!hog)".into()
                            } else {
                                "hog_device — MPD owns the DAC while its output is enabled; nothing else can mix into it".into()
                            }),
                            &theme,
                        ))
                        .child(toggle("hog", form.hog && !hog_blocked, &theme, cx.listener(move |this, on: &bool, _, cx| {
                            if let Some(f) = this.local_form.as_mut() { f.hog = *on && !hog_blocked; }
                            cx.notify();
                        }).into_toggle()))
                        .into_any_element(),
                );
                local_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .w_full()
                        .child(labeled(
                            "DSD over PCM (DoP)",
                            Some("Send DSD64/128 to a DoP-capable DAC as 24-bit PCM frames".into()),
                            &theme,
                        ))
                        .child(toggle(
                            "dop",
                            form.dop,
                            &theme,
                            cx.listener(|this, on: &bool, _, cx| {
                                if let Some(f) = this.local_form.as_mut() {
                                    f.dop = *on;
                                }
                                cx.notify();
                            })
                            .into_toggle(),
                        ))
                        .into_any_element(),
                );
                local_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .w_full()
                        .child(labeled("Volume control", Some(match form.mixer.as_str() {
                            "none" => "mixer_type none — fixed output, use the DAC/amp knob (bit-perfect)",
                            "hardware" => "mixer_type hardware — CoreAudio device volume, samples untouched",
                            _ => "mixer_type software — MPD scales samples (lossy)",
                        }.into()), &theme))
                        .child(segmented(
                            "mixer",
                            &[("none", "None".into()), ("hardware", "Hardware".into()), ("software", "Software".into())],
                            &form.mixer,
                            &theme,
                            cx.listener(|this, v: &&'static str, _, cx| {
                                if let Some(f) = this.local_form.as_mut() { f.mixer = (*v).to_owned(); }
                                cx.notify();
                            }).into_segmented(),
                        ))
                        .into_any_element(),
                );
                // Checklist
                let aof = form.from.audio_output_format.clone();
                local_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .w_full()
                        .child(labeled(
                            "Volume normalization",
                            Some(
                                "volume_normalization — a DSP stage; keep it off for bit-perfect"
                                    .into(),
                            ),
                            &theme,
                        ))
                        .child(toggle(
                            "norm",
                            form.normalization,
                            &theme,
                            cx.listener(|this, on: &bool, _, cx| {
                                if let Some(f) = this.local_form.as_mut() {
                                    f.normalization = *on;
                                }
                                cx.notify();
                            })
                            .into_toggle(),
                        ))
                        .into_any_element(),
                );
                local_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .w_full()
                        .child(labeled("ReplayGain (config default)", Some("replaygain in mpd.conf — the live mode above overrides it until restart".into()), &theme))
                        .child(segmented("rg-conf", &rg_options(), &form.replaygain, &theme, cx.listener(|this, v: &&'static str, _, cx| {
                            if let Some(f) = this.local_form.as_mut() { f.replaygain = (*v).to_owned(); }
                            cx.notify();
                        }).into_segmented()))
                        .into_any_element(),
                );
                if let Some(f) = aof {
                    local_rows.push(
                        div()
                            .flex()
                            .items_center()
                            .w_full()
                            .child(icon("triangle-alert", 14., theme.verdict_amber))
                            .child(div().w(px(10.)))
                            .child(labeled(
                                format!("audio_output_format \"{f}\" forces resampling"),
                                Some("Remove it so MPD passes the source rate through (the osx plugin syncs the DAC to it)".into()),
                                &theme,
                            ))
                            .child(toggle("drop-aof", form.drop_audio_output_format, &theme, cx.listener(|this, on: &bool, _, cx| {
                                if let Some(fm) = this.local_form.as_mut() { fm.drop_audio_output_format = *on; }
                                cx.notify();
                            }).into_toggle()))
                            .into_any_element(),
                    );
                }
                // Save row
                let (status_text, status_color) = match &self.local_status {
                    LocalSaveStatus::Idle => (String::new(), theme.text_tertiary),
                    LocalSaveStatus::Saving => (
                        "Saving and restarting MPD…".to_string(),
                        theme.text_tertiary,
                    ),
                    LocalSaveStatus::Saved(m) => (m.clone(), theme.success),
                    LocalSaveStatus::Failed(e) => (format!("Failed: {e}"), theme.danger),
                };
                let saving = self.local_status == LocalSaveStatus::Saving;
                local_rows.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .w_full()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(status_color)
                                .child(status_text),
                        )
                        .child(muted("Restarts MPD — playback pauses briefly", &theme))
                        .child(
                            text_button(
                                "save-local",
                                if saving {
                                    "Saving…"
                                } else {
                                    "Save & Restart MPD"
                                },
                                &theme,
                                true,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !saving {
                                        this.save_local(cx)
                                    }
                                },
                            )),
                        )
                        .into_any_element(),
                );
            }
            (Some(_), None) => {
                local_rows.push(muted("mpd.conf not found (looked in ~/.config/mpd, ~/.mpdconf, /opt/homebrew/etc, /usr/local/etc).", &theme).into_any_element());
            }
            (None, _) => {
                local_rows.push(
                    muted(
                        if is_local { "Detecting the local MPD…" } else { "Connect to the MPD running on this Mac (127.0.0.1) to edit its configuration here." },
                        &theme,
                    )
                    .into_any_element(),
                );
            }
        }

        let reveal = self.reveal_local;
        self.reveal_local = false;

        div()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .bg(theme.window_bg)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(22.))
                    .p(px(24.))
                    // Fixed (not max) width: wrapped subtitles must measure the
                    // same in every layout pass or sections overlap.
                    .w(px(660.))
                    .child(section("Servers", &theme, conn_rows))
                    .child(section("Audio", &theme, audio_rows))
                    .child(section("Playback", &theme, playback_rows))
                    .child(
                        div()
                            .when(reveal, |d| {
                                d.rounded(px(12.)).p(px(4.)).bg(theme.accent_soft)
                            })
                            .child(section("Local MPD (this Mac)", &theme, local_rows)),
                    ),
            )
    }
}

/// Adapters so `cx.listener` closures (which take the event by reference)
/// can be handed to widgets that pass values.
trait IntoToggle {
    fn into_toggle(self) -> impl Fn(bool, &mut Window, &mut gpui::App) + 'static;
}
impl<F: Fn(&bool, &mut Window, &mut gpui::App) + 'static> IntoToggle for F {
    fn into_toggle(self) -> impl Fn(bool, &mut Window, &mut gpui::App) + 'static {
        move |v, window, cx| self(&v, window, cx)
    }
}
trait IntoSegmented {
    fn into_segmented(self) -> impl Fn(&'static str, &mut Window, &mut gpui::App) + 'static;
}
impl<F: Fn(&&'static str, &mut Window, &mut gpui::App) + 'static> IntoSegmented for F {
    fn into_segmented(self) -> impl Fn(&'static str, &mut Window, &mut gpui::App) + 'static {
        move |v, window, cx| self(&v, window, cx)
    }
}
