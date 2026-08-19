//! Pure grouping/sorting helpers over the flat `listallinfo` song list
//! (port of MeloCore's `LibraryIndex`). O(n) — compute once per load.

use crate::mpd::Song;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryAlbum {
    pub name: String,
    pub artist: Option<String>,
    pub representative_uri: String,
}

pub fn albums(songs: &[Song]) -> Vec<LibraryAlbum> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for song in songs {
        let name = song.album.clone().unwrap_or_else(|| "Unknown Album".into());
        if !seen.insert(name.clone()) {
            continue;
        }
        result.push(LibraryAlbum {
            name,
            artist: song.album_artist.clone().or_else(|| song.artist.clone()),
            representative_uri: song.uri.clone(),
        });
    }
    result.sort_by_key(|a| a.name.to_lowercase());
    result
}

fn leading_int(v: &Option<String>, default: i64) -> i64 {
    v.as_deref()
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default)
}

/// Songs belonging to `album_name`, in disc-then-track order.
pub fn songs_in_album<'a>(album_name: &str, songs: &'a [Song]) -> Vec<&'a Song> {
    let mut out: Vec<&Song> = songs
        .iter()
        .filter(|s| s.album.as_deref().unwrap_or("Unknown Album") == album_name)
        .collect();
    out.sort_by_key(|s| (leading_int(&s.disc, 1), leading_int(&s.track, 0)));
    out
}

/// "N songs · H hr M min" like the SwiftUI album header.
pub fn album_summary(songs: &[&Song]) -> String {
    let total: f64 = songs.iter().map(|s| s.duration).sum();
    let mins = (total / 60.0).round() as i64;
    let duration = if mins >= 60 {
        format!("{} hr {} min", mins / 60, mins % 60)
    } else {
        format!("{mins} min")
    };
    let n = songs.len();
    format!("{n} {} · {duration}", if n == 1 { "song" } else { "songs" })
}

pub fn format_time(seconds: f64) -> String {
    let t = seconds.max(0.0) as i64;
    format!("{}:{:02}", t / 60, t % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(uri: &str, album: Option<&str>, disc: Option<&str>, track: Option<&str>) -> Song {
        Song {
            uri: uri.into(),
            title: None,
            artist: Some("A".into()),
            album_artist: None,
            album: album.map(str::to_owned),
            track: track.map(str::to_owned),
            disc: disc.map(str::to_owned),
            date: None,
            genre: None,
            duration: 100.0,
            format: None,
            queue_position: None,
            queue_id: None,
        }
    }

    #[test]
    fn albums_dedupe_and_sort_case_insensitively() {
        let songs = vec![
            song("1", Some("beta"), None, None),
            song("2", Some("Alpha"), None, None),
            song("3", Some("beta"), None, None),
            song("4", None, None, None),
        ];
        let a = albums(&songs);
        assert_eq!(
            a.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            vec!["Alpha", "beta", "Unknown Album"]
        );
        assert_eq!(a[1].representative_uri, "1");
    }

    #[test]
    fn songs_sorted_by_disc_then_track_with_slash_forms() {
        let songs = vec![
            song("c", Some("X"), Some("2/2"), Some("1/10")),
            song("b", Some("X"), Some("1"), Some("10/10")),
            song("a", Some("X"), None, Some("2")),
        ];
        let s = songs_in_album("X", &songs);
        assert_eq!(
            s.iter().map(|x| x.uri.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(album_summary(&s), "3 songs · 5 min");
    }

    #[test]
    fn formats_time() {
        assert_eq!(format_time(65.7), "1:05");
        assert_eq!(format_time(-3.0), "0:00");
    }
}
