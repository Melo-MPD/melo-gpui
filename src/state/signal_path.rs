//! Signal Path: what happens to the bits between the file and the DAC, as far
//! as the MPD protocol (and, for the local Mac daemon, `mpd.conf`) can tell.

use crate::mpd::{Output, PlayState, Song, Status};
use crate::state::coreaudio::CaDevice;
use crate::state::local_mpd::{LocalConf, OsxOutput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// No DSP, no attenuation, exclusive path known or unverifiable-but-clean.
    BitPerfect,
    /// Only volume attenuation in the way.
    Lossless,
    /// ReplayGain / crossfade / MixRamp / normalization / resampling active.
    Altered,
    /// Stopped or no data.
    Unknown,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::BitPerfect => "Bit-perfect",
            Verdict::Lossless => "Lossless",
            Verdict::Altered => "Altered",
            Verdict::Unknown => "Idle",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FixAction {
    ReplayGainOff,
    CrossfadeOff,
    MixRampOff,
    VolumeFull,
    EnableOutput(u32),
    /// `clearerror`
    ClearError,
    /// disableoutput + enableoutput (re-acquire a re-enumerated DAC)
    CycleOutput(u32),
    /// Requires editing mpd.conf → open Settings › Local MPD.
    OpenLocalMpd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub label: String,
    pub action: FixAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage {
    pub label: String,
    /// true when this stage changes the samples
    pub altering: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalPath {
    pub verdict: Verdict,
    /// Short text for the transport-bar pill, e.g. "24-bit · 96 kHz" or "DSD64 · DoP"
    pub pill: String,
    pub source: Option<String>,
    pub decoder: Option<String>,
    pub processing: Vec<Stage>,
    pub output: Option<String>,
    pub reasons: Vec<String>,
    pub unknowns: Vec<String>,
    pub fixes: Vec<Fix>,
}

fn khz(rate: u32) -> String {
    let k = rate as f64 / 1000.0;
    if k.fract() == 0.0 {
        format!("{} kHz", k as u32)
    } else {
        format!("{k:.1} kHz")
    }
}

fn file_kind(uri: &str) -> Option<String> {
    let ext = uri.rsplit('.').next()?.to_ascii_uppercase();
    match ext.as_str() {
        "FLAC" | "ALAC" | "WAV" | "AIFF" | "AIF" | "APE" | "WV" | "DSF" | "DFF" | "MP3" | "AAC"
        | "M4A" | "OGG" | "OPUS" | "WMA" | "MPC" | "TTA" => {
            Some(if ext == "M4A" { "AAC/ALAC".into() } else { ext })
        }
        _ => None,
    }
}

fn describe_output(i: &OutputInfo, dop: &mut bool) -> String {
    let o = &i.output;
    let mut d = format!("\"{}\" · {}", o.name, o.plugin_label());
    if o.plugin == "osx" {
        if let Some(x) = i.osx.as_ref() {
            if x.device.is_none() {
                d.push_str(" · system default");
            }
            if x.dop {
                *dop = true;
            }
            match i.device.as_ref() {
                Some(dev) => {
                    if x.device.is_none() {
                        d.push_str(&format!(" ({})", dev.name));
                    }
                    if !dev.transport.is_empty() {
                        d.push_str(" · ");
                        d.push_str(&dev.transport);
                    }
                    if let Some(r) = dev.sample_rate {
                        d.push_str(&format!(" · device at {}", khz(r)));
                    }
                    if dev.hog_pid.is_some() {
                        d.push_str(" · exclusive ✓");
                    } else if x.hog_device {
                        d.push_str(" · exclusive requested");
                    }
                }
                None => {
                    if let Some(name) = x.device.as_deref() {
                        d.push_str(&format!(" · {name} (not connected)"));
                    }
                    if x.hog_device {
                        d.push_str(" · exclusive");
                    }
                }
            }
        }
    }
    if o.plugin == "alsa" {
        match o.attribute("dop") {
            Some("1") => {
                d.push_str(" · DoP on");
                *dop = true;
            }
            Some(_) => d.push_str(" · DoP off"),
            None => {}
        }
        if let Some(f) = o.attribute("allowed_formats").filter(|f| !f.is_empty()) {
            d.push_str(&format!(" · formats {f}"));
        }
    }
    d
}

/// An MPD output with, when it is a local `osx` output, its conf block and the
/// live CoreAudio device behind it.
#[derive(Clone, Debug)]
pub struct OutputInfo {
    pub output: Output,
    pub osx: Option<OsxOutput>,
    pub device: Option<CaDevice>,
}

impl OutputInfo {
    #[cfg(test)]
    pub fn plain(output: Output) -> Self {
        OutputInfo {
            output,
            osx: None,
            device: None,
        }
    }
}

pub fn compute(
    status: &Status,
    song: Option<&Song>,
    outputs: &[OutputInfo],
    replay_gain: &str,
    local: Option<&LocalConf>,
) -> SignalPath {
    // The osx block / device of the (first) enabled output drives mixer and
    // resampling checks.
    let primary = outputs.iter().find(|i| i.output.enabled);
    let primary_osx = primary.and_then(|i| i.osx.as_ref());
    let mut reasons = Vec::new();
    let mut fixes = Vec::new();
    let mut unknowns = Vec::new();
    let mut processing = Vec::new();
    let mut attenuated = false;

    // ---- Source / decoder --------------------------------------------------
    let source = song.map(|s| {
        let mut parts: Vec<String> = Vec::new();
        if let Some(k) = file_kind(&s.uri) {
            parts.push(k);
        }
        if let Some(f) = &s.format {
            parts.push(f.display_string());
        }
        if let Some(b) = status.bitrate.filter(|b| *b > 0) {
            parts.push(format!("{b} kbps"));
        }
        if parts.is_empty() {
            "Unknown format".into()
        } else {
            parts.join(" · ")
        }
    });
    let decoder = status.audio_format.as_ref().map(|f| f.display_string());
    let is_dsd = status
        .audio_format
        .as_ref()
        .map(|f| f.bits.starts_with("dsd"))
        .unwrap_or(false);

    // ---- Processing --------------------------------------------------------
    if replay_gain != "off" {
        processing.push(Stage {
            label: format!("ReplayGain {replay_gain}"),
            altering: true,
        });
        reasons.push(format!("ReplayGain is set to {replay_gain}"));
        fixes.push(Fix {
            label: "Turn off ReplayGain".into(),
            action: FixAction::ReplayGainOff,
        });
    } else {
        processing.push(Stage {
            label: "ReplayGain off".into(),
            altering: false,
        });
    }
    if status.crossfade > 0 {
        processing.push(Stage {
            label: format!("Crossfade {} s", status.crossfade),
            altering: true,
        });
        reasons.push(format!("Crossfade of {} s mixes tracks", status.crossfade));
        fixes.push(Fix {
            label: "Set crossfade to 0".into(),
            action: FixAction::CrossfadeOff,
        });
    } else {
        processing.push(Stage {
            label: "Crossfade off".into(),
            altering: false,
        });
    }
    if let Some(delay) = status.mixrampdelay {
        processing.push(Stage {
            label: format!("MixRamp {} dB / {delay} s", status.mixrampdb),
            altering: true,
        });
        reasons.push("MixRamp overlaps track boundaries".into());
        fixes.push(Fix {
            label: "Disable MixRamp".into(),
            action: FixAction::MixRampOff,
        });
    } else {
        processing.push(Stage {
            label: "MixRamp off".into(),
            altering: false,
        });
    }
    let mixer_type = primary_osx.and_then(|o| o.mixer_type.clone());
    if status.volume < 0 {
        processing.push(Stage {
            label: "Volume fixed (no mixer)".into(),
            altering: false,
        });
    } else if status.volume >= 100 {
        processing.push(Stage {
            label: "Volume 100 %".into(),
            altering: false,
        });
    } else if mixer_type.as_deref() == Some("hardware") {
        processing.push(Stage {
            label: format!("Device volume {} % (hardware)", status.volume),
            altering: false,
        });
    } else {
        processing.push(Stage {
            label: format!("Volume {} % (attenuated)", status.volume),
            altering: false,
        });
        attenuated = true;
        fixes.push(Fix {
            label: "Set volume to 100 %".into(),
            action: FixAction::VolumeFull,
        });
    }
    if let Some(l) = local {
        if l.volume_normalization {
            processing.push(Stage {
                label: "Volume normalization".into(),
                altering: true,
            });
            reasons.push("volume_normalization is enabled in mpd.conf".into());
            fixes.push(Fix {
                label: "Fix in Local MPD settings".into(),
                action: FixAction::OpenLocalMpd,
            });
        }
        if let Some(f) = primary_osx.and_then(|o| o.format.clone()) {
            processing.push(Stage {
                label: format!("Output format {f}"),
                altering: true,
            });
            let lossy_dev = primary
                .and_then(|i| i.device.as_ref())
                .map(|d| d.is_lossy_transport())
                .unwrap_or(false);
            if lossy_dev {
                reasons.push(format!(
                    "MPD converts to {f} for the Bluetooth/AirPlay output"
                ));
            } else {
                reasons.push(format!(
                    "Output `format \"{f}\"` forces conversion — remove it for a wired DAC"
                ));
                if !fixes.iter().any(|f| f.action == FixAction::OpenLocalMpd) {
                    fixes.push(Fix {
                        label: "Fix in Local MPD settings".into(),
                        action: FixAction::OpenLocalMpd,
                    });
                }
            }
        }
        if let Some(f) = &l.audio_output_format {
            processing.push(Stage {
                label: format!("Resampling to {f}"),
                altering: true,
            });
            reasons.push(format!("audio_output_format \"{f}\" forces conversion"));
            if !fixes.iter().any(|f| f.action == FixAction::OpenLocalMpd) {
                fixes.push(Fix {
                    label: "Fix in Local MPD settings".into(),
                    action: FixAction::OpenLocalMpd,
                });
            }
        }
        if mixer_type.as_deref() == Some("software") && status.volume >= 0 && status.volume < 100 {
            reasons.push("Software mixer scales samples".into());
        }
    } else {
        unknowns.push("Output resampling and mixer type live in the server's mpd.conf".into());
    }

    // ---- Output --------------------------------------------------------------
    let enabled: Vec<&OutputInfo> = outputs.iter().filter(|i| i.output.enabled).collect();
    let mut dop = false;
    let output = if outputs.is_empty() {
        None
    } else if enabled.is_empty() {
        reasons.push("No output is enabled".into());
        if let Some(first) = outputs.first() {
            fixes.push(Fix {
                label: format!("Enable {}", first.output.name),
                action: FixAction::EnableOutput(first.output.id),
            });
        }
        Some("No output enabled".into())
    } else {
        let descs: Vec<String> = enabled
            .iter()
            .map(|i| describe_output(i, &mut dop))
            .collect();
        Some(descs.join("  +  "))
    };

    // ---- Live device (CoreAudio) checks, per enabled local output ------------
    for i in &enabled {
        let Some(dev) = i.device.as_ref() else {
            continue;
        };
        if dev.is_lossy_transport() {
            reasons.push(format!(
                "{} is a {} device — audio is re-encoded (AAC/SBC), never bit-perfect",
                dev.name, dev.transport
            ));
        }
        if let (Some(dev_rate), Some(fmt)) = (dev.sample_rate, status.audio_format.as_ref()) {
            let src_rate = fmt.sample_rate;
            if !fmt.bits.starts_with("dsd") && dev_rate != src_rate && src_rate > 0 {
                match dev.max_sample_rate {
                    Some(max) if src_rate > max => reasons.push(format!(
                        "CoreAudio downsamples {} → {} — {} tops out at {}",
                        khz(src_rate),
                        khz(dev_rate),
                        dev.name,
                        khz(max)
                    )),
                    _ => reasons.push(format!(
                        "CoreAudio is resampling {} → {} (the device is running at {})",
                        khz(src_rate),
                        khz(dev_rate),
                        khz(dev_rate)
                    )),
                }
            }
        }
        if let Some(x) = i.osx.as_ref() {
            if x.hog_device
                && dev.hog_pid.is_none()
                && status.state == PlayState::Play
                && !dev.is_lossy_transport()
            {
                unknowns.push(format!(
                    "Exclusive mode requested for {} but the device is not hogged right now",
                    dev.name
                ));
            }
            if !x.hog_device && !dev.is_lossy_transport() {
                unknowns.push(format!(
                    "{}: exclusive (hog) mode is off — other apps can mix into it",
                    dev.name
                ));
            }
        }
    }
    if let Some(err) = status.error_message.as_deref().filter(|e| !e.is_empty()) {
        let hog_stale = err.contains("560947818") || err.contains("!hog");
        let local_out = enabled.iter().find(|i| i.osx.is_some());
        let default_with_hog = local_out
            .and_then(|i| i.osx.as_ref())
            .map(|x| x.device.is_none() && x.hog_device)
            .unwrap_or(false);
        if hog_stale && default_with_hog {
            reasons.push(format!(
                "MPD error: {err} — exclusive mode on the system-default output can't work (macOS mixes it); pick a specific device or turn Exclusive off"
            ));
            fixes.push(Fix {
                label: "Fix in Local MPD settings".into(),
                action: FixAction::OpenLocalMpd,
            });
        } else if hog_stale {
            reasons.push(format!(
                "MPD error: {err} — the device is held exclusively (re-enumerated after sleep/re-plug, or another app)"
            ));
        } else {
            reasons.push(format!("MPD error: {err}"));
        }
        if let Some(local_out) = local_out {
            if (hog_stale && !default_with_hog) || err.to_lowercase().contains("failed to open") {
                fixes.push(Fix {
                    label: format!("Re-open {}", local_out.output.name),
                    action: FixAction::CycleOutput(local_out.output.id),
                });
            }
        }
        fixes.push(Fix {
            label: "Clear MPD error".into(),
            action: FixAction::ClearError,
        });
        if err.to_lowercase().contains("open audio output") {
            fixes.push(Fix {
                label: "Check output in Local MPD settings".into(),
                action: FixAction::OpenLocalMpd,
            });
        }
    }

    // ---- Verdict -------------------------------------------------------------
    let playing =
        matches!(status.state, PlayState::Play | PlayState::Pause) && status.audio_format.is_some();
    let verdict = if !playing {
        Verdict::Unknown
    } else if !reasons.is_empty() {
        Verdict::Altered
    } else if attenuated {
        Verdict::Lossless
    } else {
        Verdict::BitPerfect
    };

    let pill = match &status.audio_format {
        Some(f) if is_dsd => {
            if dop {
                format!("{} · DoP", f.bits_label())
            } else {
                f.bits_label()
            }
        }
        Some(f) => format!("{} · {}", f.bits_label(), f.sample_rate_label()),
        None => song
            .and_then(|s| s.format.as_ref())
            .map(|f| format!("{} · {}", f.bits_label(), f.sample_rate_label()))
            .unwrap_or_else(|| "—".into()),
    };

    SignalPath {
        verdict,
        pill,
        source,
        decoder,
        processing,
        output,
        reasons,
        unknowns,
        fixes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpd::AudioFormat;

    fn status(volume: i32) -> Status {
        Status {
            state: PlayState::Play,
            volume,
            audio_format: Some(AudioFormat {
                sample_rate: 96000,
                bits: "24".into(),
                channels: 2,
            }),
            bitrate: Some(2304),
            ..Default::default()
        }
    }
    fn out(plugin: &str, enabled: bool) -> OutputInfo {
        OutputInfo::plain(Output {
            id: 0,
            name: "Out".into(),
            plugin: plugin.into(),
            enabled,
            attributes: vec![],
        })
    }
    fn dev(name: &str, transport: &str, rate: u32, max: u32, hog: bool) -> CaDevice {
        CaDevice {
            id: 1,
            name: name.into(),
            transport: transport.into(),
            sample_rate: Some(rate),
            max_sample_rate: Some(max),
            hog_pid: if hog { Some(1) } else { None },
            is_default: false,
        }
    }
    fn osx_out(device: &str, hog: bool, mixer: &str, dev: Option<CaDevice>) -> OutputInfo {
        OutputInfo {
            output: Output {
                id: 0,
                name: device.into(),
                plugin: "osx".into(),
                enabled: true,
                attributes: vec![],
            },
            osx: Some(OsxOutput {
                name: device.into(),
                device: Some(device.into()),
                hog_device: hog,
                dop: false,
                mixer_type: Some(mixer.into()),
                format: None,
            }),
            device: dev,
        }
    }

    #[test]
    fn clean_remote_is_bit_perfect_with_unknowns() {
        let sp = compute(&status(-1), None, &[out("alsa", true)], "off", None);
        assert_eq!(sp.verdict, Verdict::BitPerfect);
        assert_eq!(sp.pill, "24-bit · 96 kHz");
        assert!(!sp.unknowns.is_empty());
    }

    #[test]
    fn attenuation_is_lossless_and_dsp_is_altered() {
        let sp = compute(&status(60), None, &[out("alsa", true)], "off", None);
        assert_eq!(sp.verdict, Verdict::Lossless);
        assert!(sp.fixes.iter().any(|f| f.action == FixAction::VolumeFull));
        let sp = compute(&status(100), None, &[out("alsa", true)], "album", None);
        assert_eq!(sp.verdict, Verdict::Altered);
        assert!(
            sp.fixes
                .iter()
                .any(|f| f.action == FixAction::ReplayGainOff)
        );
    }

    #[test]
    fn local_normalization_and_hardware_volume() {
        let local = LocalConf {
            volume_normalization: true,
            ..Default::default()
        };
        let scarlett = dev("Scarlett 2i4 USB", "USB", 96000, 96000, true);
        let sp = compute(
            &status(43),
            None,
            &[osx_out(
                "Scarlett 2i4 USB",
                true,
                "hardware",
                Some(scarlett.clone()),
            )],
            "off",
            Some(&local),
        );
        assert_eq!(sp.verdict, Verdict::Altered);
        assert!(sp.fixes.iter().any(|f| f.action == FixAction::OpenLocalMpd));
        assert!(
            sp.output
                .as_deref()
                .unwrap()
                .contains("USB · device at 96 kHz · exclusive ✓")
        );
        let clean = LocalConf::default();
        let sp = compute(
            &status(43),
            None,
            &[osx_out(
                "Scarlett 2i4 USB",
                true,
                "hardware",
                Some(scarlett),
            )],
            "off",
            Some(&clean),
        );
        assert_eq!(
            sp.verdict,
            Verdict::BitPerfect,
            "hardware volume does not touch samples"
        );
    }

    #[test]
    fn bluetooth_and_rate_limits_are_altered() {
        let xm6 = dev("WH-1000XM6", "Bluetooth", 44100, 48000, false);
        let local = LocalConf::default();
        let sp = compute(
            &status(-1),
            None,
            &[osx_out("WH-1000XM6", false, "none", Some(xm6))],
            "off",
            Some(&local),
        );
        assert_eq!(sp.verdict, Verdict::Altered);
        assert!(sp.reasons.iter().any(|r| r.contains("Bluetooth")));
        assert!(
            sp.reasons
                .iter()
                .any(|r| r.contains("downsamples 96 kHz → 44.1 kHz"))
        );

        // 96 kHz source on a 96 kHz-max DAC running at 96 → bit-perfect
        let sc = dev("Scarlett 2i4 USB", "USB", 96000, 96000, true);
        let sp = compute(
            &status(-1),
            None,
            &[osx_out("Scarlett 2i4 USB", true, "none", Some(sc.clone()))],
            "off",
            Some(&local),
        );
        assert_eq!(sp.verdict, Verdict::BitPerfect);

        // 192 kHz source on that DAC → downsampled, says why
        let mut st = status(-1);
        st.audio_format = Some(AudioFormat {
            sample_rate: 192000,
            bits: "24".into(),
            channels: 2,
        });
        let sp = compute(
            &st,
            None,
            &[osx_out("Scarlett 2i4 USB", true, "none", Some(sc))],
            "off",
            Some(&local),
        );
        assert_eq!(sp.verdict, Verdict::Altered);
        assert!(sp.reasons.iter().any(|r| r.contains("tops out at 96 kHz")));
    }

    #[test]
    fn stale_hog_error_offers_reopen() {
        let mut st = status(-1);
        st.error_message = Some(
            "Failed to open \"Scarlett 2i4 USB\" (osx); The operation couldn’t be completed. (OSStatus error 560947818.)".into(),
        );
        let sc = dev("Scarlett 2i4 USB", "USB", 96000, 96000, false);
        let sp = compute(
            &st,
            None,
            &[osx_out("Scarlett 2i4 USB", true, "none", Some(sc))],
            "off",
            Some(&LocalConf::default()),
        );
        assert_eq!(sp.verdict, Verdict::Altered);
        assert!(sp.reasons.iter().any(|r| r.contains("held exclusively")));
        assert!(
            sp.fixes
                .iter()
                .any(|f| matches!(f.action, FixAction::CycleOutput(0)))
        );
    }

    #[test]
    fn stopped_is_unknown_and_no_output_is_flagged() {
        let mut st = status(100);
        st.state = PlayState::Stop;
        st.audio_format = None;
        assert_eq!(
            compute(&st, None, &[], "off", None).verdict,
            Verdict::Unknown
        );
        let sp = compute(&status(100), None, &[out("alsa", false)], "off", None);
        assert_eq!(sp.verdict, Verdict::Altered);
        assert!(
            sp.fixes
                .iter()
                .any(|f| matches!(f.action, FixAction::EnableOutput(0)))
        );
    }
}
