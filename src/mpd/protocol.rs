//! Wire-format command builders (mirror of MeloCore's `MPDCommandBuilder`).

/// Escapes a value for use inside double quotes. Backslash and quote are
/// escaped; CR/LF are replaced by a space so a crafted value cannot terminate
/// the command early and inject a second one.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn q(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

// Playback
pub fn play() -> String {
    "play\n".into()
}
pub fn play_id(id: u32) -> String {
    format!("playid {id}\n")
}
pub fn pause(on: bool) -> String {
    format!("pause {}\n", on as u8)
}
pub fn stop() -> String {
    "stop\n".into()
}
pub fn next() -> String {
    "next\n".into()
}
pub fn previous() -> String {
    "previous\n".into()
}
pub fn seek_current(seconds: f64) -> String {
    format!("seekcur {seconds:.3}\n")
}

// Options
pub fn set_volume(volume: i32) -> String {
    format!("setvol {volume}\n")
}
pub fn set_repeat(on: bool) -> String {
    format!("repeat {}\n", on as u8)
}
pub fn set_random(on: bool) -> String {
    format!("random {}\n", on as u8)
}
pub fn set_single(state: &str) -> String {
    format!("single {state}\n")
}
pub fn set_consume(state: &str) -> String {
    format!("consume {state}\n")
}
pub fn set_crossfade(seconds: u32) -> String {
    format!("crossfade {seconds}\n")
}

// Info
pub fn clear_error() -> String {
    "clearerror\n".into()
}
pub fn status() -> String {
    "status\n".into()
}
pub fn current_song() -> String {
    "currentsong\n".into()
}
pub fn playlist_info() -> String {
    "playlistinfo\n".into()
}

// Queue
pub fn add_id(uri: &str) -> String {
    format!("addid {}\n", q(uri))
}
pub fn delete_id(id: u32) -> String {
    format!("deleteid {id}\n")
}
pub fn move_id(id: u32, position: usize) -> String {
    format!("moveid {id} {position}\n")
}
pub fn clear_queue() -> String {
    "clear\n".into()
}

// Idle
pub fn idle() -> String {
    "idle\n".into()
}
pub fn no_idle() -> String {
    "noidle\n".into()
}

// Cover art
pub fn album_art(uri: &str, offset: usize) -> String {
    format!("albumart {} {offset}\n", q(uri))
}
pub fn read_picture(uri: &str, offset: usize) -> String {
    format!("readpicture {} {offset}\n", q(uri))
}

// Library
pub fn search(query: &str) -> String {
    format!("search any {}\n", q(query))
}
pub fn list_all_info() -> String {
    "listallinfo\n".into()
}
pub fn find_add(tag: &str, value: &str) -> String {
    format!("findadd {tag} {}\n", q(value))
}

// Outputs
pub fn outputs() -> String {
    "outputs\n".into()
}
pub fn enable_output(id: u32) -> String {
    format!("enableoutput {id}\n")
}
pub fn disable_output(id: u32) -> String {
    format!("disableoutput {id}\n")
}
pub fn toggle_output(id: u32) -> String {
    format!("toggleoutput {id}\n")
}
pub fn output_set(id: u32, name: &str, value: &str) -> String {
    format!("outputset {id} {} {}\n", q(name), q(value))
}

// Replay gain / mixramp
pub fn replay_gain_status() -> String {
    "replay_gain_status\n".into()
}
pub fn replay_gain_mode(mode: &str) -> String {
    format!("replay_gain_mode {mode}\n")
}
pub fn mixramp_db(db: f64) -> String {
    format!("mixrampdb {db}\n")
}
/// `None` disables MixRamp (`mixrampdelay nan`).
pub fn mixramp_delay(seconds: Option<f64>) -> String {
    match seconds {
        Some(s) => format!("mixrampdelay {s}\n"),
        None => "mixrampdelay nan\n".into(),
    }
}

// Auth
pub fn password(pw: &str) -> String {
    format!("password {}\n", q(pw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn newline_injection_is_neutralised() {
        let cmd = add_id("x.flac\"\nclear\n");
        assert_eq!(
            cmd.matches('\n').count(),
            1,
            "exactly one terminating newline"
        );
        assert!(cmd.starts_with("addid \"x.flac\\\" clear \""));
    }

    #[test]
    fn builds_commands() {
        assert_eq!(play_id(7), "playid 7\n");
        assert_eq!(pause(true), "pause 1\n");
        assert_eq!(seek_current(12.5), "seekcur 12.500\n");
        assert_eq!(find_add("Album", "Kid A"), "findadd Album \"Kid A\"\n");
        assert_eq!(album_art("a/b.flac", 0), "albumart \"a/b.flac\" 0\n");
        assert_eq!(move_id(3, 1), "moveid 3 1\n");
        assert_eq!(output_set(1, "dop", "1"), "outputset 1 \"dop\" \"1\"\n");
        assert_eq!(mixramp_delay(None), "mixrampdelay nan\n");
    }
}
