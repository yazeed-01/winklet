//! Send files to a phone or another PC on the same Wi-Fi (and receive from them) with a
//! QR code: a tiny web server on a random port behind a secret link. Folders are zipped.
//! Ported from Orbit's share (same safety rules: secret link, rate limit on wrong links,
//! safe file names, upload size cap, received files marked as from another device).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a link stays open.
pub const EXPIRY: Duration = Duration::from_secs(30 * 60);
/// Largest single upload accepted.
const MAX_UPLOAD: u64 = 8 * 1024 * 1024 * 1024;
/// Wrong-link requests allowed per device before it is blocked for this share.
const MAX_BAD_REQUESTS: u32 = 20;

/// What the server tells the island.
pub enum Event {
    /// a device opened the page (first time per device)
    Connected,
    Sent(String),
    Received(String),
    Expired,
}

/// One file offered for download: (name shown, file on disk, size).
#[derive(Clone)]
pub struct Offer {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

pub struct Server {
    pub url: String,
    pub started: Instant,
    stop: AtomicBool,
    pub downloads: AtomicU32,
    pub uploads: AtomicU32,
    files: Mutex<Vec<Offer>>,
    /// folder -> its zip, so re-sharing doesn't zip again
    zips: Mutex<HashMap<PathBuf, Offer>>,
    bad: Mutex<HashMap<String, u32>>,
    seen: Mutex<Vec<String>>,
    inbox: PathBuf,
}

impl Server {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn left(&self) -> Duration {
        EXPIRY.saturating_sub(self.started.elapsed())
    }

    /// Replaces what's offered (zipping folders off the calling thread is up to the caller).
    pub fn set_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        let files = self.prepare(paths)?;
        *self.files.lock().unwrap() = files;
        Ok(())
    }

    fn prepare(&self, paths: &[PathBuf]) -> Result<Vec<Offer>, String> {
        let mut out = Vec::new();
        for p in paths {
            if p.is_dir() {
                if let Some(z) = self.zips.lock().unwrap().get(p) {
                    out.push(z.clone());
                    continue;
                }
                let name = file_name(p);
                let dir = std::env::temp_dir().join("Winklet").join("share");
                let _ = std::fs::create_dir_all(&dir);
                let zip = unique(dir.join(format!("{name}.zip")));
                let status = std::process::Command::new("tar.exe")
                    .arg("-a")
                    .arg("-cf")
                    .arg(&zip)
                    .arg("-C")
                    .arg(p.parent().unwrap_or(Path::new(".")))
                    .arg(format!("./{name}")) // never read as an option, even if it starts with "-"
                    .creation_flags_no_window()
                    .status()
                    .map_err(|e| format!("Couldn't zip {name}: {e}"))?;
                if !status.success() {
                    return Err(format!("Couldn't zip {name}"));
                }
                let offer = Offer { name: format!("{name}.zip"), size: size_of_file(&zip), path: zip };
                self.zips.lock().unwrap().insert(p.clone(), offer.clone());
                out.push(offer);
            } else if p.is_file() {
                out.push(Offer { name: file_name(p), path: p.clone(), size: size_of_file(p) });
            }
        }
        Ok(out)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // zips made for this share
        for z in self.zips.lock().unwrap().values() {
            let _ = std::fs::remove_file(&z.path);
        }
    }
}

trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
    }
}

/// Starts sharing `paths` (may be empty: then the page only receives). Received files go to
/// `inbox`. `notify` runs on the server threads.
pub fn start(paths: &[PathBuf], inbox: PathBuf, notify: impl Fn(Event) + Send + Sync + 'static) -> Result<Arc<Server>, String> {
    let ip = lan_ip().ok_or("Not connected to a network. Connect to Wi-Fi and try again.")?;
    // only on the Wi-Fi / LAN adapter, not every network this PC is on
    let http = (49152..49200)
        .find_map(|port| tiny_http::Server::http((ip.as_str(), port)).ok())
        .or_else(|| tiny_http::Server::http((ip.as_str(), 0)).ok())
        .ok_or("Couldn't open a network port")?;
    let port = http.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    let tok = token();
    let server = Arc::new(Server {
        url: format!("http://{ip}:{port}/{tok}"),
        started: Instant::now(),
        stop: AtomicBool::new(false),
        downloads: AtomicU32::new(0),
        uploads: AtomicU32::new(0),
        files: Mutex::new(Vec::new()),
        zips: Mutex::new(HashMap::new()),
        bad: Mutex::new(HashMap::new()),
        seen: Mutex::new(Vec::new()),
        inbox,
    });
    server.set_files(paths)?;
    let notify = Arc::new(notify);
    let s = server.clone();
    std::thread::Builder::new()
        .name("share".into())
        .spawn(move || {
            while !s.stop.load(Ordering::Relaxed) && s.started.elapsed() < EXPIRY {
                let Ok(Some(req)) = http.recv_timeout(Duration::from_millis(400)) else { continue };
                let (s, tok, notify) = (s.clone(), tok.clone(), notify.clone());
                std::thread::spawn(move || handle(req, &tok, &s, &*notify));
            }
            if !s.stop.load(Ordering::Relaxed) {
                notify(Event::Expired);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(server)
}

/// The Wi-Fi / LAN address other devices can reach.
pub fn lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.168.0.1:80").or_else(|_| sock.connect("8.8.8.8:80")).ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

/// 122-bit random link secret (UUID v4 uses the OS random generator).
fn token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string())
}

fn size_of_file(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Creates `dir/name`, or "name (2).ext", "name (3).ext"… if taken, atomically
/// (create_new), so an existing file is never opened. None when nothing is free.
fn create_new(dir: &Path, name: &str) -> Option<(PathBuf, std::fs::File)> {
    let first = dir.join(name);
    let stem = first.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = first.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    std::iter::once(first).chain((2..10_000).map(|i| dir.join(format!("{stem} ({i}){ext}")))).find_map(|p| {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(f) => Some(Some((p, f))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
            Err(_) => Some(None),
        }
    })?
}

/// `path`, or "name (2).ext", "name (3).ext"… when it's taken.
pub fn unique(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    (2..10_000).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists()).unwrap_or(path)
}

pub fn human_size(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{b:.0} B")
    }
}

fn header(k: &str, v: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn pct_encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Invisible / direction-changing characters that can disguise a file name
/// (e.g. "photo{U+202E}gpj.exe" displaying as "photoexe.jpg").
fn is_format_char(c: char) -> bool {
    matches!(c as u32, 0x00AD | 0x061C | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB)
}

pub fn sanitize(name: &str) -> String {
    let n: String = name
        .chars()
        .filter(|c| !is_format_char(*c))
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let n = n.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).to_string();
    let n: String = n.chars().take(180).collect();
    // Windows reserved device names (CON, NUL, COM1…) can't be normal files
    let stem = n.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    if n.is_empty() {
        "file".into()
    } else if reserved {
        format!("_{n}")
    } else {
        n
    }
}

/// Marks a received file as coming from another device ("Mark of the Web"), so
/// SmartScreen / Office warn before running or opening it.
fn mark_of_the_web(path: &Path) {
    let mut ads = path.as_os_str().to_owned();
    ads.push(":Zone.Identifier");
    let _ = std::fs::write(ads, "[ZoneTransfer]\r\nZoneId=3\r\n");
}

fn handle(req: tiny_http::Request, tok: &str, s: &Server, notify: &(dyn Fn(Event) + Send + Sync)) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let base = format!("/{tok}");
    let client = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    // anyone guessing links gets cut off after a few misses
    if s.bad.lock().unwrap().get(&client).copied().unwrap_or(0) >= MAX_BAD_REQUESTS {
        let _ = req.respond(tiny_http::Response::from_string("Too many requests").with_status_code(429));
        return;
    }
    if path != base && !path.starts_with(&format!("{base}/")) {
        *s.bad.lock().unwrap().entry(client).or_insert(0) += 1;
        let _ = req.respond(tiny_http::Response::from_string("Not found").with_status_code(404));
        return;
    }
    let no_store = || header("Cache-Control", "no-store");
    if path == base || path == format!("{base}/") {
        let page = tiny_http::Response::from_string(PAGE).with_header(header("Content-Type", "text/html; charset=utf-8")).with_header(no_store());
        let _ = req.respond(page);
        let first = {
            let mut seen = s.seen.lock().unwrap();
            let new = !seen.contains(&client);
            if new {
                seen.push(client);
            }
            new
        };
        if first {
            notify(Event::Connected);
        }
        return;
    }
    // what's offered right now (the page polls it, so files added later show up)
    if path == format!("{base}/list") {
        let files = s.files.lock().unwrap().clone();
        let items: Vec<String> = files
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{{\"i\":{i},\"name\":{},\"size\":{}}}", json_str(&f.name), json_str(&human_size(f.size))))
            .collect();
        let body = format!("{{\"files\":[{}],\"left\":{}}}", items.join(","), s.left().as_secs());
        let _ = req.respond(tiny_http::Response::from_string(body).with_header(header("Content-Type", "application/json")).with_header(no_store()));
        return;
    }
    if let Some(idx) = path.strip_prefix(&format!("{base}/f/")).and_then(|i| i.parse::<usize>().ok()) {
        let offer = s.files.lock().unwrap().get(idx).cloned();
        if let Some(f) = offer {
            if let Ok(file) = std::fs::File::open(&f.path) {
                let disp = format!(
                    "attachment; filename=\"{}\"; filename*=UTF-8''{}",
                    f.name.replace(|c: char| !c.is_ascii() || c == '"', "_"),
                    pct_encode(&f.name)
                );
                let resp = tiny_http::Response::from_file(file)
                    .with_header(header("Content-Type", "application/octet-stream"))
                    .with_header(header("Content-Disposition", &disp));
                if req.respond(resp).is_ok() {
                    s.downloads.fetch_add(1, Ordering::Relaxed);
                    notify(Event::Sent(f.name));
                }
                return;
            }
        }
    }
    if path == format!("{base}/up") {
        let name = query.split('&').find_map(|kv| kv.strip_prefix("name=")).map(pct_decode).unwrap_or_else(|| "file".into());
        let _ = std::fs::create_dir_all(&s.inbox);
        let mut req = req;
        if req.body_length().is_some_and(|n| n as u64 > MAX_UPLOAD) {
            let _ = req.respond(tiny_http::Response::from_string("File too large").with_status_code(413));
            return;
        }
        // a brand-new file only: never opens (or later deletes) one that already exists
        let Some((target, file)) = create_new(&s.inbox, &sanitize(&name)) else {
            let _ = req.respond(tiny_http::Response::from_string("error").with_status_code(500));
            return;
        };
        let written = std::io::copy(&mut req.as_reader().take(MAX_UPLOAD + 1), &mut { file });
        if matches!(written, Ok(n) if n <= MAX_UPLOAD) {
            mark_of_the_web(&target);
            s.uploads.fetch_add(1, Ordering::Relaxed);
            notify(Event::Received(file_name(&target)));
            let _ = req.respond(tiny_http::Response::from_string("ok"));
        } else {
            let _ = std::fs::remove_file(&target);
            let _ = req.respond(tiny_http::Response::from_string("error").with_status_code(500));
        }
        return;
    }
    let _ = req.respond(tiny_http::Response::from_string("Not found").with_status_code(404));
}

/// The page the phone opens: files to download (refreshes by itself) and "Send to PC".
const PAGE: &str = r##"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Winklet Share</title><style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:-apple-system,system-ui,Segoe UI,Roboto,sans-serif;background:#0b0b10;color:#f2f2f7;min-height:100vh;padding:26px 18px 40px}
.wrap{max-width:520px;margin:0 auto}
.logo{display:flex;align-items:center;gap:12px;margin-bottom:22px}
.pill{width:58px;height:30px;border-radius:15px;background:#000;border:1px solid #ffffff1c;display:flex;align-items:center;justify-content:center;gap:8px}
.pill i{width:7px;height:12px;border-radius:4px;background:#fff;box-shadow:0 0 8px #fff8;animation:blink 4s infinite}
@keyframes blink{0%,46%,50%,100%{transform:scaleY(1)}48%{transform:scaleY(.1)}}
h1{font-size:21px;font-weight:700;letter-spacing:-.3px}
.sub{color:#8e8e99;font-size:13.5px;margin-top:2px}
h2{font-size:13px;color:#8e8e99;font-weight:600;margin:22px 2px 10px;text-transform:uppercase;letter-spacing:.6px}
.card{background:#16161d;border:1px solid #ffffff10;border-radius:16px;padding:12px 12px 12px 14px;display:flex;align-items:center;gap:12px;margin-bottom:8px;animation:in .3s ease both}
@keyframes in{from{opacity:0;transform:translateY(6px)}}
.ic{width:40px;height:40px;border-radius:11px;background:#8b7bff22;color:#a99bff;display:grid;place-items:center;font-weight:700;font-size:10.5px;flex-shrink:0;text-transform:uppercase}
.meta{flex:1;min-width:0}
.name{font-weight:600;font-size:15px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.size{color:#8e8e99;font-size:12.5px;margin-top:2px}
.btn{background:#8b7bff;color:#0b0b12;border:none;border-radius:11px;padding:9px 14px;font-weight:700;font-size:14px;text-decoration:none;display:inline-block}
.btn.big{display:block;text-align:center;width:100%;padding:15px;font-size:16px;border-radius:15px;margin-top:6px;cursor:pointer}
.btn.green{background:#34d399}
.btn.dark{background:#22222c;color:#f2f2f7;border:1px solid #ffffff14}
.bar{height:4px;border-radius:2px;background:#ffffff14;margin-top:7px;overflow:hidden}
.bar i{display:block;height:100%;width:0;background:#34d399;transition:width .2s}
.ok{color:#34d399;font-weight:700}
.empty{color:#5c5c66;font-size:14px;padding:14px 4px}
.foot{color:#5c5c66;font-size:12px;text-align:center;margin-top:28px}
</style></head><body><div class="wrap">
<div class="logo"><div class="pill"><i></i><i></i></div><div><h1>Winklet Share</h1><div class="sub" id="sub">From your PC</div></div></div>
<h2>From the PC</h2><div id="files"><div class="empty">Loading…</div></div><div id="all"></div>
<h2>Send to the PC</h2><label class="btn big dark">Choose files<input id="f" type="file" multiple style="display:none"></label>
<div id="list" style="margin-top:12px"></div>
<div class="foot" id="foot">Saved in the PC's Downloads folder · works on this Wi-Fi only</div></div>
<script>
var base=location.pathname.replace(/\/$/,''),shown='';
function esc(s){return s.replace(/[&<>"]/g,function(c){return{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]})}
function load(){fetch(base+'/list',{cache:'no-store'}).then(function(r){return r.json()}).then(function(d){
 var key=JSON.stringify(d.files);var m=Math.floor(d.left/60);document.getElementById('foot').textContent='Saved in the PC\'s Downloads folder · link open for '+m+' more min';
 if(key==shown)return;shown=key;var h='';
 d.files.forEach(function(f,i){var ext=(f.name.split('.').pop()||'').slice(0,4);h+='<div class="card" style="animation-delay:'+(i*40)+'ms"><div class="ic">'+esc(ext)+'</div><div class="meta"><div class="name">'+esc(f.name)+'</div><div class="size">'+esc(f.size)+'</div></div><a class="btn dl" href="'+base+'/f/'+f.i+'" download="'+esc(f.name)+'">Get</a></div>'});
 document.getElementById('files').innerHTML=h||'<div class="empty">Nothing shared yet. Files added on the PC show up here.</div>';
 document.getElementById('sub').textContent=d.files.length?(d.files.length+(d.files.length==1?' file':' files')+' from your PC'):'Send files to your PC';
 document.getElementById('all').innerHTML=d.files.length>1?'<a class="btn big green" href="#" onclick="dlAll();return false">Download all</a>':'';
}).catch(function(){document.getElementById('foot').textContent='The PC stopped sharing'})}
function dlAll(){var a=document.querySelectorAll('.dl');a.forEach(function(x,i){setTimeout(function(){x.click()},i*700)})}
load();setInterval(load,3000);
var q=[],busy=false;
document.getElementById('f').onchange=function(e){for(var i=0;i<e.target.files.length;i++)q.push(add(e.target.files[i]));next();e.target.value=''};
function add(file){var c=document.createElement('div');c.className='card';c.innerHTML='<div class="ic">'+esc((file.name.split('.').pop()||'').slice(0,4))+'</div><div class="meta"><div class="name"></div><div class="size">Waiting…</div><div class="bar"><i></i></div></div>';c.querySelector('.name').textContent=file.name;document.getElementById('list').prepend(c);return{file:file,el:c}}
function next(){if(busy||!q.length)return;busy=true;var t=q.shift();var x=new XMLHttpRequest();x.open('POST',base+'/up?name='+encodeURIComponent(t.file.name));
x.upload.onprogress=function(e){if(e.lengthComputable){t.el.querySelector('i').style.width=(e.loaded/e.total*100)+'%';t.el.querySelector('.size').textContent=Math.round(e.loaded/e.total*100)+'%'}};
x.onload=function(){t.el.querySelector('.size').innerHTML=x.status==200?'<span class="ok">✓ Sent to PC</span>':'Failed';t.el.querySelector('i').style.width='100%';busy=false;next()};
x.onerror=function(){t.el.querySelector('.size').textContent='Failed';busy=false;next()};x.send(t.file)}
</script></body></html>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(sanitize("../../evil.exe"), "_.._evil.exe");
        assert_eq!(sanitize("CON.txt"), "_CON.txt");
        assert_eq!(sanitize("photo\u{202E}gpj.exe"), "photogpj.exe");
        assert_eq!(sanitize("   "), "file");
    }

    #[test]
    fn uploads_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("winklet-share-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a.txt"), "keep me").unwrap();
        let (p, _) = create_new(&dir, "a.txt").unwrap();
        assert_eq!(p.file_name().unwrap(), "a (2).txt");
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "keep me");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(48_200_000), "48.2 MB");
    }
}
