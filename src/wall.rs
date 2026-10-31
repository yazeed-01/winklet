//! Desktop wallpaper: read and set it through IDesktopWallpaper, and shell thumbnails of
//! pictures (Explorer's own thumbnail cache, so even big photos come back quickly).

use std::path::{Path, PathBuf};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetObjectW, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL, DWPOS_FIT, DWPOS_SPAN, DWPOS_STRETCH, DesktopWallpaper, IDesktopWallpaper,
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
};
use windows::core::{HSTRING, PCWSTR};

/// Fill, Fit, Stretch, Centre, Span: in the order of the switch on the Wallpaper tab.
const POSITIONS: [DESKTOP_WALLPAPER_POSITION; 5] = [DWPOS_FILL, DWPOS_FIT, DWPOS_STRETCH, DWPOS_CENTER, DWPOS_SPAN];

const PICTURES: [&str; 6] = ["jpg", "jpeg", "png", "bmp", "webp", "jfif"];

fn desktop() -> Option<IDesktopWallpaper> {
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL).ok() }
}

/// The picture on the (first) monitor right now.
pub fn current() -> Option<String> {
    let d = desktop()?;
    unsafe {
        let mon = d.GetMonitorDevicePathAt(0).ok()?;
        let path = d.GetWallpaper(PCWSTR(mon.0));
        CoTaskMemFree(Some(mon.0 as *const _));
        let path = path.ok()?;
        let s = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const _));
        s.filter(|s| !s.is_empty())
    }
}

/// Puts `path` on every monitor.
pub fn set(path: &str) -> bool {
    let Some(d) = desktop() else { return false };
    unsafe { d.SetWallpaper(PCWSTR::null(), &HSTRING::from(path)).is_ok() }
}

/// How the picture fills the screen, as an index in the switch (0 = Fill).
pub fn position() -> i32 {
    desktop()
        .and_then(|d| unsafe { d.GetPosition() }.ok())
        .and_then(|p| POSITIONS.iter().position(|x| *x == p))
        .unwrap_or(0) as i32
}

pub fn set_position(i: i32) -> bool {
    let (Some(d), Some(p)) = (desktop(), POSITIONS.get(i.max(0) as usize)) else { return false };
    unsafe { d.SetPosition(*p).is_ok() }
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

/// Windows' own wallpapers.
pub fn windows_dir() -> PathBuf {
    PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join(r"Web\Wallpaper")
}

/// A thumbnail about `size` px on its long side, as (width, height, RGBA). COM must be
/// initialised on the calling thread.
pub fn thumbnail(path: &str, size: i32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let item: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        let hbm = item.GetImage(SIZE { cx: size, cy: size }, SIIGBF_BIGGERSIZEOK).ok()?;
        let out = (|| {
            let mut bm = BITMAP::default();
            if GetObjectW(HGDIOBJ(hbm.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth.max(1) as u32, bm.bmHeight.unsigned_abs().max(1));
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = GetDC(None);
            let mut px = vec![0u8; (w * h * 4) as usize];
            let lines = GetDIBits(dc, hbm, 0, h, Some(px.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
            ReleaseDC(None, dc);
            if lines == 0 {
                return None;
            }
            // photos come back without alpha
            let opaque = px.chunks(4).all(|p| p[3] == 0);
            for p in px.chunks_mut(4) {
                p.swap(0, 2); // BGRA -> RGBA
                if opaque {
                    p[3] = 255;
                }
            }
            Some((w, h, px))
        })();
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-applies the wallpaper already on the desktop, so nothing visibly changes.
    #[test]
    #[ignore]
    fn probe_wallpaper() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let cur = current().expect("current wallpaper");
        println!("current: {cur}, position {}", position());
        assert!(set(&cur));
        assert!(set_position(position()));
        let t = thumbnail(&cur, 200).expect("thumbnail");
        println!("thumb {}x{}", t.0, t.1);
        println!("{} pictures in Windows' folder", pictures(&windows_dir()).len());
    }
}
