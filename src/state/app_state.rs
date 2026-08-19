//! The single source of truth for the UI (port of AppEnvironment + PlayerStore
//! + QueueStore + ConnectionStore). One GPUI entity; views observe it.

use crate::mpd::client::{ArtKind, DiscoveredServer, MpdEvent, SessionEvent};
use crate::mpd::discovery::Discovery;
use crate::mpd::{
    Command, ConnectionState, MpdClient, Output, PlayState, ServerProfile, Song, Status,
};
use crate::state::coreaudio::{self, CaDevice};
use crate::state::cover_cache::{self, CoverCache};
use crate::state::library_index::{self, LibraryAlbum};
use crate::state::local_mpd::{self, LocalMpd, OsxOutput};
use crate::state::persistence;
use crate::state::power;
use crate::state::signal_path::{self, FixAction, OutputInfo, SignalPath};
use futures::StreamExt;
use gpui::{Context, Image, Task};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const AUTO_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const ERROR_BANNER_TTL: Duration = Duration::from_secs(5);

pub struct AppState {
    pub client: MpdClient,
    _discovery: Option<Discovery>,
    cover_cache: CoverCache,

    // Connection
    pub connection_state: ConnectionState,
    pub active_profile: Option<ServerProfile>,
    pub profiles: Vec<ServerProfile>,
    pub discovered: Vec<DiscoveredServer>,
    reconnecting: bool,
    reconnect_attempt: u32,
    reconnect_task: Option<Task<()>>,
    auto_connect_pending: bool,
    _auto_connect_timeout: Option<Task<()>>,

    // Player
    pub status: Status,
    pub current_song: Option<Song>,
    pub cover_art: Option<Arc<Image>>,
    pub backdrop: Option<Arc<Image>>,
    last_known_elapsed: f64,
    last_sync: Instant,
    ticker: Option<Task<()>>,
    _backdrop_task: Option<Task<()>>,

    // Queue
    pub queue: Vec<Song>,
    pub queue_loaded: bool,

    // Library
    pub library: Vec<Song>,
    pub albums: Vec<LibraryAlbum>,
    pub library_loading: bool,
    pub library_loaded: bool,
    /// Bumped every time `library` is replaced, so views can memoise derived data.
    pub library_version: u64,
    /// album name → art (None = known to have none)
    pub album_art: HashMap<String, Option<Arc<Image>>>,
    pending_album_art: HashMap<String, String>,

    // Audio
    pub outputs: Vec<Output>,
    pub replay_gain_mode: String,
    pub bit_perfect_mode: bool,
    /// Local (this Mac) MPD config, present when the active server is local.
    pub local_mpd: Option<LocalMpd>,
    pub audio_devices: Vec<CaDevice>,
    pub audio_devices_loading: bool,
    _devices_task: Option<Task<()>>,
    _local_task: Option<Task<()>>,

    /// (output name, message) of the last failed enable/disable, from MPD's ACK.
    pub last_output_error: Option<(String, String)>,
    /// After wake: cycle the enabled local osx outputs once outputs are known.
    post_wake_recovery: bool,
    slept_at: Option<Instant>,
    /// Transient sidebar note ("Reconnected after sleep").
    pub wake_notice: Option<String>,
    _wake_notice_task: Option<Task<()>>,

    // Transient error banner
    pub last_error: Option<String>,
    _error_task: Option<Task<()>>,
}

impl AppState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<MpdEvent>();
        let client = MpdClient::new(tx.clone());
        power::start(tx.clone());
        let discovery = Discovery::start(tx);
        let persisted = persistence::load();

        // Event pump: background threads → this entity.
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.next().await {
                if this
                    .update(cx, |state, cx| state.handle(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let mut state = AppState {
            client,
            _discovery: discovery,
            cover_cache: CoverCache::new(),
            connection_state: ConnectionState::Disconnected,
            active_profile: None,
            profiles: persisted.profiles,
            discovered: Vec::new(),
            reconnecting: false,
            reconnect_attempt: 0,
            reconnect_task: None,
            auto_connect_pending: false,
            _auto_connect_timeout: None,
            status: Status::default(),
            current_song: None,
            cover_art: None,
            backdrop: None,
            last_known_elapsed: 0.0,
            last_sync: Instant::now(),
            ticker: None,
            _backdrop_task: None,
            queue: Vec::new(),
            queue_loaded: false,
            library: Vec::new(),
            albums: Vec::new(),
            library_loading: false,
            library_loaded: false,
            library_version: 0,
            album_art: HashMap::new(),
            pending_album_art: HashMap::new(),
            outputs: Vec::new(),
            replay_gain_mode: "off".into(),
            bit_perfect_mode: persisted.bit_perfect_mode,
            local_mpd: None,
            audio_devices: Vec::new(),
            audio_devices_loading: false,
            _devices_task: None,
            _local_task: None,
            last_output_error: None,
            post_wake_recovery: false,
            slept_at: None,
            wake_notice: None,
            _wake_notice_task: None,
            last_error: None,
            _error_task: None,
        };

        // Reconnect to the last-used server; otherwise auto-connect to the
        // first Bonjour server that shows up within the timeout.
        if let Some(profile) = persisted.last_profile {
            state.connect(profile, cx);
        } else {
            state.auto_connect_pending = true;
            state._auto_connect_timeout = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(AUTO_CONNECT_TIMEOUT).await;
                let _ = this.update(cx, |s, _| s.auto_connect_pending = false);
            }));
        }
        state
    }

    // ----- Connection -------------------------------------------------------

    pub fn connect(&mut self, profile: ServerProfile, cx: &mut Context<Self>) {
        let switching = self
            .active_profile
            .as_ref()
            .map(|p| p.id != profile.id)
            .unwrap_or(true);
        if switching {
            self.clear_content();
        }
        self.reconnecting = false;
        self.reconnect_attempt = 0;
        self.reconnect_task = None;
        self.auto_connect_pending = false;
        if profile.is_manual && !self.profiles.iter().any(|p| p.id == profile.id) {
            self.profiles.push(profile.clone());
        }
        self.active_profile = Some(profile.clone());
        self.persist();
        self.client.connect(profile);
        self.refresh_local_mpd(cx);
        cx.notify();
    }

    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.client.disconnect();
        self.reconnecting = false;
        self.reconnect_task = None;
        self.active_profile = None;
        self.connection_state = ConnectionState::Disconnected;
        self.persist();
        self.refresh_ticker(cx);
        cx.notify();
    }

    pub fn add_profile(&mut self, profile: ServerProfile) {
        if !self.profiles.iter().any(|p| p.id == profile.id) {
            self.profiles.push(profile);
        }
        self.persist();
    }

    pub fn remove_profile(&mut self, id: &str, cx: &mut Context<Self>) {
        self.profiles.retain(|p| p.id != id);
        if self.active_profile.as_ref().map(|p| p.id.as_str()) == Some(id) {
            self.disconnect(cx);
        }
        self.persist();
        cx.notify();
    }

    fn persist(&self) {
        persistence::save(
            &self.profiles,
            self.active_profile.as_ref(),
            self.bit_perfect_mode,
        );
    }

    fn clear_content(&mut self) {
        self.status = Status::default();
        self.current_song = None;
        self.cover_art = None;
        self.backdrop = None;
        self.queue.clear();
        self.queue_loaded = false;
        self.library.clear();
        self.albums.clear();
        self.library_loaded = false;
        self.library_loading = false;
        self.album_art.clear();
        self.pending_album_art.clear();
        self.outputs.clear();
        self.replay_gain_mode = "off".into();
    }

    /// Reconnect path (mirrors `MPDClient.scheduleReconnect`): attempt 0 is
    /// immediate, then `min(2^n, 30)` seconds between attempts, forever.
    fn schedule_reconnect(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.active_profile.clone() else {
            return;
        };
        self.reconnecting = true;
        self.connection_state = ConnectionState::Error("Connection lost — reconnecting…".into());
        self.refresh_ticker(cx);
        cx.notify();
        let attempt = self.reconnect_attempt;
        let delay = if attempt == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(2f64.powi(attempt as i32).min(30.0))
        };
        self.reconnect_task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let _ = this.update(cx, |s, _cx| {
                if s.reconnecting && s.active_profile.as_ref().map(|p| &p.id) == Some(&profile.id) {
                    s.client.connect(profile);
                }
            });
        }));
    }

    pub fn refresh_all(&self) {
        self.client.send(Command::RefreshAll);
    }

    // ----- Event handling ---------------------------------------------------

    fn handle(&mut self, event: MpdEvent, cx: &mut Context<Self>) {
        match event {
            MpdEvent::Discovered(list) => {
                self.discovered = list;
                if self.auto_connect_pending && self.active_profile.is_none() {
                    if let Some(first) = self.discovered.first().cloned() {
                        let profile =
                            ServerProfile::discovered(&first.name, &first.host, first.port);
                        self.connect(profile, cx);
                    }
                }
                cx.notify();
            }
            MpdEvent::SystemWillSleep => {
                self.slept_at = Some(Instant::now());
                eprintln!("[melo] system will sleep");
            }
            MpdEvent::SystemWoke => {
                eprintln!("[melo] system woke from sleep");
                self.audio_devices = coreaudio::output_devices();
                if let Some(profile) = self.active_profile.clone() {
                    // Fresh sockets (the old ones may be half-dead), then re-open
                    // hogged local outputs once the new session reports them.
                    self.post_wake_recovery = true;
                    self.reconnecting = false;
                    self.reconnect_task = None;
                    self.client.connect(profile);
                    self.refresh_local_mpd(cx);
                }
                cx.notify();
            }
            MpdEvent::Session { generation, event } => {
                if generation != self.client.generation() {
                    return; // straggler from a torn-down session
                }
                self.handle_session_event(event, cx);
            }
        }
    }

    fn handle_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::State(state) => {
                eprintln!("[melo] connection: {state:?}");
                match &state {
                    ConnectionState::Connected => {
                        self.reconnecting = false;
                        self.reconnect_attempt = 0;
                        self.reconnect_task = None;
                    }
                    ConnectionState::Error(_) if self.reconnecting => {
                        self.reconnect_attempt += 1;
                        self.schedule_reconnect(cx);
                        return;
                    }
                    _ => {}
                }
                self.connection_state = state;
                self.refresh_ticker(cx);
                cx.notify();
            }
            SessionEvent::ConnectionLost => {
                if !self.reconnecting {
                    self.reconnect_attempt = 0;
                    self.schedule_reconnect(cx);
                }
            }
            SessionEvent::Status(status) => {
                crate::perf::mark("first_status");
                self.last_known_elapsed = status.elapsed.unwrap_or(0.0);
                self.last_sync = Instant::now();
                self.status = status;
                self.refresh_ticker(cx);
                cx.notify();
            }
            SessionEvent::CurrentSong(song) => {
                let changed =
                    song.as_ref().map(|s| &s.uri) != self.current_song.as_ref().map(|s| &s.uri);
                self.current_song = song;
                if changed {
                    self.cover_art = None;
                    self.backdrop = None;
                    if let Some(uri) = self.current_song.as_ref().map(|s| s.uri.clone()) {
                        if let Some(img) = self.cover_cache.get(&uri) {
                            self.cover_art = Some(img.clone());
                            self.spawn_backdrop(uri, img.bytes.clone(), cx);
                        } else if !self.cover_cache.is_known_miss(&uri) {
                            self.client.request_art(uri, ArtKind::NowPlaying);
                        }
                    }
                }
                cx.notify();
            }
            SessionEvent::Queue(songs) => {
                self.queue = songs;
                self.queue_loaded = true;
                cx.notify();
            }
            SessionEvent::Library(songs) => {
                self.albums = library_index::albums(&songs);
                self.library = songs;
                self.library_loading = false;
                self.library_loaded = true;
                self.library_version += 1;
                cx.notify();
            }
            SessionEvent::Art { uri, kind, data } => {
                let image = match data {
                    Some(bytes) => {
                        let img = self.cover_cache.store(&uri, bytes);
                        if img.is_none() {
                            self.cover_cache.note_miss(&uri);
                        }
                        img
                    }
                    None => {
                        self.cover_cache.note_miss(&uri);
                        None
                    }
                };
                match kind {
                    ArtKind::NowPlaying => {
                        // Stale-response guard: the track may have changed meanwhile.
                        if self.current_song.as_ref().map(|s| &s.uri) == Some(&uri) {
                            self.cover_art = image.clone();
                            if let Some(img) = image {
                                self.spawn_backdrop(uri, img.bytes.clone(), cx);
                            }
                        }
                    }
                    ArtKind::Album => {
                        if let Some(album) = self.pending_album_art.remove(&uri) {
                            self.album_art.insert(album, image);
                        }
                    }
                }
                cx.notify();
            }
            SessionEvent::Outputs(outputs) => {
                self.outputs = outputs;
                if self.post_wake_recovery {
                    self.post_wake_recovery = false;
                    self.recover_after_wake(cx);
                }
                if let Some((name, _)) = &self.last_output_error {
                    if self.outputs.iter().any(|o| &o.name == name && o.enabled) {
                        self.last_output_error = None;
                    }
                }
                cx.notify();
            }
            SessionEvent::ReplayGain(mode) => {
                self.replay_gain_mode = mode;
                cx.notify();
            }
            SessionEvent::Error(message) => {
                eprintln!("[mpd] {message}");
                // `Failed to enable output "Name" (osx); ...` → remember it per output.
                if message.contains("output") {
                    let name = message
                        .split_once('"')
                        .and_then(|(_, rest)| rest.split_once('"'))
                        .map(|(n, _)| n.to_owned());
                    if let Some(name) = name {
                        self.last_output_error = Some((name, message.clone()));
                    }
                }
                self.last_error = Some(message);
                self._error_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(ERROR_BANNER_TTL).await;
                    let _ = this.update(cx, |s, cx| {
                        s.last_error = None;
                        cx.notify();
                    });
                }));
                cx.notify();
            }
        }
    }

    fn spawn_backdrop(&mut self, uri: String, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let work = cx
            .background_executor()
            .spawn(async move { cover_cache::blurred_backdrop(&bytes) });
        self._backdrop_task = Some(cx.spawn(async move |this, cx| {
            let backdrop = work.await;
            let _ = this.update(cx, |s, cx| {
                if s.current_song.as_ref().map(|s| &s.uri) == Some(&uri) {
                    s.backdrop = backdrop;
                    cx.notify();
                }
            });
        }));
    }

    // ----- Progress interpolation -------------------------------------------

    /// Elapsed seconds as shown: interpolated while playing and connected.
    pub fn display_elapsed(&self) -> f64 {
        if self.status.state == PlayState::Play && self.connection_state.is_connected() {
            let e = self.last_known_elapsed + self.last_sync.elapsed().as_secs_f64();
            match self.status.duration {
                Some(d) if d > 0.0 => e.min(d),
                _ => e,
            }
        } else {
            self.last_known_elapsed
        }
    }

    fn refresh_ticker(&mut self, cx: &mut Context<Self>) {
        let should_run =
            self.status.state == PlayState::Play && self.connection_state.is_connected();
        if should_run && self.ticker.is_none() {
            self.ticker = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        } else if !should_run {
            self.ticker = None;
        }
    }

    // ----- Playback intents (thin wrappers) ---------------------------------

    pub fn toggle_play_pause(&self) {
        if self.status.state == PlayState::Play {
            self.client.send(Command::Pause(true));
        } else {
            self.client.send(Command::Play);
        }
    }
    pub fn next(&self) {
        self.client.send(Command::Next);
    }
    pub fn previous(&self) {
        self.client.send(Command::Previous);
    }
    pub fn play_id(&self, id: u32) {
        self.client.send(Command::PlayId(id));
    }
    pub fn seek_to(&self, seconds: f64) {
        self.client.send(Command::SeekCurrent(seconds));
    }
    pub fn set_volume(&self, volume: i32) {
        if self.bit_perfect_mode {
            return;
        }
        self.client.send(Command::SetVolume(volume.clamp(0, 100)));
    }
    pub fn volume_delta(&self, delta: i32) {
        if self.status.volume >= 0 {
            self.set_volume(self.status.volume + delta);
        }
    }
    pub fn toggle_random(&self) {
        self.client.send(Command::SetRandom(!self.status.random_on));
    }
    /// Off → repeat all → repeat one → off (matches the iOS/macOS control).
    pub fn cycle_repeat(&self) {
        let st = &self.status;
        if !st.repeat_on {
            self.client.send(Command::SetRepeat(true));
        } else if st.single_mode == "0" {
            self.client.send(Command::SetSingle("1".into()));
        } else {
            self.client.send(Command::SetRepeat(false));
            self.client.send(Command::SetSingle("0".into()));
        }
    }
    pub fn set_consume(&self, on: bool) {
        self.client
            .send(Command::SetConsume(if on { "1" } else { "0" }.into()));
    }
    pub fn set_crossfade(&self, seconds: u32) {
        self.client.send(Command::SetCrossfade(seconds.min(30)));
    }
    pub fn clear_queue(&self) {
        self.client.send(Command::ClearQueue);
    }
    pub fn play_album(&self, name: &str) {
        self.client.send(Command::PlayAlbum(name.to_owned()));
    }
    pub fn add_album(&self, name: &str) {
        self.client.send(Command::AddAlbum(name.to_owned()));
    }
    pub fn play_uri(&self, uri: &str) {
        self.client.send(Command::PlayUri(uri.to_owned()));
    }

    // ----- Audio: outputs, options, bit-perfect -----------------------------

    pub fn route_to_output(&self, id: u32) {
        self.client.send(Command::RouteToOutput(id));
    }
    pub fn set_output_enabled(&self, id: u32, enabled: bool) {
        self.client.send(if enabled {
            Command::EnableOutput(id)
        } else {
            Command::DisableOutput(id)
        });
    }
    pub fn set_output_attribute(&self, id: u32, name: &str, value: &str) {
        self.client.send(Command::OutputSet {
            id,
            name: name.to_owned(),
            value: value.to_owned(),
        });
    }
    pub fn set_replay_gain(&self, mode: &str) {
        self.client.send(Command::SetReplayGain(mode.to_owned()));
    }
    pub fn disable_mixramp(&self) {
        self.client.send(Command::SetMixRampDelay(None));
        self.client.send(Command::RefreshOptions);
    }
    pub fn has_mixer(&self) -> bool {
        self.status.volume >= 0
    }
    /// Re-sends the bit-perfect protocol settings (RG off, xfade 0, MixRamp off, vol 100).
    pub fn apply_bit_perfect(&self) {
        self.client.send(Command::ApplyBitPerfect {
            has_mixer: self.has_mixer(),
        });
    }
    pub fn set_bit_perfect(&mut self, on: bool, cx: &mut Context<Self>) {
        self.bit_perfect_mode = on;
        self.persist();
        if on {
            self.apply_bit_perfect();
        }
        cx.notify();
    }
    /// Applies a Signal Path fix. Returns true when the caller should open
    /// Settings › Local MPD instead.
    pub fn apply_fix(&self, action: &FixAction) -> bool {
        match action {
            FixAction::ReplayGainOff => self.set_replay_gain("off"),
            FixAction::CrossfadeOff => self.client.send(Command::SetCrossfade(0)),
            FixAction::MixRampOff => self.disable_mixramp(),
            FixAction::VolumeFull => self.client.send(Command::SetVolume(100)),
            FixAction::EnableOutput(id) => self.set_output_enabled(*id, true),
            FixAction::ClearError => self.client.send(Command::ClearError),
            FixAction::CycleOutput(id) => self.client.send(Command::CycleOutput(*id)),
            FixAction::OpenLocalMpd => return true,
        }
        false
    }
    pub fn signal_path(&self) -> SignalPath {
        let infos = self.output_infos();
        signal_path::compute(
            &self.status,
            self.current_song.as_ref(),
            &infos,
            &self.replay_gain_mode,
            self.local_mpd.as_ref().and_then(|l| l.conf.as_ref()),
        )
    }
    /// True when bit-perfect mode is on but the server drifted (another
    /// client re-enabled a DSP stage or moved the volume).
    pub fn bit_perfect_drifted(&self) -> bool {
        self.bit_perfect_mode
            && (self.replay_gain_mode != "off"
                || self.status.crossfade > 0
                || self.status.mixrampdelay.is_some()
                || (self.status.volume >= 0 && self.status.volume < 100))
    }

    /// Short label for the transport bar: the CoreAudio device for the local
    /// osx output when known, otherwise the MPD output name(s).
    pub fn output_label(&self) -> String {
        let enabled: Vec<&Output> = self.outputs.iter().filter(|o| o.enabled).collect();
        match enabled.as_slice() {
            [] if self.outputs.is_empty() => "No outputs".into(),
            [] => "No output".into(),
            [one] => self.output_display_name(one),
            many => format!("{} outputs", many.len()),
        }
    }

    /// Device-aware name for one output.
    pub fn output_display_name(&self, o: &Output) -> String {
        if let Some(osx) = self.osx_block_for(o) {
            return match &osx.device {
                Some(d) => d.clone(),
                None => match self.audio_devices.iter().find(|d| d.is_default) {
                    Some(d) => format!("System default ({})", d.name),
                    None => "System default output".into(),
                },
            };
        }
        o.name.clone()
    }

    /// The local `audio_output { type "osx" }` block behind an MPD output.
    pub fn osx_block_for(&self, o: &Output) -> Option<&OsxOutput> {
        if o.plugin != "osx" {
            return None;
        }
        let conf = self.local_mpd.as_ref()?.conf.as_ref()?;
        conf.osx_for_output(&o.name).or_else(|| {
            // Legacy single block whose name doesn't match: only if it's the only one.
            if conf.osx_outputs.len() == 1 {
                conf.osx_outputs.first()
            } else {
                None
            }
        })
    }

    /// Live CoreAudio device behind an MPD output (local osx outputs only).
    pub fn output_device(&self, o: &Output) -> Option<&CaDevice> {
        let osx = self.osx_block_for(o)?;
        match &osx.device {
            Some(name) => self.audio_devices.iter().find(|d| &d.name == name),
            None => self.audio_devices.iter().find(|d| d.is_default),
        }
    }

    /// Outputs enriched with conf block + live device, for the Signal Path.
    pub fn output_infos(&self) -> Vec<OutputInfo> {
        self.outputs
            .iter()
            .map(|o| OutputInfo {
                output: o.clone(),
                osx: self.osx_block_for(o).cloned(),
                device: self.output_device(o).cloned(),
            })
            .collect()
    }

    /// True when the local server exposes osx outputs (i.e. the editor applies).
    pub fn has_local_osx_outputs(&self) -> bool {
        self.local_mpd
            .as_ref()
            .and_then(|l| l.conf.as_ref())
            .map(|c| !c.osx_outputs.is_empty())
            .unwrap_or(false)
    }

    /// After a wake: MPD may still hold an exclusive claim on a DAC that macOS
    /// re-enumerated (`!hog` open failures). Cycling the local osx outputs
    /// releases the stale claim and re-acquires the device.
    fn recover_after_wake(&mut self, cx: &mut Context<Self>) {
        let mut reopened: Vec<String> = Vec::new();
        if self.is_local_server() {
            let targets: Vec<(u32, String)> = self
                .outputs
                .iter()
                .filter(|o| {
                    o.enabled && self.osx_block_for(o).map(|x| x.hog_device).unwrap_or(false)
                })
                .map(|o| (o.id, o.name.clone()))
                .collect();
            for (id, name) in targets {
                self.client.send(Command::CycleOutput(id));
                reopened.push(name);
            }
            if self.status.error_message.is_some() {
                self.client.send(Command::ClearError);
            }
        }
        self.client.send(Command::RefreshAll);
        let notice = if reopened.is_empty() {
            "Reconnected after sleep".to_string()
        } else {
            format!(
                "Reconnected after sleep · re-opened {}",
                reopened.join(", ")
            )
        };
        eprintln!("[melo] {notice}");
        self.wake_notice = Some(notice);
        self._wake_notice_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(8)).await;
            let _ = this.update(cx, |s, cx| {
                s.wake_notice = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn is_local_server(&self) -> bool {
        self.active_profile
            .as_ref()
            .map(|p| local_mpd::is_local_host(&p.host))
            .unwrap_or(false)
    }

    /// Re-detects the local daemon/config on a background thread (only when
    /// the active server is this machine; otherwise clears it).
    pub fn refresh_local_mpd(&mut self, cx: &mut Context<Self>) {
        if !self.is_local_server() {
            self.local_mpd = None;
            return;
        }
        let work = cx
            .background_executor()
            .spawn(async move { local_mpd::detect() });
        self._local_task = Some(cx.spawn(async move |this, cx| {
            let detected = work.await;
            let _ = this.update(cx, |s, cx| {
                s.local_mpd = Some(detected);
                s.load_audio_devices(cx);
                cx.notify();
            });
        }));
    }

    /// Refreshes the CoreAudio device list (cheap, synchronous). While a local
    /// server is active this is polled every 2 s so rate/hog state stays live.
    pub fn load_audio_devices(&mut self, cx: &mut Context<Self>) {
        self.audio_devices = coreaudio::output_devices();
        self.audio_devices_loading = false;
        if self._devices_task.is_none() && self.local_mpd.is_some() {
            self._devices_task = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                    let alive = this.update(cx, |s, cx| {
                        if s.local_mpd.is_none() {
                            s._devices_task = None;
                            return false;
                        }
                        let fresh = coreaudio::output_devices();
                        if fresh != s.audio_devices {
                            s.audio_devices = fresh;
                            cx.notify();
                        }
                        true
                    });
                    if !matches!(alive, Ok(true)) {
                        break;
                    }
                }
            }));
        }
        cx.notify();
    }

    // ----- Library ----------------------------------------------------------

    pub fn load_library(&mut self, cx: &mut Context<Self>) {
        if self.library_loading || !self.connection_state.is_connected() {
            return;
        }
        self.library_loading = true;
        self.client.send(Command::FetchLibrary);
        cx.notify();
    }

    /// Ensures art for an album is cached or requested. Safe to call from
    /// render code (no notify; the Art event notifies when it lands).
    pub fn ensure_album_art(&mut self, album: &LibraryAlbum) {
        if self.album_art.contains_key(&album.name) {
            return;
        }
        let uri = album.representative_uri.clone();
        if let Some(img) = self.cover_cache.get(&uri) {
            self.album_art.insert(album.name.clone(), Some(img));
        } else if self.cover_cache.is_known_miss(&uri) {
            self.album_art.insert(album.name.clone(), None);
        } else if !self.pending_album_art.contains_key(&uri) {
            self.pending_album_art
                .insert(uri.clone(), album.name.clone());
            self.client.request_art(uri, ArtKind::Album);
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connection_state.is_connected()
    }
}
