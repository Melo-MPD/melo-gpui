//! Parses MPD key: value responses into typed models
//! (mirror of MeloCore's `MPDResponseParser`).

use super::models::{AudioFormat, Output, PlayState, Song, Status};
use std::collections::HashMap;

fn kv(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(':')?;
    Some((k.trim(), v.trim()))
}

fn dict(lines: &[String]) -> HashMap<&str, &str> {
    lines.iter().filter_map(|l| kv(l)).collect()
}

pub fn parse_status(lines: &[String]) -> Status {
    let d = dict(lines);
    let get = |k: &str| d.get(k).copied();
    fn num<T: std::str::FromStr>(v: Option<&str>) -> Option<T> {
        v.and_then(|v| v.parse().ok())
    }
    Status {
        state: PlayState::parse(get("state").unwrap_or("stop")),
        volume: num(get("volume")).unwrap_or(-1),
        repeat_on: get("repeat") == Some("1"),
        random_on: get("random") == Some("1"),
        single_mode: get("single").unwrap_or("0").to_owned(),
        consume_mode: get("consume").unwrap_or("0").to_owned(),
        crossfade: num(get("xfade")).unwrap_or(0),
        mixrampdb: num(get("mixrampdb")).unwrap_or(0.0),
        mixrampdelay: num::<f64>(get("mixrampdelay")).filter(|d| d.is_finite() && *d > 0.0),
        queue_version: num(get("playlist")).unwrap_or(0),
        queue_length: num(get("playlistlength")).unwrap_or(0),
        current_song_id: num(get("songid")),
        elapsed: num(get("elapsed")),
        duration: num(get("duration")),
        bitrate: num(get("bitrate")),
        audio_format: get("audio").and_then(parse_audio_format),
        error_message: get("error").map(str::to_owned),
    }
}

pub fn parse_song(lines: &[String]) -> Option<Song> {
    let d = dict(lines);
    let file = d.get("file")?;
    let s = |k: &str| d.get(k).map(|v| v.to_string());
    Some(Song {
        uri: file.to_string(),
        title: s("Title"),
        artist: s("Artist"),
        album_artist: s("AlbumArtist"),
        album: s("Album"),
        track: s("Track"),
        disc: s("Disc"),
        date: s("Date"),
        genre: s("Genre"),
        duration: d
            .get("duration")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
        format: d.get("Format").and_then(|v| parse_audio_format(v)),
        queue_position: d.get("Pos").and_then(|v| v.parse().ok()),
        queue_id: d.get("Id").and_then(|v| v.parse().ok()),
    })
}

/// Splits a multi-song response (`playlistinfo`, `listallinfo`, `search`) on
/// `file:` boundaries. Directory/playlist entries from `listallinfo` carry no
/// `file:` and are dropped.
pub fn parse_songs(lines: &[String]) -> Vec<Song> {
    let mut songs = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in lines {
        if line.starts_with("file:") && !current.is_empty() {
            if let Some(song) = parse_song(&current) {
                songs.push(song);
            }
            current.clear();
        }
        current.push(line.clone());
    }
    if let Some(song) = parse_song(&current) {
        songs.push(song);
    }
    songs
}

/// "44100:24:2" → AudioFormat
pub fn parse_audio_format(value: &str) -> Option<AudioFormat> {
    let mut parts = value.splitn(3, ':');
    let sample_rate = parts.next()?.parse().ok()?;
    let bits = parts.next()?.to_owned();
    let channels = parts.next()?.parse().ok()?;
    Some(AudioFormat {
        sample_rate,
        bits,
        channels,
    })
}

/// `outputs` response → outputs, split on `outputid:`.
pub fn parse_outputs(lines: &[String]) -> Vec<Output> {
    let mut outputs = Vec::new();
    let mut current: Option<Output> = None;
    for line in lines {
        let Some((k, v)) = kv(line) else { continue };
        match k {
            "outputid" => {
                if let Some(o) = current.take() {
                    outputs.push(o);
                }
                current = v.parse().ok().map(|id| Output {
                    id,
                    name: String::new(),
                    plugin: String::new(),
                    enabled: false,
                    attributes: Vec::new(),
                });
            }
            "outputname" => {
                if let Some(o) = current.as_mut() {
                    o.name = v.to_owned();
                }
            }
            "plugin" => {
                if let Some(o) = current.as_mut() {
                    o.plugin = v.to_owned();
                }
            }
            "outputenabled" => {
                if let Some(o) = current.as_mut() {
                    o.enabled = v == "1";
                }
            }
            "attribute" => {
                if let Some(o) = current.as_mut() {
                    let (name, value) = v.split_once('=').unwrap_or((v, ""));
                    o.attributes
                        .push((name.trim().to_owned(), value.trim().to_owned()));
                }
            }
            _ => {}
        }
    }
    if let Some(o) = current {
        outputs.push(o);
    }
    outputs
}

/// `replay_gain_status` → "off" | "track" | "album" | "auto"
pub fn parse_replay_gain(lines: &[String]) -> String {
    lines
        .iter()
        .filter_map(|l| kv(l))
        .find(|(k, _)| *k == "replay_gain_mode")
        .map(|(_, v)| v.to_owned())
        .unwrap_or_else(|| "off".into())
}

pub fn parse_changed_subsystems(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|l| kv(l))
        .filter(|(k, _)| *k == "changed")
        .map(|(_, v)| v.to_owned())
        .collect()
}

#[allow(dead_code)]
pub fn parse_add_id(lines: &[String]) -> Option<u32> {
    lines
        .iter()
        .filter_map(|l| kv(l))
        .find(|(k, _)| *k == "Id")
        .and_then(|(_, v)| v.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_owned).collect()
    }

    #[test]
    fn parses_status() {
        let st = parse_status(&lines(
            "volume: 45\nrepeat: 1\nrandom: 0\nsingle: oneshot\nconsume: 0\nplaylist: 12\nplaylistlength: 3\nstate: play\nsongid: 9\nelapsed: 12.345\nduration: 240.0\nbitrate: 1411\naudio: 44100:16:2\nxfade: 5",
        ));
        assert_eq!(st.state, PlayState::Play);
        assert_eq!(st.volume, 45);
        assert!(st.repeat_on);
        assert_eq!(st.single_mode, "oneshot");
        assert_eq!(st.current_song_id, Some(9));
        assert_eq!(st.elapsed, Some(12.345));
        assert_eq!(st.crossfade, 5);
        assert_eq!(
            st.audio_format.unwrap().display_string(),
            "44.1 kHz · 16-bit · Stereo"
        );
    }

    #[test]
    fn parses_queue_with_multiple_songs() {
        let songs = parse_songs(&lines(
            "file: a/1.flac\nTitle: One\nArtist: X\nduration: 10.5\nPos: 0\nId: 1\nfile: a/2.flac\nTitle: Two\nPos: 1\nId: 2",
        ));
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].queue_id, Some(1));
        assert_eq!(songs[1].display_title(), "Two");
        assert_eq!(songs[1].display_artist(), "Unknown Artist");
    }

    #[test]
    fn listallinfo_directories_are_skipped() {
        let songs = parse_songs(&lines(
            "directory: a\nLast-Modified: 2020\nfile: a/1.flac\nTitle: One\ndirectory: b\nfile: b/1.flac",
        ));
        assert_eq!(songs.len(), 2);
    }

    #[test]
    fn untitled_song_falls_back_to_filename() {
        let s = parse_song(&lines("file: Some Dir/My Track.mp3")).unwrap();
        assert_eq!(s.display_title(), "My Track.mp3");
    }

    #[test]
    fn parses_outputs_with_attributes() {
        let outs = parse_outputs(&lines(
            "outputid: 0\noutputname: Beelink Analog\nplugin: alsa\noutputenabled: 1\nattribute: allowed_formats=\nattribute: dop=0\noutputid: 1\noutputname: Stream\nplugin: httpd\noutputenabled: 0",
        ));
        assert_eq!(outs.len(), 2);
        assert_eq!(outs[0].name, "Beelink Analog");
        assert!(outs[0].enabled);
        assert_eq!(outs[0].attribute("dop"), Some("0"));
        assert_eq!(outs[0].attribute("allowed_formats"), Some(""));
        assert_eq!(outs[0].plugin_label(), "ALSA");
        assert_eq!(outs[1].id, 1);
        assert!(!outs[1].enabled);
    }

    #[test]
    fn parses_format_tag_and_mixramp() {
        let s = parse_song(&lines("file: a.flac\nFormat: 96000:24:2")).unwrap();
        assert_eq!(
            s.format.unwrap().display_string(),
            "96 kHz · 24-bit · Stereo"
        );
        let st = parse_status(&lines("state: stop\nmixrampdb: -17\nmixrampdelay: nan"));
        assert_eq!(st.mixrampdb, -17.0);
        assert_eq!(st.mixrampdelay, None);
        assert_eq!(
            parse_replay_gain(&lines("replay_gain_mode: album")),
            "album"
        );
    }

    #[test]
    fn parses_changed() {
        assert_eq!(
            parse_changed_subsystems(&lines("changed: player\nchanged: mixer")),
            vec!["player", "mixer"]
        );
    }
}
