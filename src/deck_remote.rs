//! Phone deck: the Winklet Android app shows the Deck and presses its keys over the Wi-Fi.
//! A small web server on the LAN address behind a long secret that stays the same (so the
//! phone reconnects by itself) until "New link". Like Shelf > Send, a device that keeps
//! guessing wrong links gets blocked. Only the keys on the Deck can be pressed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Wrong-link requests allowed per device before it is blocked.
const MAX_BAD_REQUESTS: u32 = 20;

/// What the phone sees: the keys as JSON, and the program icons as PNGs by key id.
#[derive(Default)]
pub struct Snapshot {
    pub json: String,
    pub icons: HashMap<String, Vec<u8>>,
}

pub struct Remote {
    pub url: String,
    pub port: u16,
    stop: AtomicBool,
    snap: Mutex<Snapshot>,
    bad: Mutex<HashMap<String, u32>>,
    /// when the phone last asked for the deck (unix seconds), 0 = never
    seen: AtomicU64,
}

impl Remote {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn set(&self, snap: Snapshot) {
        *self.snap.lock().unwrap() = snap;
    }

    /// Seconds since the phone last checked in, None if it never has.
    pub fn seen_ago(&self) -> Option<u64> {
        match self.seen.load(Ordering::Relaxed) {
            0 => None,
            t => Some(now().saturating_sub(t)),
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Starts serving on `port` if it's free (so a paired phone finds the PC again), else on
/// another. `press` runs on a server thread with the id of the key pressed.
pub fn start(token: &str, port: u16, press: impl Fn(String) + Send + Sync + 'static) -> Result<Arc<Remote>, String> {
    let ip = crate::share::lan_ip().ok_or("Not connected to a network. Connect to Wi-Fi and try again.")?;
    // only on the Wi-Fi / LAN adapter, not every network this PC is on
    let http = std::iter::once(port)
        .filter(|p| *p != 0)
        .chain(49200..49260)
        .find_map(|p| tiny_http::Server::http((ip.as_str(), p)).ok())
        .or_else(|| tiny_http::Server::http((ip.as_str(), 0)).ok())
        .ok_or("Couldn't open a network port")?;
    let port = http.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    let remote = Arc::new(Remote {
        url: format!("http://{ip}:{port}/{token}"),
        port,
        stop: AtomicBool::new(false),
        snap: Mutex::new(Snapshot::default()),
        bad: Mutex::new(HashMap::new()),
        seen: AtomicU64::new(0),
    });
    let press = Arc::new(press);
    let (r, tok) = (remote.clone(), token.to_string());
    std::thread::Builder::new()
        .name("phone-deck".into())
        .spawn(move || {
            while !r.stop.load(Ordering::Relaxed) {
                let Ok(Some(req)) = http.recv_timeout(Duration::from_millis(400)) else { continue };
                let (r, tok, press) = (r.clone(), tok.clone(), press.clone());
                std::thread::spawn(move || handle(req, &tok, &r, &*press));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(remote)
}

fn header(k: &str, v: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn handle(req: tiny_http::Request, tok: &str, r: &Remote, press: &(dyn Fn(String) + Send + Sync)) {
    let url = req.url().split('?').next().unwrap_or("").to_string();
    let base = format!("/{tok}");
    let client = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    if r.bad.lock().unwrap().get(&client).copied().unwrap_or(0) >= MAX_BAD_REQUESTS {
        let _ = req.respond(tiny_http::Response::from_string("Too many requests").with_status_code(429));
        return;
    }
    if url != base && !url.starts_with(&format!("{base}/")) {
        *r.bad.lock().unwrap().entry(client).or_insert(0) += 1;
        let _ = req.respond(tiny_http::Response::from_string("Not found").with_status_code(404));
        return;
    }
    let rest = url[base.len()..].trim_start_matches('/');
    let no_store = || header("Cache-Control", "no-store");
    match (req.method(), rest) {
        // scanned with the phone's camera instead of the app
        (tiny_http::Method::Get, "") => {
            let _ = req.respond(tiny_http::Response::from_string(PAGE).with_header(header("Content-Type", "text/html; charset=utf-8")));
        }
        (tiny_http::Method::Get, "deck") => {
            r.seen.store(now(), Ordering::Relaxed);
            let json = r.snap.lock().unwrap().json.clone();
            let _ = req.respond(tiny_http::Response::from_string(json).with_header(header("Content-Type", "application/json")).with_header(no_store()));
        }
        (tiny_http::Method::Get, path) if path.starts_with("icon/") => {
            let png = r.snap.lock().unwrap().icons.get(&path["icon/".len()..]).cloned();
            let _ = match png {
                Some(png) => req.respond(tiny_http::Response::from_data(png).with_header(header("Content-Type", "image/png"))),
                None => req.respond(tiny_http::Response::from_string("Not found").with_status_code(404)),
            };
        }
        // a press is a POST, so nothing presses a key just by loading a link
        (tiny_http::Method::Post, path) if path.starts_with("press/") => {
            let id = path["press/".len()..].to_string();
            let known = r.snap.lock().unwrap().json.contains(&format!("\"id\":\"{id}\""));
            if known && !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) {
                r.seen.store(now(), Ordering::Relaxed);
                press(id);
                let _ = req.respond(tiny_http::Response::from_string("ok"));
            } else {
                let _ = req.respond(tiny_http::Response::from_string("No such key").with_status_code(404));
            }
        }
        _ => {
            let _ = req.respond(tiny_http::Response::from_string("Not found").with_status_code(404));
        }
    }
}

const PAGE: &str = r##"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Winklet Deck</title><style>
body{font-family:system-ui,sans-serif;background:#0b0b10;color:#f2f2f7;display:grid;place-items:center;min-height:100vh;margin:0;padding:24px;text-align:center}
h1{font-size:22px;margin:0 0 10px}p{color:#8e8e99;font-size:15px;max-width:340px;line-height:1.45}
</style></head><body><div><h1>Winklet Deck</h1><p>Open the Winklet app on your phone, tap <b>Connect PC deck</b> and scan this code there.</p></div></body></html>"##;
