//! Session management: three sockets (command / idle / cover art) on
//! background threads, fire-and-forget commands in, typed events out.
//!
//! Every event is tagged with the session generation so the UI can drop
//! stragglers from a session that has since been torn down.

use super::connection::{Connection, Killer, MpdError, assemble_binary};
use super::models::{ConnectionState, Output, ServerProfile, Song, Status};
use super::{parser, protocol};
use futures::channel::mpsc::UnboundedSender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const LIBRARY_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Command {
    Play,
    PlayId(u32),
    Pause(bool),
    Stop,
    Next,
    Previous,
    SeekCurrent(f64),
    SetVolume(i32),
    SetRepeat(bool),
    SetRandom(bool),
    SetSingle(String),
    SetConsume(String),
    SetCrossfade(u32),
    RefreshStatus,
    RefreshStatusAndSong,
    RefreshQueue,
    RefreshAll,
    AddId(String),
    DeleteId(u32),
    MoveId {
        id: u32,
        position: usize,
    },
    ClearQueue,
    FetchLibrary,
    /// `findadd Album "<name>"`
    AddAlbum(String),
    /// clear → findadd Album → play
    PlayAlbum(String),
    /// clear → addid → play
    PlayUri(String),
    // Outputs
    RefreshOutputs,
    EnableOutput(u32),
    DisableOutput(u32),
    /// Enable `id`, disable every other output (exclusive routing).
    RouteToOutput(u32),
    OutputSet {
        id: u32,
        name: String,
        value: String,
    },
    // Options (replay gain, mixramp) — status + replay_gain_status
    RefreshOptions,
    SetReplayGain(String),
    SetMixRampDelay(Option<f64>),
    /// replay_gain_mode off · crossfade 0 · mixrampdelay nan · setvol 100 (if a mixer exists)
    ApplyBitPerfect {
        has_mixer: bool,
    },
    ClearError,
    /// disableoutput → enableoutput: releases and re-acquires the device
    /// (fixes MPD's stale hog claim after sleep / USB re-enumeration).
    CycleOutput(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtKind {
    NowPlaying,
    Album,
}

#[derive(Clone, Debug)]
pub struct ArtRequest {
    pub uri: String,
    pub kind: ArtKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredServer {
    pub name: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug)]
pub enum SessionEvent {
    State(ConnectionState),
    /// A socket died. The owner decides whether/when to reconnect.
    ConnectionLost,
    Status(Status),
    CurrentSong(Option<Song>),
    Queue(Vec<Song>),
    Library(Vec<Song>),
    Outputs(Vec<Output>),
    ReplayGain(String),
    Art {
        uri: String,
        kind: ArtKind,
        data: Option<Vec<u8>>,
    },
    Error(String),
}

#[derive(Debug)]
pub enum MpdEvent {
    Session {
        generation: u64,
        event: SessionEvent,
    },
    Discovered(Vec<DiscoveredServer>),
    /// The Mac woke from sleep (NSWorkspaceDidWakeNotification).
    SystemWoke,
    /// The Mac is about to sleep.
    SystemWillSleep,
}

struct Session {
    generation: u64,
    cmd_tx: mpsc::Sender<Command>,
    art_tx: mpsc::Sender<ArtRequest>,
    killers: Arc<Mutex<Vec<Killer>>>,
    cancelled: Arc<AtomicBool>,
}

pub struct MpdClient {
    events: UnboundedSender<MpdEvent>,
    session: Option<Session>,
    generation: u64,
}

#[derive(Clone)]
struct Emitter {
    events: UnboundedSender<MpdEvent>,
    generation: u64,
    cancelled: Arc<AtomicBool>,
}

impl Emitter {
    fn emit(&self, event: SessionEvent) {
        if self.cancelled.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.events.unbounded_send(MpdEvent::Session {
            generation: self.generation,
            event,
        });
    }
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl MpdClient {
    pub fn new(events: UnboundedSender<MpdEvent>) -> Self {
        MpdClient {
            events,
            session: None,
            generation: 0,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Tears down any existing session and opens a new one on background
    /// threads. Progress is reported through `SessionEvent::State`.
    pub fn connect(&mut self, profile: ServerProfile) {
        self.disconnect();
        self.generation += 1;
        let generation = self.generation;

        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let (art_tx, art_rx) = mpsc::channel::<ArtRequest>();
        let killers: Arc<Mutex<Vec<Killer>>> = Arc::default();
        let cancelled = Arc::new(AtomicBool::new(false));

        let emitter = Emitter {
            events: self.events.clone(),
            generation,
            cancelled: cancelled.clone(),
        };

        // Initial refresh queued before the command thread even exists.
        let _ = cmd_tx.send(Command::RefreshAll);

        self.session = Some(Session {
            generation,
            cmd_tx: cmd_tx.clone(),
            art_tx,
            killers: killers.clone(),
            cancelled,
        });

        thread::Builder::new()
            .name(format!("mpd-connect-{generation}"))
            .spawn(move || {
                run_connector(profile, emitter, cmd_tx, cmd_rx, art_rx, killers);
            })
            .expect("spawn connector thread");
    }

    pub fn disconnect(&mut self) {
        if let Some(session) = self.session.take() {
            session.cancelled.store(true, Ordering::SeqCst);
            for k in session.killers.lock().unwrap().iter() {
                k.kill();
            }
            let _ = self.events.unbounded_send(MpdEvent::Session {
                generation: session.generation,
                event: SessionEvent::State(ConnectionState::Disconnected),
            });
        }
    }

    pub fn send(&self, cmd: Command) {
        if let Some(s) = &self.session {
            let _ = s.cmd_tx.send(cmd);
        }
    }

    pub fn request_art(&self, uri: impl Into<String>, kind: ArtKind) {
        if let Some(s) = &self.session {
            let _ = s.art_tx.send(ArtRequest {
                uri: uri.into(),
                kind,
            });
        }
    }
}

impl Drop for MpdClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn run_connector(
    profile: ServerProfile,
    emitter: Emitter,
    cmd_tx: mpsc::Sender<Command>,
    cmd_rx: mpsc::Receiver<Command>,
    art_rx: mpsc::Receiver<ArtRequest>,
    killers: Arc<Mutex<Vec<Killer>>>,
) {
    emitter.emit(SessionEvent::State(ConnectionState::Connecting));
    let pw = profile.password.as_deref();

    let open =
        |timeout: Option<Duration>| Connection::open(&profile.host, profile.port, pw, timeout);

    let (command, k1) = match open(Some(COMMAND_TIMEOUT)) {
        Ok(c) => c,
        Err(e) => {
            emitter.emit(SessionEvent::State(ConnectionState::Error(e.to_string())));
            return;
        }
    };
    let (idle, k2) = match open(None) {
        Ok(c) => c,
        Err(e) => {
            emitter.emit(SessionEvent::State(ConnectionState::Error(e.to_string())));
            return;
        }
    };
    let (art, k3) = match open(Some(COMMAND_TIMEOUT)) {
        Ok(c) => c,
        Err(e) => {
            emitter.emit(SessionEvent::State(ConnectionState::Error(e.to_string())));
            return;
        }
    };

    {
        let mut ks = killers.lock().unwrap();
        ks.extend([k1, k2, k3]);
    }
    if emitter.is_cancelled() {
        for k in killers.lock().unwrap().iter() {
            k.kill();
        }
        return;
    }

    emitter.emit(SessionEvent::State(ConnectionState::Connected));

    let g = emitter.generation;
    let e1 = emitter.clone();
    thread::Builder::new()
        .name(format!("mpd-command-{g}"))
        .spawn(move || run_command_loop(command, cmd_rx, e1))
        .expect("spawn command thread");
    let e2 = emitter.clone();
    thread::Builder::new()
        .name(format!("mpd-idle-{g}"))
        .spawn(move || run_idle_loop(idle, cmd_tx, e2))
        .expect("spawn idle thread");
    let e3 = emitter;
    thread::Builder::new()
        .name(format!("mpd-art-{g}"))
        .spawn(move || run_art_loop(art, art_rx, e3))
        .expect("spawn art thread");
}

fn run_command_loop(mut conn: Connection, rx: mpsc::Receiver<Command>, emitter: Emitter) {
    while let Ok(cmd) = rx.recv() {
        if emitter.is_cancelled() {
            return;
        }
        match execute(&mut conn, &cmd, &emitter) {
            Ok(()) => {}
            Err(e) if e.is_connection_error() => {
                emitter.emit(SessionEvent::ConnectionLost);
                return;
            }
            Err(e) => emitter.emit(SessionEvent::Error(e.to_string())),
        }
    }
}

fn execute(conn: &mut Connection, cmd: &Command, emitter: &Emitter) -> Result<(), MpdError> {
    use Command::*;
    let simple = |conn: &mut Connection, s: String| conn.command(&s).map(|_| ());
    match cmd {
        Play => simple(conn, protocol::play()),
        PlayId(id) => simple(conn, protocol::play_id(*id)),
        Pause(on) => simple(conn, protocol::pause(*on)),
        Stop => simple(conn, protocol::stop()),
        Next => simple(conn, protocol::next()),
        Previous => simple(conn, protocol::previous()),
        SeekCurrent(s) => simple(conn, protocol::seek_current(*s)),
        SetVolume(v) => simple(conn, protocol::set_volume((*v).clamp(0, 100))),
        SetRepeat(on) => simple(conn, protocol::set_repeat(*on)),
        SetRandom(on) => simple(conn, protocol::set_random(*on)),
        SetSingle(s) => simple(conn, protocol::set_single(s)),
        SetConsume(s) => simple(conn, protocol::set_consume(s)),
        SetCrossfade(s) => simple(conn, protocol::set_crossfade(*s)),
        AddId(uri) => simple(conn, protocol::add_id(uri)),
        DeleteId(id) => simple(conn, protocol::delete_id(*id)),
        MoveId { id, position } => simple(conn, protocol::move_id(*id, *position)),
        ClearQueue => simple(conn, protocol::clear_queue()),
        AddAlbum(name) => simple(conn, protocol::find_add("Album", name)),
        PlayAlbum(name) => {
            conn.command(&protocol::clear_queue())?;
            conn.command(&protocol::find_add("Album", name))?;
            simple(conn, protocol::play())
        }
        PlayUri(uri) => {
            conn.command(&protocol::clear_queue())?;
            conn.command(&protocol::add_id(uri))?;
            simple(conn, protocol::play())
        }
        RefreshStatus => {
            let status = parser::parse_status(&conn.command(&protocol::status())?);
            emitter.emit(SessionEvent::Status(status));
            Ok(())
        }
        RefreshStatusAndSong => {
            // Fetched back-to-back on the same socket so the pair is consistent.
            let status = parser::parse_status(&conn.command(&protocol::status())?);
            let song = parser::parse_song(&conn.command(&protocol::current_song())?);
            emitter.emit(SessionEvent::Status(status));
            emitter.emit(SessionEvent::CurrentSong(song));
            Ok(())
        }
        RefreshQueue => {
            let songs = parser::parse_songs(&conn.command(&protocol::playlist_info())?);
            emitter.emit(SessionEvent::Queue(songs));
            Ok(())
        }
        RefreshAll => {
            execute(conn, &RefreshStatusAndSong, emitter)?;
            execute(conn, &RefreshQueue, emitter)?;
            execute(conn, &RefreshOutputs, emitter)?;
            let rg = parser::parse_replay_gain(&conn.command(&protocol::replay_gain_status())?);
            emitter.emit(SessionEvent::ReplayGain(rg));
            Ok(())
        }
        RefreshOutputs => {
            let outputs = parser::parse_outputs(&conn.command(&protocol::outputs())?);
            emitter.emit(SessionEvent::Outputs(outputs));
            Ok(())
        }
        RefreshOptions => {
            let status = parser::parse_status(&conn.command(&protocol::status())?);
            emitter.emit(SessionEvent::Status(status));
            let rg = parser::parse_replay_gain(&conn.command(&protocol::replay_gain_status())?);
            emitter.emit(SessionEvent::ReplayGain(rg));
            Ok(())
        }
        EnableOutput(id) => simple(conn, protocol::enable_output(*id)),
        DisableOutput(id) => simple(conn, protocol::disable_output(*id)),
        RouteToOutput(id) => {
            let outputs = parser::parse_outputs(&conn.command(&protocol::outputs())?);
            // Enable the target first so playback never has zero outputs.
            conn.command(&protocol::enable_output(*id))?;
            for o in outputs.iter().filter(|o| o.id != *id && o.enabled) {
                conn.command(&protocol::disable_output(o.id))?;
            }
            Ok(())
        }
        OutputSet { id, name, value } => simple(conn, protocol::output_set(*id, name, value)),
        SetReplayGain(mode) => {
            conn.command(&protocol::replay_gain_mode(mode))?;
            // MPD does not always emit an idle event for replay gain; refresh explicitly.
            let rg = parser::parse_replay_gain(&conn.command(&protocol::replay_gain_status())?);
            emitter.emit(SessionEvent::ReplayGain(rg));
            Ok(())
        }
        SetMixRampDelay(delay) => simple(conn, protocol::mixramp_delay(*delay)),
        ClearError => {
            conn.command(&protocol::clear_error())?;
            execute(conn, &RefreshStatus, emitter)
        }
        CycleOutput(id) => {
            conn.command(&protocol::disable_output(*id))?;
            std::thread::sleep(Duration::from_millis(200));
            conn.command(&protocol::enable_output(*id))?;
            execute(conn, &RefreshOutputs, emitter)
        }
        ApplyBitPerfect { has_mixer } => {
            conn.command(&protocol::replay_gain_mode("off"))?;
            conn.command(&protocol::set_crossfade(0))?;
            conn.command(&protocol::mixramp_delay(None))?;
            if *has_mixer {
                conn.command(&protocol::set_volume(100))?;
            }
            execute(conn, &RefreshOptions, emitter)
        }
        FetchLibrary => {
            let _ = conn.set_read_timeout(Some(LIBRARY_TIMEOUT));
            let result = conn.command(&protocol::list_all_info());
            let _ = conn.set_read_timeout(Some(COMMAND_TIMEOUT));
            let songs = parser::parse_songs(&result?);
            emitter.emit(SessionEvent::Library(songs));
            Ok(())
        }
    }
}

fn run_idle_loop(mut conn: Connection, cmd_tx: mpsc::Sender<Command>, emitter: Emitter) {
    loop {
        if emitter.is_cancelled() {
            return;
        }
        match conn.command(&protocol::idle()) {
            Ok(lines) => {
                let subsystems = parser::parse_changed_subsystems(&lines);
                let mut want_status_song = false;
                let mut want_status = false;
                let mut want_queue = false;
                let mut want_outputs = false;
                for s in &subsystems {
                    match s.as_str() {
                        "player" | "mixer" => want_status_song = true,
                        "options" => want_status = true,
                        "playlist" => want_queue = true,
                        "output" => want_outputs = true,
                        _ => {}
                    }
                }
                if want_outputs {
                    let _ = cmd_tx.send(Command::RefreshOutputs);
                }
                if want_status_song {
                    let _ = cmd_tx.send(Command::RefreshStatusAndSong);
                }
                if want_status {
                    let _ = cmd_tx.send(Command::RefreshOptions);
                }
                if want_queue {
                    let _ = cmd_tx.send(Command::RefreshQueue);
                }
            }
            Err(e) => {
                if !emitter.is_cancelled() {
                    eprintln!("[mpd] idle loop error: {e}");
                    emitter.emit(SessionEvent::ConnectionLost);
                }
                return;
            }
        }
    }
}

fn run_art_loop(mut conn: Connection, rx: mpsc::Receiver<ArtRequest>, emitter: Emitter) {
    while let Ok(req) = rx.recv() {
        if emitter.is_cancelled() {
            return;
        }
        match fetch_album_art(&mut conn, &req.uri) {
            Ok(data) => emitter.emit(SessionEvent::Art {
                uri: req.uri,
                kind: req.kind,
                data: Some(data),
            }),
            Err(MpdError::NoArt) => emitter.emit(SessionEvent::Art {
                uri: req.uri,
                kind: req.kind,
                data: None,
            }),
            Err(e) if e.is_connection_error() => {
                emitter.emit(SessionEvent::ConnectionLost);
                return;
            }
            Err(e) => {
                eprintln!("[mpd] art error for {}: {e}", req.uri);
                emitter.emit(SessionEvent::Art {
                    uri: req.uri,
                    kind: req.kind,
                    data: None,
                })
            }
        }
    }
}

/// `albumart` first (folder cover), then `readpicture` (embedded tag).
fn fetch_album_art(conn: &mut Connection, uri: &str) -> Result<Vec<u8>, MpdError> {
    if let Some(data) = attempt_binary(conn, |off| protocol::album_art(uri, off))? {
        return Ok(data);
    }
    if let Some(data) = attempt_binary(conn, |off| protocol::read_picture(uri, off))? {
        return Ok(data);
    }
    Err(MpdError::NoArt)
}

/// One binary fetch; "no art" style server responses map to `Ok(None)` so the
/// caller can try a fallback command. Connection errors propagate.
fn attempt_binary(
    conn: &mut Connection,
    make: impl Fn(usize) -> String,
) -> Result<Option<Vec<u8>>, MpdError> {
    match assemble_binary(|off| conn.binary_command(&make(off))) {
        Ok(data) => Ok(data),
        Err(MpdError::Server { .. }) | Err(MpdError::Protocol(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use futures::StreamExt;

    /// Manual smoke test against a local mpd: `cargo test live_session -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_session() {
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let mut client = MpdClient::new(tx);
        client.connect(ServerProfile::manual("local", "127.0.0.1", 6600, None));
        client.send(Command::FetchLibrary);
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let mut got_status = false;
        let mut got_queue = false;
        let mut got_library = false;
        let mut got_art = false;
        futures::executor::block_on(async {
            while std::time::Instant::now() < deadline {
                let next = futures::future::poll_fn(|cx| match rx.poll_next_unpin(cx) {
                    std::task::Poll::Ready(v) => std::task::Poll::Ready(Some(v)),
                    std::task::Poll::Pending => std::task::Poll::Ready(None),
                })
                .await;
                match next {
                    Some(Some(MpdEvent::Session { event, .. })) => match &event {
                        SessionEvent::Status(s) => {
                            got_status = true;
                            println!(
                                "status: {:?} vol={} elapsed={:?}",
                                s.state, s.volume, s.elapsed
                            );
                        }
                        SessionEvent::CurrentSong(s) => {
                            println!("song: {:?}", s.as_ref().map(|s| s.display_title()));
                            if let Some(s) = s {
                                client.request_art(&s.uri, ArtKind::NowPlaying);
                            }
                        }
                        SessionEvent::Queue(q) => {
                            got_queue = true;
                            println!("queue: {} songs", q.len());
                        }
                        SessionEvent::Library(l) => {
                            got_library = true;
                            println!("library: {} songs", l.len());
                        }
                        SessionEvent::Art { uri, data, .. } => {
                            got_art = true;
                            println!("art for {uri}: {:?} bytes", data.as_ref().map(|d| d.len()));
                        }
                        other => println!("event: {other:?}"),
                    },
                    Some(Some(other)) => println!("{other:?}"),
                    _ => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        });
        client.disconnect();
        assert!(
            got_status && got_queue && got_library,
            "status={got_status} queue={got_queue} library={got_library} art={got_art}"
        );
    }
}
