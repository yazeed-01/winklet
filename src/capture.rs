//! Instant full-screen screenshots: GDI grab on the UI thread (~20 ms), then
//! PNG encoding + clipboard on a short-lived worker thread.

use std::path::PathBuf;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_DIB;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

pub struct Shot {
    pub width: u32,
    pub height: u32,
    /// top-down BGRA
    pub bgra: Vec<u8>,
}

/// Grabs every monitor (the whole virtual desktop).
pub fn grab() -> Option<Shot> {
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        if w <= 0 || h <= 0 {
            return None;
        }
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let ok = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        (ok && lines > 0).then_some(Shot { width: w as u32, height: h as u32, bgra })
    }
}

/// Saves to Pictures\Screenshots and copies the image to the clipboard.
pub fn save_and_copy(shot: Shot) -> Option<PathBuf> {
    copy_to_clipboard(&shot);
    let dir = std::env::var_os("USERPROFILE").map(PathBuf::from)?.join("Pictures").join("Screenshots");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("Winklet {}.png", chrono::Local::now().format("%Y-%m-%d %H-%M-%S")));

    let mut rgba = shot.bgra;
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    let file = std::io::BufWriter::new(std::fs::File::create(&path).ok()?);
    let mut enc = png::Encoder::new(file, shot.width, shot.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().ok()?.write_image_data(&rgba).ok()?;
    Some(path)
}

fn copy_to_clipboard(shot: &Shot) {
    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: shot.width as i32,
        biHeight: shot.height as i32, // bottom-up: the most compatible CF_DIB layout
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let hsize = size_of::<BITMAPINFOHEADER>();
    let stride = shot.width as usize * 4;
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = (|| -> windows::core::Result<()> {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, hsize + shot.bgra.len())?;
            let p = GlobalLock(mem) as *mut u8;
            std::ptr::copy_nonoverlapping(&header as *const _ as *const u8, p, hsize);
            for (i, row) in shot.bgra.chunks_exact(stride).rev().enumerate() {
                std::ptr::copy_nonoverlapping(row.as_ptr(), p.add(hsize + i * stride), stride);
            }
            let _ = GlobalUnlock(mem);
            SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(mem.0)))?;
            Ok(())
        })();
        let _ = CloseClipboard();
    }
}
