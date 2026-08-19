//! Local (Homebrew) MPD on this Mac: detect `mpd.conf`, read/write the keys
//! Melo cares about (structure-preserving), manage the `osx` audio_output
//! block, enumerate CoreAudio output devices and restart the daemon.
//!
//! Port of `Melo-macOS/Features/LocalMPDSettingsView.swift`'s `MPDConfManager`
//! plus the audio-output editor.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

/// What Melo needs to know about the local `osx` output for the Signal Path
/// and the settings editor.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OsxOutput {
    pub name: String,
    /// `device "…"` — `None` = system default device.
    pub device: Option<String>,
    pub hog_device: bool,
    pub dop: bool,
    /// "none" | "hardware" | "software" | None (MPD default: hardware if the
    /// plugin has a mixer, which osx does).
    pub mixer_type: Option<String>,
    /// Per-output `format "rate:bits:channels"` — forces conversion. Needed
    /// for Bluetooth/AirPlay (CoreAudio rejects other rates); poison for DACs.
    pub format: Option<String>,
}

/// Snapshot of the local config relevant to audio + the basic editor.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LocalConf {
    pub music_directory: String,
    pub bind_to_address: String,
    pub port: String,
    pub password: String,
    pub replaygain: String,
    pub volume_normalization: bool,
    pub audio_output_format: Option<String>,
    /// Every `type "osx"` block, in file order.
    pub osx_outputs: Vec<OsxOutput>,
    /// Other `audio_output` blocks: (type, name)
    pub other_outputs: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct LocalMpd {
    pub conf_path: Option<PathBuf>,
    pub running: bool,
    pub conf: Option<LocalConf>,
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

pub fn candidate_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".config/mpd/mpd.conf"));
        v.push(home.join(".mpdconf"));
    }
    v.push(PathBuf::from("/opt/homebrew/etc/mpd.conf"));
    v.push(PathBuf::from("/usr/local/etc/mpd.conf"));
    v
}

pub fn detect() -> LocalMpd {
    let conf_path = candidate_paths().into_iter().find(|p| p.exists());
    let running = is_running();
    let conf = conf_path
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|text| parse_conf(&text));
    LocalMpd {
        conf_path,
        running,
        conf,
    }
}

pub fn is_running() -> bool {
    Command::new("/usr/bin/pgrep")
        .args(["-x", "mpd"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Whether the given MPD host is this machine (so the local conf applies).
pub fn is_local_host(host: &str) -> bool {
    let h = host.trim().trim_end_matches('.').to_lowercase();
    if matches!(h.as_str(), "127.0.0.1" | "localhost" | "::1" | "0.0.0.0") {
        return true;
    }
    let mine = hostname().to_lowercase();
    !mine.is_empty()
        && (h == mine || h == format!("{mine}.local") || h.trim_end_matches(".local") == mine)
}

fn hostname() -> String {
    Command::new("/bin/hostname")
        .arg("-s")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_owned()
}

/// Splits `key   "value"` / `key value` (first whitespace run separates).
fn split_kv(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let idx = t.find(char::is_whitespace)?;
    Some((&t[..idx], t[idx..].trim()))
}

struct Block {
    name: String,
    /// line index of `name {`
    start: usize,
    /// line index of `}`
    end: usize,
    entries: Vec<(String, String)>,
}

fn find_blocks(lines: &[String]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut open: Option<(String, usize, Vec<(String, String)>)> = None;
    for (i, raw) in lines.iter().enumerate() {
        let t = raw.trim();
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        if let Some(name) = t.strip_suffix('{') {
            if open.is_none() {
                open = Some((name.trim().to_owned(), i, Vec::new()));
            }
            continue;
        }
        if t == "}" {
            if let Some((name, start, entries)) = open.take() {
                blocks.push(Block {
                    name,
                    start,
                    end: i,
                    entries,
                });
            }
            continue;
        }
        if let Some((_, _, entries)) = open.as_mut() {
            if let Some((k, v)) = split_kv(t) {
                entries.push((k.to_owned(), unquote(v)));
            }
        }
    }
    blocks
}

fn yes(v: Option<&String>) -> bool {
    matches!(
        v.map(|s| s.to_lowercase()).as_deref(),
        Some("yes") | Some("true") | Some("1")
    )
}

pub fn parse_conf(text: &str) -> LocalConf {
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut top: HashMap<String, String> = HashMap::new();
    let mut depth = 0i32;
    for raw in &lines {
        let t = raw.trim();
        if t.ends_with('{') {
            depth += 1;
            continue;
        }
        if t == "}" {
            depth = (depth - 1).max(0);
            continue;
        }
        if depth != 0 {
            continue;
        }
        if let Some((k, v)) = split_kv(t) {
            top.entry(k.to_owned()).or_insert_with(|| unquote(v));
        }
    }
    let mut osx_outputs = Vec::new();
    let mut other_outputs = Vec::new();
    for b in find_blocks(&lines)
        .into_iter()
        .filter(|b| b.name == "audio_output")
    {
        let get = |k: &str| {
            b.entries
                .iter()
                .find(|(kk, _)| kk == k)
                .map(|(_, v)| v.clone())
        };
        let ty = get("type").unwrap_or_default();
        if ty == "osx" {
            osx_outputs.push(OsxOutput {
                name: get("name").unwrap_or_else(|| "CoreAudio".into()),
                device: get("device").filter(|d| !d.is_empty()),
                hog_device: yes(get("hog_device").as_ref()),
                dop: yes(get("dop").as_ref()),
                mixer_type: get("mixer_type"),
                format: get("format").filter(|f| !f.is_empty()),
            });
        } else {
            other_outputs.push((ty, get("name").unwrap_or_default()));
        }
    }
    LocalConf {
        music_directory: top.get("music_directory").cloned().unwrap_or_default(),
        bind_to_address: top
            .get("bind_to_address")
            .cloned()
            .unwrap_or_else(|| "127.0.0.1".into()),
        port: top.get("port").cloned().unwrap_or_else(|| "6600".into()),
        password: top.get("password").cloned().unwrap_or_default(),
        replaygain: top
            .get("replaygain")
            .cloned()
            .unwrap_or_else(|| "off".into()),
        volume_normalization: yes(top.get("volume_normalization")),
        audio_output_format: top
            .get("audio_output_format")
            .cloned()
            .filter(|s| !s.is_empty()),
        osx_outputs,
        other_outputs,
    }
}

impl LocalConf {
    /// The osx block behind an MPD output name (blocks are keyed by `name`).
    pub fn osx_for_output(&self, output_name: &str) -> Option<&OsxOutput> {
        self.osx_outputs.iter().find(|o| o.name == output_name)
    }
    /// The (single) osx block Melo manages — the first one in the file.
    pub fn osx(&self) -> Option<&OsxOutput> {
        self.osx_outputs.first()
    }
}

// ---------------------------------------------------------------------------
// Writing (structure-preserving)
// ---------------------------------------------------------------------------

/// Updates top-level keys in place (un-commenting matching commented lines),
/// comments out keys whose value is `None`, appends missing keys.
/// Port of `MPDConfManager.writeConf`.
pub fn apply_top_level(text: &str, updates: &[(&str, Option<String>)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut touched: Vec<&str> = Vec::new();
    let mut depth = 0i32;
    for i in 0..lines.len() {
        let t = lines[i].trim().to_owned();
        if t.ends_with('{') {
            depth += 1;
            continue;
        }
        if t == "}" {
            depth = (depth - 1).max(0);
            continue;
        }
        if depth != 0 {
            continue;
        }
        let stripped = t
            .strip_prefix('#')
            .map(|s| s.trim().to_owned())
            .unwrap_or(t.clone());
        let Some(ws) = stripped.find(char::is_whitespace) else {
            continue;
        };
        let key = &stripped[..ws];
        let Some((_, new_value)) = updates.iter().find(|(k, _)| *k == key) else {
            continue;
        };
        touched.push(updates.iter().find(|(k, _)| *k == key).unwrap().0);
        match new_value {
            Some(v) => lines[i] = format!("{key}\t\t\"{v}\""),
            None => {
                let old = stripped[ws..].trim();
                lines[i] = format!("# {key}\t\t{old}");
            }
        }
    }
    for (key, value) in updates {
        if !touched.contains(key) {
            if let Some(v) = value {
                lines.push(format!("{key}\t\t\"{v}\""));
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn osx_block_body(osx: &OsxOutput) -> Vec<String> {
    let mut body: Vec<String> = vec![
        "    type\t\t\"osx\"".into(),
        format!(
            "    name\t\t\"{}\"",
            if osx.name.trim().is_empty() {
                "CoreAudio"
            } else {
                osx.name.trim()
            }
        ),
    ];
    if let Some(d) = osx.device.as_deref().filter(|d| !d.trim().is_empty()) {
        body.push(format!("    device\t\t\"{}\"", d.trim()));
    }
    body.push(format!(
        "    hog_device\t\"{}\"",
        if osx.hog_device { "yes" } else { "no" }
    ));
    body.push(format!(
        "    dop\t\t\"{}\"",
        if osx.dop { "yes" } else { "no" }
    ));
    if let Some(m) = osx.mixer_type.as_deref().filter(|m| !m.is_empty()) {
        body.push(format!("    mixer_type\t\"{m}\""));
    }
    if let Some(f) = osx.format.as_deref().filter(|f| !f.is_empty()) {
        body.push(format!("    format\t\t\"{f}\""));
    }
    body
}

const OSX_MANAGED_KEYS: [&str; 7] = [
    "type",
    "name",
    "device",
    "hog_device",
    "dop",
    "mixer_type",
    "format",
];

/// Makes the file's `type "osx"` blocks exactly `desired`: existing blocks are
/// matched by `device` (then by `name`) and rewritten in place (unmanaged keys
/// such as `channel_map` and comments are kept), unmatched osx blocks are
/// removed, missing ones are appended. Non-osx blocks are never touched.
pub fn apply_osx_outputs(text: &str, desired: &[OsxOutput]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let blocks = find_blocks(&lines);
    let osx_blocks: Vec<&Block> = blocks
        .iter()
        .filter(|b| {
            b.name == "audio_output" && b.entries.iter().any(|(k, v)| k == "type" && v == "osx")
        })
        .collect();

    // Match desired → existing block index.
    let get = |b: &Block, k: &str| -> Option<String> {
        b.entries
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.clone())
    };
    let mut assigned: Vec<Option<usize>> = vec![None; desired.len()];
    let mut used: Vec<bool> = vec![false; osx_blocks.len()];
    for (di, d) in desired.iter().enumerate() {
        // by device first
        if let Some(dev) = d.device.as_deref().filter(|x| !x.is_empty()) {
            if let Some((bi, _)) = osx_blocks
                .iter()
                .enumerate()
                .find(|(bi, b)| !used[*bi] && get(b, "device").as_deref() == Some(dev))
            {
                assigned[di] = Some(bi);
                used[bi] = true;
                continue;
            }
        }
        // then by name (or a device-less block for the "System default" output)
        if let Some((bi, _)) = osx_blocks.iter().enumerate().find(|(bi, b)| {
            !used[*bi]
                && (get(b, "name").as_deref() == Some(d.name.as_str())
                    || (d.device.is_none() && get(b, "device").filter(|x| !x.is_empty()).is_none()))
        }) {
            assigned[di] = Some(bi);
            used[bi] = true;
        }
    }

    // Build edits: (start, end, replacement lines). Process from the bottom so
    // indices stay valid.
    struct Edit {
        start: usize,
        end: usize,
        replacement: Vec<String>,
    }
    let mut edits: Vec<Edit> = Vec::new();
    for (bi, b) in osx_blocks.iter().enumerate() {
        if let Some(di) = assigned.iter().position(|a| *a == Some(bi)) {
            let mut kept: Vec<String> = Vec::new();
            for line in &lines[b.start + 1..b.end] {
                match split_kv(line) {
                    Some((k, _)) if OSX_MANAGED_KEYS.contains(&k) => {}
                    _ => kept.push(line.clone()),
                }
            }
            let mut new_block = vec![lines[b.start].clone()];
            new_block.extend(osx_block_body(&desired[di]));
            new_block.extend(kept);
            new_block.push(lines[b.end].clone());
            edits.push(Edit {
                start: b.start,
                end: b.end,
                replacement: new_block,
            });
        } else {
            // Remove the block plus one preceding blank line, if any.
            let start = if b.start > 0 && lines[b.start - 1].trim().is_empty() {
                b.start - 1
            } else {
                b.start
            };
            edits.push(Edit {
                start,
                end: b.end,
                replacement: Vec::new(),
            });
        }
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.start));
    for e in edits {
        lines.splice(e.start..=e.end, e.replacement);
    }
    // Append the ones that had no block.
    for (di, d) in desired.iter().enumerate() {
        if assigned[di].is_none() {
            if lines.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
                lines.push(String::new());
            }
            lines.push("audio_output {".into());
            lines.extend(osx_block_body(d));
            lines.push("}".into());
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Single-output editor entry point: makes the file's osx blocks exactly this one.
pub fn apply_osx_output(text: &str, osx: &OsxOutput) -> String {
    apply_osx_outputs(text, std::slice::from_ref(osx))
}

/// Bit-perfect friendly top-level defaults, applied together with the osx block.
pub fn bit_perfect_top_level(
    replaygain: &str,
    volume_normalization: bool,
    drop_audio_output_format: bool,
) -> Vec<(&'static str, Option<String>)> {
    let mut v = vec![
        ("replaygain", Some(replaygain.to_owned())),
        (
            "volume_normalization",
            Some(if volume_normalization { "yes" } else { "no" }.to_owned()),
        ),
    ];
    if drop_audio_output_format {
        v.push(("audio_output_format", None));
    }
    v
}

pub fn write_conf(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("conf.melo-tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

// ---------------------------------------------------------------------------
// Restart
// ---------------------------------------------------------------------------

fn brew_path() -> Option<&'static str> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .into_iter()
        .find(|p| Path::new(p).exists())
}

fn mpd_path() -> Option<&'static str> {
    ["/opt/homebrew/bin/mpd", "/usr/local/bin/mpd"]
        .into_iter()
        .find(|p| Path::new(p).exists())
}

/// Restarts the local daemon so `audio_output` changes take effect (SIGHUP
/// only reloads a subset of the config). Uses `brew services` when MPD is
/// managed by it, otherwise kills and relaunches with the given conf.
pub fn restart(conf_path: &Path) -> Result<String, String> {
    if let Some(brew) = brew_path() {
        let managed = Command::new(brew)
            .args(["services", "list"])
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.starts_with("mpd") && l.contains("started"))
            })
            .unwrap_or(false);
        if managed {
            let out = Command::new(brew)
                .args(["services", "restart", "mpd"])
                .output()
                .map_err(|e| e.to_string())?;
            return if out.status.success() {
                Ok("Restarted via brew services".into())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
            };
        }
    }
    let mpd = mpd_path().ok_or_else(|| "mpd binary not found (brew install mpd)".to_string())?;
    // SIGTERM, then SIGKILL if it hangs (MPD can wedge inside CoreAudio when
    // an output failed to open and never gets to its signal handler).
    let _ = Command::new("/usr/bin/pkill").args(["-x", "mpd"]).output();
    if !wait_until_stopped(3000) {
        let _ = Command::new("/usr/bin/pkill")
            .args(["-9", "-x", "mpd"])
            .output();
        if !wait_until_stopped(2000) {
            return Err("Could not stop the running mpd (even with SIGKILL)".into());
        }
    }
    let out = Command::new(mpd)
        .arg(conf_path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    // mpd daemonises; make sure it actually came up.
    for _ in 0..30 {
        if is_running() {
            return Ok("Restarted mpd".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("mpd exited right after starting — check its log".into())
}

fn wait_until_stopped(max_ms: u64) -> bool {
    for _ in 0..(max_ms / 100) {
        if !is_running() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    !is_running()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = r#"music_directory		"~/Desktop/media/Music"
# comment
bind_to_address		"0.0.0.0"
port		"6600"

audio_output {
    type "osx"
    name "CoreAudio"
    channel_map "0,1"
}

audio_output {
    type "httpd"
    name "Stream"
    port "8000"
}

auto_update "yes"
volume_normalization		"yes"
replaygain		"off"
"#;

    #[test]
    fn parses_top_level_and_blocks() {
        let c = parse_conf(CONF);
        assert_eq!(c.port, "6600");
        assert_eq!(c.bind_to_address, "0.0.0.0");
        assert!(c.volume_normalization);
        assert_eq!(c.replaygain, "off");
        assert!(c.audio_output_format.is_none());
        let osx = c.osx().unwrap();
        assert_eq!(osx.name, "CoreAudio");
        assert_eq!(osx.device, None);
        assert!(!osx.hog_device);
        assert_eq!(
            c.other_outputs,
            vec![("httpd".to_string(), "Stream".to_string())]
        );
    }

    #[test]
    fn top_level_updates_preserve_structure() {
        let out = apply_top_level(
            CONF,
            &[
                ("volume_normalization", Some("no".into())),
                ("audio_output_format", None),
                ("replaygain", Some("off".into())),
            ],
        );
        assert!(out.contains("volume_normalization\t\t\"no\""));
        assert!(out.contains("# comment"));
        assert!(out.contains("    port \"8000\""));
        assert!(!out.contains("audio_output_format"));
    }

    fn osx(name: &str, device: Option<&str>) -> OsxOutput {
        OsxOutput {
            name: name.into(),
            device: device.map(str::to_owned),
            hog_device: device.is_some(),
            dop: false,
            mixer_type: Some("none".into()),
            format: None,
        }
    }

    #[test]
    fn multi_output_rewrite_matches_removes_and_appends() {
        // Legacy single "CoreAudio" block (device-less) → becomes the System default row;
        // Scarlett and XM6 (with format) are appended; httpd untouched.
        let mut xm6 = osx("WH-1000XM6", Some("WH-1000XM6"));
        xm6.hog_device = false;
        xm6.format = Some("44100:24:2".into());
        let desired = vec![
            osx("System default", None),
            osx("Scarlett 2i4 USB", Some("Scarlett 2i4 USB")),
            xm6,
        ];
        let out = apply_osx_outputs(CONF, &desired);
        let c = parse_conf(&out);
        assert_eq!(c.osx_outputs.len(), 3);
        assert_eq!(c.osx_outputs[0].name, "System default");
        assert!(
            out.contains("channel_map \"0,1\""),
            "unmanaged key kept in the rewritten block"
        );
        assert!(out.contains("name \"Stream\""), "httpd block kept");
        let sc = c.osx_for_output("Scarlett 2i4 USB").unwrap();
        assert!(sc.hog_device);
        assert_eq!(sc.format, None);
        let bt = c.osx_for_output("WH-1000XM6").unwrap();
        assert!(!bt.hog_device);
        assert_eq!(bt.format.as_deref(), Some("44100:24:2"));

        // Now drop the XM6 and the default: only Scarlett remains (matched by device).
        let out2 = apply_osx_outputs(&out, &[osx("Scarlett 2i4 USB", Some("Scarlett 2i4 USB"))]);
        let c2 = parse_conf(&out2);
        assert_eq!(c2.osx_outputs.len(), 1);
        assert_eq!(
            c2.osx_outputs[0].device.as_deref(),
            Some("Scarlett 2i4 USB")
        );
        assert!(out2.contains("name \"Stream\""));
        assert_eq!(out2.matches("audio_output {").count(), 2);
    }

    #[test]
    fn legacy_named_block_is_migrated_by_device() {
        let legacy = "port \"6600\"\n\naudio_output {\n    type \"osx\"\n    name \"CoreAudio\"\n    device \"Scarlett 2i4 USB\"\n    hog_device \"yes\"\n}\n";
        let out = apply_osx_outputs(legacy, &[osx("Scarlett 2i4 USB", Some("Scarlett 2i4 USB"))]);
        let c = parse_conf(&out);
        assert_eq!(c.osx_outputs.len(), 1);
        assert_eq!(c.osx_outputs[0].name, "Scarlett 2i4 USB");
        assert_eq!(out.matches("audio_output {").count(), 1);
    }

    #[test]
    fn empty_desired_removes_all_osx_blocks() {
        let out = apply_osx_outputs(CONF, &[]);
        let c = parse_conf(&out);
        assert!(c.osx_outputs.is_empty());
        assert_eq!(c.other_outputs.len(), 1);
    }

    #[test]
    fn local_host_detection() {
        assert!(is_local_host("127.0.0.1"));
        assert!(is_local_host("localhost"));
        assert!(!is_local_host("bee.local"));
    }
}
