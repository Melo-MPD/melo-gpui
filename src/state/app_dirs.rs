//! Per-user directories: `~/Library/Application Support/Melo` and
//! `~/Library/Caches/Melo`. The prototype used `Melo-GPUI`; the first launch
//! after the rename moves that directory over so saved profiles and the cover
//! cache survive. (Migration shim — safe to delete a few releases after 0.1.)

use std::fs;
use std::path::PathBuf;

const APP_DIR: &str = "Melo";
const LEGACY_APP_DIR: &str = "Melo-GPUI";

fn migrated(base: PathBuf) -> PathBuf {
    let new = base.join(APP_DIR);
    let old = base.join(LEGACY_APP_DIR);
    if !new.exists() && old.is_dir() {
        let _ = fs::rename(&old, &new);
    }
    new
}

/// `~/Library/Application Support/Melo`
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(migrated)
}

/// `~/Library/Caches/Melo`
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(migrated)
}
