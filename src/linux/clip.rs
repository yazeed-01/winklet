//! Clipboard history, the Linux side: a thread that hears every change of the CLIPBOARD
//! owner (XFixes), reads text, pictures or copied files, and puts an item back when you pick
//! it. Copies that password managers mark as private (x-kde-passwordManagerHint) are skipped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, WindowClass};

/// What a copy turned out to be.
pub enum Got {
    Text(String),
    Files(Vec<String>),
    /// saved as `<id>.png` with a small `<id>_t.png` next to it; `hash` finds repeats
    Image { id: String, w: u32, h: u32, hash: u64 },
}

/// texts longer than this are cut (a few hundred KB of text is plenty for a history)
const MAX_TEXT: usize = 200_000;

/// bumped by the watcher on every change of the clipboard's owner
static SEQ: AtomicU32 = AtomicU32::new(0);

/// One clipboard for the whole run: on X11 what we copy lives as long as it does.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_clipboard<R>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<R>) -> Option<R> {
    let mut c = CLIPBOARD.lock().ok()?;
    if c.is_none() {
        *c = arboard::Clipboard::new().ok();
    }
    f(c.as_mut()?)
}

pub fn dir() -> PathBuf {
    crate::store::data_dir().join("clips")
}

pub fn image_path(id: &str) -> PathBuf {
    dir().join(format!("{id}.png"))
}

pub fn thumb_path(id: &str) -> PathBuf {
    dir().join(format!("{id}_t.png"))
}

/// Watches the clipboard and calls `on_copy` (on the watcher thread) for every new copy.
pub fn watch(paused: Arc<AtomicBool>, on_copy: impl Fn(Got) + Send + 'static) {
    std::thread::spawn(move || {
        let Ok((conn, screen)) = x11rb::connect(None) else { return };
        let root = conn.setup().roots[screen].root;
        let atom = |name: &str| conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom).unwrap_or(0);
        let (clipboard, targets, private) = (atom("CLIPBOARD"), atom("TARGETS"), atom("x-kde-passwordManagerHint"));
        let into = atom("WINKLET_TARGETS");
        // a hidden window to receive the list of formats on offer
        let Ok(win) = conn.generate_id() else { return };
        let _ = conn.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new());
        if conn.xfixes_query_version(5, 0).ok().and_then(|c| c.reply().ok()).is_none() {
            return;
        }
        let mask = SelectionEventMask::SET_SELECTION_OWNER | SelectionEventMask::SELECTION_WINDOW_DESTROY | SelectionEventMask::SELECTION_CLIENT_CLOSE;
        let _ = conn.xfixes_select_selection_input(root, clipboard, mask);
        let _ = conn.flush();
        while let Ok(ev) = conn.wait_for_event() {
            let Event::XfixesSelectionNotify(_) = ev else { continue };
            SEQ.fetch_add(1, Ordering::Relaxed);
            if paused.load(Ordering::Relaxed) {
                continue;
            }
            // the app that copied may still be getting ready
            std::thread::sleep(Duration::from_millis(60));
            // the formats on offer: a private copy is never read
            let _ = conn.convert_selection(win, clipboard, targets, into, x11rb::CURRENT_TIME);
            let _ = conn.flush();
            let mut offered: Vec<u32> = Vec::new();
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while std::time::Instant::now() < deadline {
                match conn.poll_for_event() {
                    Ok(Some(Event::SelectionNotify(n))) if n.requestor == win => {
                        if n.property != 0 {
                            offered = conn
                                .get_property(true, win, into, AtomEnum::ANY, 0, 4096)
                                .ok()
                                .and_then(|c| c.reply().ok())
                                .and_then(|r| r.value32().map(|v| v.collect()))
                                .unwrap_or_default();
                        }
                        break;
                    }
                    Ok(Some(Event::XfixesSelectionNotify(_))) => {
                        SEQ.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) => std::thread::sleep(Duration::from_millis(10)),
                    Err(_) => return,
                }
            }
            if offered.contains(&private) {
                continue;
            }
            if let Some(got) = read() {
                on_copy(got);
            }
        }
    });
}

fn read() -> Option<Got> {
    let files = with_clipboard(|c| c.get().file_list().ok()).unwrap_or_default();
    if !files.is_empty() {
        return Some(Got::Files(files.iter().map(|p| p.to_string_lossy().into_owned()).collect()));
    }
    if let Some(mut t) = with_clipboard(|c| c.get_text().ok()) {
        if t.trim().is_empty() {
            return None;
        }
        if t.len() > MAX_TEXT {
            let mut cut = MAX_TEXT;
            while !t.is_char_boundary(cut) {
                cut -= 1;
            }
            t.truncate(cut);
        }
        return Some(Got::Text(t));
    }
    let img = with_clipboard(|c| c.get_image().ok())?;
    save_image(img.width as u32, img.height as u32, &img.bytes)
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> bool {
    let Ok(file) = std::fs::File::create(path) else { return false };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().and_then(|mut wr| wr.write_image_data(rgba)).is_ok()
}

/// Box-filtered thumbnail no bigger than `max` on its long side.
fn shrink(w: u32, h: u32, rgba: &[u8], max: u32) -> (u32, u32, Vec<u8>) {
    let scale = (w.max(h) as f32 / max as f32).max(1.0);
    let (tw, th) = (((w as f32 / scale) as u32).max(1), ((h as f32 / scale) as u32).max(1));
    let mut out = vec![0u8; (tw * th * 4) as usize];
    for ty in 0..th {
        for tx in 0..tw {
            let (x0, x1) = ((tx as f32 * scale) as u32, (((tx + 1) as f32 * scale) as u32).clamp(1, w));
            let (y0, y1) = ((ty as f32 * scale) as u32, (((ty + 1) as f32 * scale) as u32).clamp(1, h));
            let mut acc = [0u32; 4];
            let mut n = 0;
            for y in y0..y1.max(y0 + 1) {
                for x in x0..x1.max(x0 + 1) {
                    let i = ((y.min(h - 1) * w + x.min(w - 1)) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += rgba[i + c] as u32;
                    }
                    n += 1;
                }
            }
            let o = ((ty * tw + tx) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    (tw, th, out)
}

fn save_image(w: u32, h: u32, rgba: &[u8]) -> Option<Got> {
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return None;
    }
    let mut hash: u64 = 0xcbf29ce484222325 ^ ((w as u64) << 32 | h as u64);
    for b in rgba.iter().step_by(97) {
        hash = (hash ^ *b as u64).wrapping_mul(0x100000001b3);
    }
    let _ = std::fs::create_dir_all(dir());
    let id = crate::store::new_id();
    if !write_png(&image_path(&id), w, h, rgba) {
        return None;
    }
    let (tw, th, small) = shrink(w, h, rgba, 96);
    write_png(&thumb_path(&id), tw, th, &small);
    Some(Got::Image { id, w, h, hash })
}

pub fn set_text(text: &str) -> bool {
    with_clipboard(|c| c.set_text(text).ok()).is_some()
}

/// Puts copied files back on the clipboard (as if copied in the file manager).
pub fn set_files(files: &[String]) -> bool {
    with_clipboard(|c| c.set().file_list(files).ok()).is_some()
}

/// Puts a picture (RGBA) back on the clipboard.
pub fn set_image(w: u32, h: u32, rgba: &[u8]) -> bool {
    let img = arboard::ImageData { width: w as usize, height: h as usize, bytes: rgba.into() };
    with_clipboard(|c| c.set_image(img).ok()).is_some()
}

/// Bumps on every clipboard change, from any app.
pub fn seq() -> u32 {
    SEQ.load(Ordering::Relaxed)
}

/// The picture on the clipboard, as RGBA (None when there is no picture).
pub fn read_image() -> Option<(u32, u32, Vec<u8>)> {
    let img = with_clipboard(|c| c.get_image().ok())?;
    Some((img.width as u32, img.height as u32, img.bytes.into_owned()))
}
