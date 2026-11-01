//! Now-playing info for any app (Spotify, browsers, VLC, ...) through MPRIS, the desktop's
//! media player interface, using the `playerctl` command. Runs on its own thread and only
//! reports when something actually changed.

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

pub enum Cmd {
    Toggle,
    Next,
    Prev,
}

#[derive(Clone, Default, PartialEq)]
pub struct Info {
    pub has: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub app: String,
    /// seconds; only filled while `want_position` is set
    pub pos: f64,
    pub len: f64,
}

fn playerctl(args: &[&str]) -> Option<String> {
    let out = Command::new("playerctl").args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
}

/// `want_position`: the UI is showing the progress bar, so report position too.
pub fn spawn(want_position: Arc<AtomicBool>, on_change: impl Fn(Info) + Send + 'static) -> Sender<Cmd> {
    let (tx, rx) = mpsc::channel::<Cmd>();
    std::thread::Builder::new()
        .name("media".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let mut last = Info::default();
            loop {
                let wait = if want_position.load(Ordering::Relaxed) { 1000 } else { 2000 };
                match rx.recv_timeout(Duration::from_millis(wait)) {
                    Ok(cmd) => {
                        let _ = playerctl(&[match cmd {
                            Cmd::Toggle => "play-pause",
                            Cmd::Next => "next",
                            Cmd::Prev => "previous",
                        }]);
                        // give the player a moment to publish its new state
                        std::thread::sleep(Duration::from_millis(250));
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                let info = read(want_position.load(Ordering::Relaxed)).unwrap_or_default();
                if info != last {
                    on_change(info.clone());
                    last = info;
                }
            }
        })
        .expect("spawn media thread");
    tx
}

fn read(with_pos: bool) -> Option<Info> {
    // one call for everything; fields split by a character no title contains
    let fmt = "{{status}}\u{1f}{{title}}\u{1f}{{artist}}\u{1f}{{playerName}}\u{1f}{{position}}\u{1f}{{mpris:length}}";
    let line = playerctl(&["metadata", "--format", fmt])?;
    let f: Vec<&str> = line.split('\u{1f}').collect();
    if f.len() < 6 || f[0].is_empty() || f[0] == "Stopped" {
        return None;
    }
    let playing = f[0] == "Playing";
    let mut info = Info {
        has: true,
        playing,
        title: f[1].to_string(),
        artist: f[2].to_string(),
        app: pretty_app(f[3]),
        ..Default::default()
    };
    if with_pos {
        // microseconds
        let us = |s: &str| s.trim().parse::<f64>().unwrap_or(0.0) / 1_000_000.0;
        let (pos, end) = (us(f[4]), us(f[5]));
        info.len = end.max(0.0);
        info.pos = if end > 0.0 { pos.min(end) } else { 0.0 };
    }
    if info.title.is_empty() {
        info.title = info.app.clone();
    }
    Some(info)
}

fn pretty_app(player: &str) -> String {
    // "chromium.instance1234" → "chromium"
    let id = player.split('.').next().unwrap_or(player);
    match id.to_lowercase().as_str() {
        "chromium" => "Chromium".into(),
        "chrome" | "google-chrome" => "Google Chrome".into(),
        "firefox" => "Firefox".into(),
        "spotify" => "Spotify".into(),
        "vlc" => "VLC".into(),
        "brave" => "Brave".into(),
        "msedge" | "edge" => "Microsoft Edge".into(),
        _ => {
            let mut c = id.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}
