//! Instant full-screen screenshots: an X11 grab of the root window on the UI thread, then
//! PNG encoding + clipboard on a short-lived worker thread. (A native Wayland desktop only
//! lets its own screenshot tool do this; Winklet runs on XWayland there, which sees only
//! X apps, so the grab can come back black.)

use std::path::PathBuf;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};

pub struct Shot {
    pub width: u32,
    pub height: u32,
    /// top-down BGRA
    pub bgra: Vec<u8>,
}

/// Grabs every monitor (the whole root window).
pub fn grab() -> Option<Shot> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let s = &conn.setup().roots[screen];
    let (w, h) = (s.width_in_pixels, s.height_in_pixels);
    // 24/32-bit TrueColor: ZPixmap rows are already BGRX, 4 bytes a pixel
    if s.root_depth < 24 {
        return None;
    }
    let img = conn.get_image(ImageFormat::Z_PIXMAP, s.root, 0, 0, w, h, !0).ok()?.reply().ok()?;
    let need = w as usize * h as usize * 4;
    (img.data.len() >= need).then(|| Shot { width: w as u32, height: h as u32, bgra: img.data[..need].to_vec() })
}

/// Saves to Pictures/Screenshots and copies the image to the clipboard.
pub fn save_and_copy(shot: Shot) -> Option<PathBuf> {
    let mut rgba = shot.bgra;
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    crate::clip::set_image(shot.width, shot.height, &rgba);
    let pictures = std::process::Command::new("xdg-user-dir")
        .arg("PICTURES")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Pictures")))?;
    let dir = pictures.join("Screenshots");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("Winklet {}.png", chrono::Local::now().format("%Y-%m-%d %H-%M-%S")));
    let file = std::io::BufWriter::new(std::fs::File::create(&path).ok()?);
    let mut enc = png::Encoder::new(file, shot.width, shot.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().ok()?.write_image_data(&rgba).ok()?;
    Some(path)
}
