use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlayState {
    Play,
    #[default]
    Stop,
    Pause,
}

impl PlayState {
    pub fn parse(s: &str) -> Self {
        match s {
            "play" => PlayState::Play,
            "pause" => PlayState::Pause,
            _ => PlayState::Stop,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    /// "16", "24", "32", "f" (float), "dsd64", ...
    pub bits: String,
    pub channels: u32,
}

impl AudioFormat {
    /// e.g. "44.1 kHz", "192 kHz"
    pub fn sample_rate_label(&self) -> String {
        let khz = self.sample_rate as f64 / 1000.0;
        if khz.fract() == 0.0 {
            format!("{} kHz", khz as u32)
        } else {
            format!("{khz:.1} kHz")
        }
    }

    pub fn bits_label(&self) -> String {
        match self.bits.as_str() {
            "f" => "Float".into(),
            "dsd64" => "DSD64".into(),
            "dsd128" => "DSD128".into(),
            "dsd256" => "DSD256".into(),
            "dsd512" => "DSD512".into(),
            other => format!("{other}-bit"),
        }
    }

    pub fn channels_label(&self) -> String {
        match self.channels {
            1 => "Mono".into(),
            2 => "Stereo".into(),
            n => format!("{n}ch"),
        }
    }

    /// e.g. "44.1 kHz · 24-bit · Stereo"
    pub fn display_string(&self) -> String {
        format!(
            "{} · {} · {}",
            self.sample_rate_label(),
            self.bits_label(),
            self.channels_label()
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub state: PlayState,
    /// -1 if no mixer
    pub volume: i32,
    pub repeat_on: bool,
    pub random_on: bool,
    /// "0", "1", "oneshot"
    pub single_mode: String,
    /// "0", "1", "oneshot"
    pub consume_mode: String,
    pub crossfade: u32,
    pub mixrampdb: f64,
    /// `None` = MixRamp disabled (MPD reports `nan` or omits it).
    pub mixrampdelay: Option<f64>,
    pub queue_version: u64,
    pub queue_length: usize,
    pub current_song_id: Option<u32>,
    pub elapsed: Option<f64>,
    pub duration: Option<f64>,
    pub bitrate: Option<u32>,
    pub audio_format: Option<AudioFormat>,
    pub error_message: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            state: PlayState::Stop,
            volume: -1,
            repeat_on: false,
            random_on: false,
            single_mode: "0".into(),
            consume_mode: "0".into(),
            crossfade: 0,
            mixrampdb: 0.0,
            mixrampdelay: None,
            queue_version: 0,
            queue_length: 0,
            current_song_id: None,
            elapsed: None,
            duration: None,
            bitrate: None,
            audio_format: None,
            error_message: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub uri: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track: Option<String>,
    pub disc: Option<String>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub duration: f64,
    /// Source format from the `Format:` tag (MPD 0.21+), e.g. 44100:16:2.
    pub format: Option<AudioFormat>,
    pub queue_position: Option<usize>,
    pub queue_id: Option<u32>,
}

impl Song {
    pub fn display_title(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        self.uri
            .rsplit('/')
            .next()
            .map(str::to_owned)
            .unwrap_or_else(|| self.uri.clone())
    }

    pub fn display_artist(&self) -> String {
        self.artist
            .clone()
            .or_else(|| self.album_artist.clone())
            .unwrap_or_else(|| "Unknown Artist".into())
    }
}

/// One MPD audio output (from `outputs`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub id: u32,
    pub name: String,
    /// "osx", "alsa", "pipewire", "pulse", "httpd", "null", ...
    pub plugin: String,
    pub enabled: bool,
    /// Runtime attributes (`attribute: name=value`), e.g. ALSA `dop`, `allowed_formats`.
    pub attributes: Vec<(String, String)>,
}

impl Output {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Human label for the plugin type.
    pub fn plugin_label(&self) -> &'static str {
        match self.plugin.as_str() {
            "osx" => "CoreAudio",
            "alsa" => "ALSA",
            "pipewire" => "PipeWire",
            "pulse" => "PulseAudio",
            "jack" => "JACK",
            "httpd" => "HTTP stream",
            "shout" => "Icecast",
            "snapcast" => "Snapcast",
            "null" => "Null",
            "fifo" => "FIFO",
            "pipe" => "Pipe",
            "recorder" => "Recorder",
            "ao" => "libao",
            "oss" => "OSS",
            "sndio" => "sndio",
            "wasapi" => "WASAPI",
            "winmm" => "WinMM",
            _ => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub is_manual: bool,
}

impl ServerProfile {
    pub fn manual(name: &str, host: &str, port: u16, password: Option<String>) -> Self {
        let name = if name.trim().is_empty() {
            host
        } else {
            name.trim()
        };
        ServerProfile {
            id: uuid::Uuid::new_v4().to_string().to_uppercase(),
            name: name.to_owned(),
            host: host.trim().to_owned(),
            port,
            password: password.filter(|p| !p.is_empty()),
            is_manual: true,
        }
    }

    pub fn discovered(name: &str, host: &str, port: u16) -> Self {
        ServerProfile {
            id: format!("bonjour:{name}"),
            name: name.to_owned(),
            host: host.to_owned(),
            port,
            password: None,
            is_manual: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl ConnectionState {
    pub fn is_connected(&self) -> bool {
        matches!(self, ConnectionState::Connected)
    }
}
