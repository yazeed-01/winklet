//! What a snip is for besides the picture itself: the text in it (Windows' own offline
//! OCR, copied as plain text) or a Google Lens search for it.

use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

/// The text in an RGBA picture, one line per line of text. Blocks: run it off the UI thread.
pub fn ocr(w: u32, h: u32, rgba: &[u8]) -> windows::core::Result<String> {
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    let max = OcrEngine::MaxImageDimension()?;
    // small snips read much better at twice the size
    let scale = if w.max(h) < 1200 && w.max(h) * 2 <= max { 2 } else { 1 };
    let (sw, sh) = (w * scale, h * scale);
    if sw > max || sh > max {
        return Err(windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG));
    }
    let mut bgra = vec![0u8; (sw * sh * 4) as usize];
    for y in 0..sh {
        for x in 0..sw {
            let s = &rgba[(((y / scale) * w + x / scale) * 4) as usize..][..4];
            let d = &mut bgra[((y * sw + x) * 4) as usize..][..4];
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = 255;
        }
    }
    let writer = DataWriter::new()?;
    writer.WriteBytes(&bgra)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, sw as i32, sh as i32)?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let mut text = String::new();
    for line in result.Lines()? {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&line.Text()?.to_string_lossy());
    }
    Ok(text.trim().to_string())
}

/// Opens Google Lens on the picture in the default browser. Lens only takes uploads, so a
/// small local page holds the picture and posts it to Lens the moment it opens.
pub fn lens(w: u32, h: u32, rgba: &[u8]) -> bool {
    let Some(png) = encode_png(w, h, rgba) else { return false };
    let page = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Google Lens</title>
<style>body{{background:#111;color:#bbb;font:14px "Segoe UI",sans-serif;display:grid;place-items:center;height:100vh;margin:0}}</style></head>
<body><p>Opening Google Lens…</p>
<form id="f" method="POST" enctype="multipart/form-data"><input type="file" name="encoded_image" id="i" hidden></form>
<script>
const b = atob("{}"), u = new Uint8Array(b.length);
for (let k = 0; k < b.length; k++) u[k] = b.charCodeAt(k);
const t = new DataTransfer();
t.items.add(new File([u], "snip.png", {{ type: "image/png" }}));
document.getElementById("i").files = t.files;
const f = document.getElementById("f");
f.action = "https://lens.google.com/v3/upload?ep=ccm&s=&st=" + Date.now();
f.submit();
</script></body></html>"#,
        base64(&png)
    );
    let path = std::env::temp_dir().join("winklet-lens.html");
    std::fs::write(&path, page).is_ok() && crate::sys::open(&path.to_string_lossy(), "")
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header().ok()?.write_image_data(rgba).ok()?;
    }
    Some(out)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}
