//! The phone link: the Winklet Android app shows the Deck and presses its keys over the Wi-Fi,
//! and sends its notifications, calls and battery the other way. A small web server on the
//! LAN address behind a secret path that stays the same (so the phone reconnects by itself)
//! until "New link". Like Shelf > Send, a device that keeps guessing wrong gets blocked.
//!
//! Every message is sealed with AES-256-GCM under a key that only travels in the pairing QR
//! code, after the `#` (a link's fragment is never sent over the network). So someone else on
//! the Wi-Fi can't read the notifications or the Deck, can't press keys, and can't replay a
//! message they recorded. Requests are `POST /<secret>/x` with `nonce | sealed envelope`,
//! the envelope being `{"ts":unix secs,"op":"deck"|"icon"|"press"|"phone","arg":..,"body":..}`;
//! the reply is `nonce | sealed bytes`, tied to the request's nonce.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Wrong-link or wrong-key requests allowed per device before it is blocked.
const MAX_BAD_REQUESTS: u32 = 20;

/// Largest sealed request accepted (a notification's title and text fit easily).
const MAX_REQUEST: u64 = 32 * 1024;

/// How far a request's clock may be from the PC's, in seconds.
const MAX_SKEW: u64 = 300;

const REQ_AAD: &[u8] = b"winklet-req";
const RES_AAD: &[u8] = b"winklet-res";

/// What the phone sees: the keys as JSON, and the program icons as PNGs by key id.
#[derive(Default)]
pub struct Snapshot {
    pub json: String,
    pub icons: HashMap<String, Vec<u8>>,
}

pub struct Remote {
    /// the pairing link, key included: what the QR code holds
    pub url: String,
    pub port: u16,
    stop: AtomicBool,
    snap: Mutex<Snapshot>,
    bad: Mutex<HashMap<String, u32>>,
    /// nonces of recent requests (with their time), so a recorded one can't be sent again
    used: Mutex<HashMap<[u8; 12], u64>>,
    cipher: Aes256Gcm,
    /// when the phone last checked in (unix seconds), 0 = never
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

/// A fresh random 256-bit key, as 64 hex digits.
pub fn new_key() -> String {
    let mut k = [0u8; 32];
    OsRng.fill_bytes(&mut k);
    k.iter().map(|b| format!("{b:02x}")).collect()
}

fn key_bytes(hex: &str) -> Option<[u8; 32]> {
    let mut k = [0u8; 32];
    if hex.len() != 64 {
        return None;
    }
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(k)
}

type Handler = dyn Fn(String) + Send + Sync;

/// Starts serving on `port` if it's free (so a paired phone finds the PC again), else on
/// another. `press` runs on a server thread with the id of the key pressed, `phone` with the
/// JSON of an event the phone sent (a notification, a call, or its battery).
pub fn start(
    token: &str,
    key: &str,
    port: u16,
    press: impl Fn(String) + Send + Sync + 'static,
    phone: impl Fn(String) + Send + Sync + 'static,
) -> Result<Arc<Remote>, String> {
    let k = key_bytes(key).ok_or("The phone link's key is damaged: press New link")?;
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
        url: format!("http://{ip}:{port}/{token}#{key}"),
        port,
        stop: AtomicBool::new(false),
        snap: Mutex::new(Snapshot::default()),
        bad: Mutex::new(HashMap::new()),
        used: Mutex::new(HashMap::new()),
        cipher: Aes256Gcm::new(&k.into()),
        seen: AtomicU64::new(0),
    });
    let press: Arc<Handler> = Arc::new(press);
    let phone: Arc<Handler> = Arc::new(phone);
    let (r, tok) = (remote.clone(), token.to_string());
    std::thread::Builder::new()
        .name("phone-deck".into())
        .spawn(move || {
            while !r.stop.load(Ordering::Relaxed) {
                let Ok(Some(req)) = http.recv_timeout(Duration::from_millis(400)) else { continue };
                let (r, tok, press, phone) = (r.clone(), tok.clone(), press.clone(), phone.clone());
                std::thread::spawn(move || handle(req, &tok, &r, &*press, &*phone));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(remote)
}

fn header(k: &str, v: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn handle(mut req: tiny_http::Request, tok: &str, r: &Remote, press: &Handler, phone: &Handler) {
    let url = req.url().split('?').next().unwrap_or("").to_string();
    let base = format!("/{tok}");
    let client = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let strike = |r: &Remote| *r.bad.lock().unwrap().entry(client.clone()).or_insert(0) += 1;
    if r.bad.lock().unwrap().get(&client).copied().unwrap_or(0) >= MAX_BAD_REQUESTS {
        let _ = req.respond(tiny_http::Response::from_string("Too many requests").with_status_code(429));
        return;
    }
    if url != base && !url.starts_with(&format!("{base}/")) {
        strike(r);
        let _ = req.respond(tiny_http::Response::from_string("Not found").with_status_code(404));
        return;
    }
    let rest = url[base.len()..].trim_start_matches('/');
    match (req.method(), rest) {
        // scanned with the phone's camera instead of the app
        (tiny_http::Method::Get, "") => {
            let _ = req.respond(tiny_http::Response::from_string(PAGE).with_header(header("Content-Type", "text/html; charset=utf-8")));
        }
        (tiny_http::Method::Post, "x") => {
            use std::io::Read;
            let mut body = Vec::new();
            let _ = req.as_reader().take(MAX_REQUEST).read_to_end(&mut body);
            match open(r, &body) {
                Some((nonce, env)) => {
                    r.seen.store(now(), Ordering::Relaxed);
                    let reply = match answer(r, &env, press, phone) {
                        Some(bytes) => bytes,
                        None => {
                            let _ = req.respond(tiny_http::Response::from_string("Not found").with_status_code(404));
                            return;
                        }
                    };
                    let sealed = seal(r, &nonce, &reply);
                    let _ = req.respond(
                        tiny_http::Response::from_data(sealed)
                            .with_header(header("Content-Type", "application/octet-stream"))
                            .with_header(header("Cache-Control", "no-store")),
                    );
                }
                None => {
                    // wrong key, a replay, or a stale clock
                    strike(r);
                    let _ = req.respond(tiny_http::Response::from_string("Bad request").with_status_code(400));
                }
            }
        }
        _ => {
            let _ = req.respond(tiny_http::Response::from_string("Not found").with_status_code(404));
        }
    }
}

/// Opens a sealed request: checks the key, the clock and that its nonce wasn't used before.
fn open(r: &Remote, body: &[u8]) -> Option<([u8; 12], serde_json::Value)> {
    if body.len() < 12 + 16 {
        return None;
    }
    let nonce: [u8; 12] = body[..12].try_into().ok()?;
    let plain = r.cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &body[12..], aad: REQ_AAD }).ok()?;
    let env: serde_json::Value = serde_json::from_slice(&plain).ok()?;
    let ts = env.get("ts")?.as_u64()?;
    let t = now();
    if t.abs_diff(ts) > MAX_SKEW {
        return None;
    }
    let mut used = r.used.lock().unwrap();
    used.retain(|_, at| t.saturating_sub(*at) <= 2 * MAX_SKEW);
    if used.insert(nonce, t).is_some() {
        return None;
    }
    Some((nonce, env))
}

/// Seals a reply, tied to the request it answers.
fn seal(r: &Remote, req_nonce: &[u8; 12], reply: &[u8]) -> Vec<u8> {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let aad = [RES_AAD, req_nonce.as_slice()].concat();
    let sealed = r.cipher.encrypt(&nonce, Payload { msg: reply, aad: &aad }).unwrap_or_default();
    [nonce.as_slice(), &sealed].concat()
}

/// Does what an opened request asks; None for something unknown.
fn answer(r: &Remote, env: &serde_json::Value, press: &Handler, phone: &Handler) -> Option<Vec<u8>> {
    let arg = env.get("arg").and_then(|a| a.as_str()).unwrap_or("");
    match env.get("op")?.as_str()? {
        "deck" => Some(r.snap.lock().unwrap().json.clone().into_bytes()),
        "icon" => r.snap.lock().unwrap().icons.get(arg).cloned(),
        "press" => {
            let known = r.snap.lock().unwrap().json.contains(&format!("\"id\":\"{arg}\""));
            if !(known && !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric())) {
                return None;
            }
            press(arg.to_string());
            Some(b"ok".to_vec())
        }
        "phone" => {
            let body = env.get("body")?.as_str()?;
            serde_json::from_str::<serde_json::Value>(body).ok().filter(|v| v.is_object())?;
            phone(body.to_string());
            Some(b"ok".to_vec())
        }
        _ => None,
    }
}

const PAGE: &str = r##"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Winklet Deck</title><style>
body{font-family:system-ui,sans-serif;background:#0b0b10;color:#f2f2f7;display:grid;place-items:center;min-height:100vh;margin:0;padding:24px;text-align:center}
h1{font-size:22px;margin:0 0 10px}p{color:#8e8e99;font-size:15px;max-width:340px;line-height:1.45}
</style></head><body><div><h1>Winklet Deck</h1><p>Open the Winklet app on your phone, tap <b>Connect PC deck</b> and scan this code there.</p></div></body></html>"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(key: &str) -> Remote {
        Remote {
            url: String::new(),
            port: 0,
            stop: AtomicBool::new(false),
            snap: Mutex::new(Snapshot { json: r#"{"keys":[{"id":"abc123"}]}"#.into(), icons: HashMap::new() }),
            bad: Mutex::new(HashMap::new()),
            used: Mutex::new(HashMap::new()),
            cipher: Aes256Gcm::new(&key_bytes(key).unwrap().into()),
            seen: AtomicU64::new(0),
        }
    }

    /// What the phone does: nonce | seal(envelope).
    fn request(r: &Remote, env: serde_json::Value) -> Vec<u8> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let sealed = r.cipher.encrypt(&nonce, Payload { msg: env.to_string().as_bytes(), aad: REQ_AAD }).unwrap();
        [nonce.as_slice(), &sealed].concat()
    }

    #[test]
    fn sealed_requests() {
        let key = new_key();
        let r = remote(&key);
        let ok = request(&r, serde_json::json!({ "ts": now(), "op": "deck" }));
        let (nonce, env) = open(&r, &ok).expect("opens");
        assert_eq!(env["op"], "deck");
        // the reply opens with the request's nonce
        let reply = seal(&r, &nonce, b"hello");
        let aad = [RES_AAD, nonce.as_slice()].concat();
        let plain = r.cipher.decrypt(Nonce::from_slice(&reply[..12]), Payload { msg: &reply[12..], aad: &aad }).unwrap();
        assert_eq!(plain, b"hello");
        // the same request again is refused
        assert!(open(&r, &ok).is_none());
        // an old one is refused
        assert!(open(&r, &request(&r, serde_json::json!({ "ts": now() - 3600, "op": "deck" }))).is_none());
        // another key can't open it
        assert!(open(&remote(&new_key()), &request(&r, serde_json::json!({ "ts": now(), "op": "deck" }))).is_none());
        // tampering breaks it
        let mut bad = request(&r, serde_json::json!({ "ts": now(), "op": "deck" }));
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(open(&r, &bad).is_none());
    }

    #[test]
    fn only_deck_keys_press() {
        let r = remote(&new_key());
        let none = |_: String| {};
        assert!(answer(&r, &serde_json::json!({ "op": "press", "arg": "abc123" }), &none, &none).is_some());
        assert!(answer(&r, &serde_json::json!({ "op": "press", "arg": "zzz" }), &none, &none).is_none());
        assert!(answer(&r, &serde_json::json!({ "op": "phone", "body": "not json" }), &none, &none).is_none());
    }
}
