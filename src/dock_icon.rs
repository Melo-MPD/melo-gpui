//! App icon handling: the 1024 px master is a flat square, so we round its
//! corners the way macOS renders app icons (≈22.4 % radius) once at startup,
//! use that for the Dock (so `cargo run` shows it), the About window and the
//! `.icns` produced by `scripts/bundle.sh`.

use std::sync::OnceLock;

pub const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icon/AppIcon-1024.png");

/// PNG bytes of the master icon with macOS-style rounded corners.
pub fn rounded_icon_png() -> &'static [u8] {
    static ROUNDED: OnceLock<Vec<u8>> = OnceLock::new();
    ROUNDED.get_or_init(|| round_corners(APP_ICON_PNG).unwrap_or_else(|| APP_ICON_PNG.to_vec()))
}

fn round_corners(png: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let size = w.min(h) as f32;
    // Apple's app-icon superellipse is close to a rounded rect with r ≈ 0.2237 · size.
    let r = size * 0.2237;
    let mut out = img.clone();
    for (x, y, p) in out.enumerate_pixels_mut() {
        let fx = x as f32 + 0.5;
        let fy = y as f32 + 0.5;
        // distance from the nearest corner centre, only inside corner squares
        let cx = if fx < r {
            r
        } else if fx > w as f32 - r {
            w as f32 - r
        } else {
            fx
        };
        let cy = if fy < r {
            r
        } else if fy > h as f32 - r {
            h as f32 - r
        } else {
            fy
        };
        let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        if d > r {
            // 1 px anti-aliased edge
            let a = (r + 1.0 - d).clamp(0.0, 1.0);
            p.0[3] = (p.0[3] as f32 * a) as u8;
        }
    }
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(out)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .ok()?;
    Some(buf)
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
pub fn install() {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    unsafe {
        let bytes = rounded_icon_png();
        let data: *mut Object =
            msg_send![class!(NSData), dataWithBytes: bytes.as_ptr() length: bytes.len()];
        if data.is_null() {
            return;
        }
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        let image: *mut Object = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage: image];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}

/// `melo --write-icon <path>`: dumps the rounded PNG (used by scripts/bundle.sh).
pub fn maybe_handle_cli() -> bool {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--write-icon") {
        if let Some(path) = args.next() {
            if let Err(e) = std::fs::write(&path, rounded_icon_png()) {
                eprintln!("failed to write icon: {e}");
            }
        }
        return true;
    }
    false
}
