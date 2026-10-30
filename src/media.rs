//! Now-playing info for any app (Spotify, browsers, Apple Music, ...) through
//! Windows' Global System Media Transport Controls. Runs on its own thread and
//! only reports when something actually changed.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

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

/// `want_position`: the UI is showing the progress bar, so report position too.
pub fn spawn(want_position: Arc<AtomicBool>, on_change: impl Fn(Info) + Send + 'static) -> Sender<Cmd> {
    let (tx, rx) = mpsc::channel::<Cmd>();
    std::thread::Builder::new()
        .name("media".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let Ok(mgr) = Manager::RequestAsync().and_then(|op| op.join()) else { return };
            let mut last = Info::default();
            loop {
                let wait = if want_position.load(Ordering::Relaxed) { 1000 } else { 2000 };
                match rx.recv_timeout(Duration::from_millis(wait)) {
                    Ok(cmd) => {
                        if let Ok(s) = mgr.GetCurrentSession() {
                            let _ = match cmd {
                                Cmd::Toggle => s.TryTogglePlayPauseAsync(),
                                Cmd::Next => s.TrySkipNextAsync(),
                                Cmd::Prev => s.TrySkipPreviousAsync(),
                            }
                            .and_then(|op| op.join());
                            // give the player a moment to publish its new state
                            std::thread::sleep(Duration::from_millis(250));
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                let info = read(&mgr, want_position.load(Ordering::Relaxed)).unwrap_or_default();
                if info != last {
                    on_change(info.clone());
                    last = info;
                }
            }
        })
        .expect("spawn media thread");
    tx
}

fn read(mgr: &Manager, with_pos: bool) -> windows::core::Result<Info> {
    let s: Session = mgr.GetCurrentSession()?;
    let props = s.TryGetMediaPropertiesAsync()?.join()?;
    let playing = s.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;
    let mut info = Info {
        has: true,
        playing,
        title: props.Title()?.to_string(),
        artist: props.Artist()?.to_string(),
        app: pretty_app(&s.SourceAppUserModelId()?.to_string()),
        ..Default::default()
    };
    if with_pos {
        let t = s.GetTimelineProperties()?;
        let ticks = |d: i64| d as f64 / 10_000_000.0;
        let end = ticks(t.EndTime()?.Duration) - ticks(t.StartTime()?.Duration);
        let mut pos = ticks(t.Position()?.Duration);
        if playing {
            // apps only publish the position now and then; extrapolate from the last update
            let updated = ticks(t.LastUpdatedTime()?.UniversalTime) - 11_644_473_600.0;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            if updated > 0.0 {
                pos += (now - updated).max(0.0);
            }
        }
        info.len = end.max(0.0);
        info.pos = if end > 0.0 { pos.min(end) } else { 0.0 };
    }
    if info.title.is_empty() {
        info.title = info.app.clone();
    }
    Ok(info)
}

fn pretty_app(aumid: &str) -> String {
    let id = aumid.rsplit('!').next().unwrap_or(aumid);
    let id = id.trim_end_matches(".exe").trim_end_matches(".EXE");
    let id = id.rsplit(['.', '\\']).next().unwrap_or(id);
    match id.to_lowercase().as_str() {
        "msedge" => "Microsoft Edge".into(),
        "chrome" => "Google Chrome".into(),
        "firefox" => "Firefox".into(),
        "spotify" => "Spotify".into(),
        "app" => "Apple Music".into(),
        _ => id.to_string(),
    }
}
