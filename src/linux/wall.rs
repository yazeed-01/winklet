//! Desktop wallpaper: read and set it on GNOME (and Cinnamon, Budgie: gsettings) and KDE
//! Plasma, and thumbnails of pictures decoded here.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PICTURES: [&str; 6] = ["jpg", "jpeg", "png", "bmp", "webp", "jfif"];

/// GNOME's picture-options, in the order of the switch on the Wallpaper tab:
/// Fill, Fit, Stretch, Centre, Span.
const POSITIONS: [&str; 5] = ["zoom", "scaled", "stretched", "centered", "spanned"];

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn kde() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_uppercase().contains("KDE"))
}

/// gsettings strings come back quoted: 'file:///usr/share/…'
fn unquote(s: &str) -> String {
    s.trim().trim_matches('\'').to_string()
}

fn from_uri(uri: &str) -> String {
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    // %20 and friends
    let b = path.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&path[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The picture on the desktop right now.
pub fn current() -> Option<String> {
    if kde() {
        // ~/.config/plasma-org.kde.plasma.desktop-appletsrc: Image=file:///…
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let text = std::fs::read_to_string(home.join(".config/plasma-org.kde.plasma.desktop-appletsrc")).ok()?;
        return text.lines().filter_map(|l| l.strip_prefix("Image=")).last().map(from_uri).filter(|p| !p.is_empty());
    }
    let dark = output("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"]).is_some_and(|s| s.contains("dark"));
    let key = if dark { "picture-uri-dark" } else { "picture-uri" };
    output("gsettings", &["get", "org.gnome.desktop.background", key])
        .map(|s| from_uri(&unquote(&s)))
        .filter(|p| !p.is_empty())
        .or_else(|| output("gsettings", &["get", "org.gnome.desktop.background", "picture-uri"]).map(|s| from_uri(&unquote(&s))))
}

/// Puts `path` on the desktop (light and dark style alike on GNOME).
pub fn set(path: &str) -> bool {
    if kde() {
        return output("plasma-apply-wallpaperimage", &[path]).is_some();
    }
    let uri = format!("file://{path}");
    let light = output("gsettings", &["set", "org.gnome.desktop.background", "picture-uri", &uri]).is_some();
    let _ = output("gsettings", &["set", "org.gnome.desktop.background", "picture-uri-dark", &uri]);
    light
}

/// How the picture fills the screen, as an index in the switch (0 = Fill).
pub fn position() -> i32 {
    output("gsettings", &["get", "org.gnome.desktop.background", "picture-options"])
        .and_then(|s| POSITIONS.iter().position(|p| *p == unquote(&s)))
        .unwrap_or(0) as i32
}

pub fn set_position(i: i32) -> bool {
    let Some(p) = POSITIONS.get(i.max(0) as usize) else { return false };
    output("gsettings", &["set", "org.gnome.desktop.background", "picture-options", p]).is_some()
}

/// Pictures in `dir` and its subfolders (a few levels deep), sorted by name.
pub fn pictures(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if out.len() >= 400 {
                return;
            }
            if p.is_dir() {
                if depth < 3 {
                    walk(&p, depth + 1, out);
                }
            } else if p.extension().is_some_and(|e| PICTURES.contains(&e.to_string_lossy().to_lowercase().as_str())) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out
}

/// The wallpapers that come with the desktop.
pub fn windows_dir() -> PathBuf {
    ["/usr/share/backgrounds", "/usr/share/wallpapers"].iter().map(PathBuf::from).find(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from("/usr/share/backgrounds"))
}

/// A thumbnail about `size` px on its long side, as (width, height, RGBA).
pub fn thumbnail(path: &str, size: i32) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let t = img.thumbnail(size.max(1) as u32, size.max(1) as u32).to_rgba8();
    Some((t.width(), t.height(), t.into_raw()))
}
