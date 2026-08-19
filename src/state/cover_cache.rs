//! Memory + disk cache for cover art keyed by song URI, plus decoding helpers.
//! Disk layout: `~/Library/Caches/Melo/coverArt/<fnv-hash-of-uri>`.

use gpui::{Image, ImageFormat};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

pub struct CoverCache {
    memory: HashMap<String, Arc<Image>>,
    /// URIs known to have no art, so we don't re-request them.
    misses: std::collections::HashSet<String>,
    dir: Option<PathBuf>,
}

impl CoverCache {
    pub fn new() -> Self {
        let dir = super::app_dirs::cache_dir().map(|d| d.join("coverArt"));
        if let Some(d) = &dir {
            let _ = fs::create_dir_all(d);
        }
        CoverCache {
            memory: HashMap::new(),
            misses: Default::default(),
            dir,
        }
    }

    pub fn get(&mut self, uri: &str) -> Option<Arc<Image>> {
        if let Some(img) = self.memory.get(uri) {
            return Some(img.clone());
        }
        let path = self.path_for(uri)?;
        let bytes = fs::read(path).ok()?;
        let img = decode(bytes)?;
        self.memory.insert(uri.to_owned(), img.clone());
        Some(img)
    }

    pub fn is_known_miss(&self, uri: &str) -> bool {
        self.misses.contains(uri)
    }

    pub fn note_miss(&mut self, uri: &str) {
        self.misses.insert(uri.to_owned());
    }

    /// Stores raw bytes; returns the decoded image (None when undecodable).
    pub fn store(&mut self, uri: &str, bytes: Vec<u8>) -> Option<Arc<Image>> {
        if let Some(path) = self.path_for(uri) {
            let _ = fs::write(path, &bytes);
        }
        let img = decode(bytes)?;
        self.memory.insert(uri.to_owned(), img.clone());
        Some(img)
    }

    fn path_for(&self, uri: &str) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(format!("{:016x}", fnv1a(uri))))
    }
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Wraps encoded bytes as a GPUI image (decoding is done lazily by GPUI's
/// asset system on a background thread).
pub fn decode(bytes: Vec<u8>) -> Option<Arc<Image>> {
    let format = match image::guess_format(&bytes).ok()? {
        image::ImageFormat::Png => ImageFormat::Png,
        image::ImageFormat::Jpeg => ImageFormat::Jpeg,
        image::ImageFormat::WebP => ImageFormat::Webp,
        image::ImageFormat::Gif => ImageFormat::Gif,
        image::ImageFormat::Bmp => ImageFormat::Bmp,
        image::ImageFormat::Tiff => ImageFormat::Tiff,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

/// Produces the heavily blurred, dimmed backdrop used behind Now Playing.
/// CPU work — call from a background thread.
pub fn blurred_backdrop(bytes: &[u8]) -> Option<Arc<Image>> {
    let img = image::load_from_memory(bytes).ok()?;
    let small = img
        .resize_exact(96, 96, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let blurred = image::imageops::fast_blur(&small, 12.0);
    let mut out = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut out);
    image::DynamicImage::ImageRgba8(blurred)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .ok()?;
    Some(Arc::new(Image::from_bytes(ImageFormat::Png, out)))
}
