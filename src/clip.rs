//! Clipboard history, the Windows side: a small thread that notices every copy (by the
//! clipboard's sequence number), reads text, pictures or copied files, and puts an item
//! back on the clipboard when you pick it. Copies that password managers mark as private
//! are skipped, like Windows' own clipboard history does.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFOHEADER};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP};
use windows::core::w;

/// What a copy turned out to be.
pub enum Got {
    Text(String),
    Files(Vec<String>),
    /// saved as `<id>.png` with a small `<id>_t.png` next to it; `hash` finds repeats
    Image { id: String, w: u32, h: u32, hash: u64 },
}

/// texts longer than this are cut (a few hundred KB of text is plenty for a history)
const MAX_TEXT: usize = 200_000;

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
        let mut seq = unsafe { GetClipboardSequenceNumber() };
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let now = unsafe { GetClipboardSequenceNumber() };
            if now == seq {
                continue;
            }
            seq = now;
            if paused.load(Ordering::Relaxed) {
                continue;
            }
            // the app that copied may still be writing more formats
            std::thread::sleep(Duration::from_millis(60));
            if let Some(got) = read() {
                on_copy(got);
            }
        }
    });
}

/// Opens the clipboard, retrying briefly while another app has it open.
fn open() -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

fn private_copy() -> bool {
    unsafe {
        let skip = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let ignore = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
        if IsClipboardFormatAvailable(skip).is_ok() || IsClipboardFormatAvailable(ignore).is_ok() {
            return true;
        }
        let allow = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if IsClipboardFormatAvailable(allow).is_ok() {
            if let Ok(h) = GetClipboardData(allow) {
                let p = GlobalLock(HGLOBAL(h.0)) as *const u32;
                let no = !p.is_null() && *p == 0;
                let _ = GlobalUnlock(HGLOBAL(h.0));
                return no;
            }
        }
        false
    }
}

/// Copied files win over text, text over a picture (Office puts a picture next to copied text).
fn read() -> Option<Got> {
    if !open() {
        return None;
    }
    let got = unsafe {
        if private_copy() {
            None
        } else if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok() {
            read_files()
        } else if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok() {
            read_text()
        } else if IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok() {
            read_dib()
        } else {
            None
        }
    };
    let _ = unsafe { CloseClipboard() };
    // pictures are written to disk after the clipboard is closed again
    match got {
        Some(Raw::Image(w, h, rgba)) => save_image(w, h, &rgba),
        Some(Raw::Text(t)) => Some(Got::Text(t)),
        Some(Raw::Files(f)) => Some(Got::Files(f)),
        None => None,
    }
}

enum Raw {
    Text(String),
    Files(Vec<String>),
    Image(u32, u32, Vec<u8>),
}

unsafe fn read_text() -> Option<Raw> {
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() {
            return None;
        }
        let max = GlobalSize(g) / 2;
        let len = (0..max).take_while(|&i| *p.add(i) != 0).count();
        let mut text = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        let _ = GlobalUnlock(g);
        if text.trim().is_empty() {
            return None;
        }
        if text.len() > MAX_TEXT {
            let mut cut = MAX_TEXT;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        Some(Raw::Text(text))
    }
}

unsafe fn read_files() -> Option<Raw> {
    unsafe {
        let h = GetClipboardData(CF_HDROP.0 as u32).ok()?;
        let drop = HDROP(h.0);
        let n = DragQueryFileW(drop, u32::MAX, None);
        let files: Vec<String> = (0..n)
            .filter_map(|i| {
                let len = DragQueryFileW(drop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                (got > 0).then(|| String::from_utf16_lossy(&buf[..got]))
            })
            .collect();
        (!files.is_empty()).then_some(Raw::Files(files))
    }
}

/// A CF_DIB (24 or 32 bit, top-down or bottom-up) as RGBA.
unsafe fn read_dib() -> Option<Raw> {
    unsafe {
        let h = GetClipboardData(CF_DIB.0 as u32).ok()?;
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u8;
        if p.is_null() {
            return None;
        }
        let size = GlobalSize(g);
        let out = (|| {
            if size < size_of::<BITMAPINFOHEADER>() {
                return None;
            }
            let hdr = std::ptr::read_unaligned(p as *const BITMAPINFOHEADER);
            let (w, h) = (hdr.biWidth.unsigned_abs(), hdr.biHeight.unsigned_abs());
            let bpp = hdr.biBitCount as usize;
            if w == 0 || h == 0 || !(bpp == 24 || bpp == 32) || w > 16384 || h > 16384 {
                return None;
            }
            // BI_BITFIELDS with a plain header: three colour masks follow it
            let masks = if hdr.biCompression == 3 && hdr.biSize == 40 { 12 } else { 0 };
            let offset = hdr.biSize as usize + masks + hdr.biClrUsed as usize * 4;
            let stride = (w as usize * bpp).div_ceil(32) * 4;
            if offset + stride * h as usize > size {
                return None;
            }
            let bottom_up = hdr.biHeight > 0;
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            let mut any_alpha = false;
            for y in 0..h as usize {
                let src_row = if bottom_up { h as usize - 1 - y } else { y };
                let row = std::slice::from_raw_parts(p.add(offset + src_row * stride), stride);
                for x in 0..w as usize {
                    let s = &row[x * bpp / 8..];
                    let d = &mut rgba[(y * w as usize + x) * 4..][..4];
                    d[0] = s[2];
                    d[1] = s[1];
                    d[2] = s[0];
                    d[3] = if bpp == 32 { s[3] } else { 255 };
                    any_alpha |= bpp == 32 && s[3] != 0;
                }
            }
            if bpp == 32 && !any_alpha {
                rgba.chunks_mut(4).for_each(|px| px[3] = 255);
            }
            Some(Raw::Image(w, h, rgba))
        })();
        let _ = GlobalUnlock(g);
        out
    }
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> bool {
    let Ok(file) = std::fs::File::create(path) else { return false };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().and_then(|mut wr| wr.write_image_data(rgba)).is_ok()
}

/// Shrinks to fit `max` px (box filter), for the list's thumbnails.
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
    // FNV-1a over a sample of the pixels: enough to spot the same picture copied twice
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

/// Puts copied files back on the clipboard (as if copied in Explorer).
pub fn set_files(files: &[String]) -> bool {
    let mut wide: Vec<u16> = Vec::new();
    for f in files {
        wide.extend(f.encode_utf16());
        wide.push(0);
    }
    wide.push(0);
    let head = size_of::<DROPFILES>();
    unsafe {
        if !open() {
            return false;
        }
        let ok = (|| -> windows::core::Result<()> {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, head + wide.len() * 2)?;
            let p = GlobalLock(mem) as *mut u8;
            let df = DROPFILES { pFiles: head as u32, fWide: true.into(), ..Default::default() };
            std::ptr::copy_nonoverlapping(&df as *const _ as *const u8, p, head);
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, p.add(head), wide.len() * 2);
            let _ = GlobalUnlock(mem);
            SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(mem.0)))?;
            Ok(())
        })()
        .is_ok();
        let _ = CloseClipboard();
        ok
    }
}

/// Puts a picture (RGBA) back on the clipboard as a bottom-up 32-bit DIB.
pub fn set_image(w: u32, h: u32, rgba: &[u8]) -> bool {
    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w as i32,
        biHeight: h as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let hsize = size_of::<BITMAPINFOHEADER>();
    let stride = w as usize * 4;
    unsafe {
        if !open() {
            return false;
        }
        let ok = (|| -> windows::core::Result<()> {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, hsize + rgba.len())?;
            let p = GlobalLock(mem) as *mut u8;
            std::ptr::copy_nonoverlapping(&header as *const _ as *const u8, p, hsize);
            for (i, row) in rgba.chunks_exact(stride).rev().enumerate() {
                let dst = p.add(hsize + i * stride);
                for (x, px) in row.chunks_exact(4).enumerate() {
                    *dst.add(x * 4) = px[2];
                    *dst.add(x * 4 + 1) = px[1];
                    *dst.add(x * 4 + 2) = px[0];
                    *dst.add(x * 4 + 3) = px[3];
                }
            }
            let _ = GlobalUnlock(mem);
            SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(mem.0)))?;
            Ok(())
        })()
        .is_ok();
        let _ = CloseClipboard();
        ok
    }
}

#[cfg(test)]
mod tests {
    /// Reads what's on the clipboard now (read-only).
    #[test]
    #[ignore]
    fn probe_clipboard() {
        match super::read() {
            Some(super::Got::Text(t)) => println!("text, {} chars", t.chars().count()),
            Some(super::Got::Files(f)) => println!("{} files", f.len()),
            Some(super::Got::Image { w, h, .. }) => println!("image {w}x{h} (saved to the clips folder)"),
            None => println!("nothing readable"),
        }
    }
}
