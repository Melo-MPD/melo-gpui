//! Saved server profiles as JSON under
//! `~/Library/Application Support/Melo/profiles.json`.
//!
//! Prototype simplification vs the Swift app: passwords live in the same
//! JSON file rather than the Keychain.

use crate::mpd::ServerProfile;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default)]
struct StoreFile {
    #[serde(default)]
    profiles: Vec<ServerProfile>,
    #[serde(default)]
    last_profile: Option<ServerProfile>,
    #[serde(default)]
    bit_perfect_mode: bool,
}

fn store_path() -> Option<PathBuf> {
    let dir = super::app_dirs::data_dir()?;
    Some(dir.join("profiles.json"))
}

pub struct Persisted {
    pub profiles: Vec<ServerProfile>,
    pub last_profile: Option<ServerProfile>,
    pub bit_perfect_mode: bool,
}

pub fn load() -> Persisted {
    let file = store_path()
        .and_then(|p| fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<StoreFile>(&bytes).ok())
        .unwrap_or_default();
    Persisted {
        profiles: file.profiles,
        last_profile: file.last_profile,
        bit_perfect_mode: file.bit_perfect_mode,
    }
}

pub fn save(
    profiles: &[ServerProfile],
    last_profile: Option<&ServerProfile>,
    bit_perfect_mode: bool,
) {
    let Some(path) = store_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let file = StoreFile {
        profiles: profiles.iter().filter(|p| p.is_manual).cloned().collect(),
        last_profile: last_profile.cloned(),
        bit_perfect_mode,
    };
    match serde_json::to_vec_pretty(&file) {
        Ok(bytes) => {
            if let Err(e) = fs::write(&path, bytes) {
                eprintln!("[persistence] write failed: {e}");
            }
        }
        Err(e) => eprintln!("[persistence] encode failed: {e}"),
    }
}
