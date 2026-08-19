//! Embedded SVG icons (Lucide, ISC licence) served through GPUI's AssetSource.

use anyhow::Result;
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $( (concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg"))), )*
        ];
    };
}

icons!(
    "play-circle",
    "pause-circle",
    "skip-back",
    "skip-forward",
    "play",
    "pause",
    "shuffle",
    "repeat",
    "repeat-1",
    "volume",
    "volume-2",
    "music",
    "list-music",
    "library",
    "settings",
    "audio-lines",
    "trash-2",
    "plus",
    "minus",
    "refresh-cw",
    "x",
    "search",
    "check",
    "server",
    "wifi",
    "chevron-down",
    "chevron-right",
    "chevron-left",
    "speaker",
    "lock",
    "circle-check",
    "triangle-alert",
    "info",
    "cable",
    "radio-on",
    "radio-off",
    "activity",
    "hard-drive",
    "melo-logo",
);

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
