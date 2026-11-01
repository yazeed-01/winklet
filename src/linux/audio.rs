//! Sound through PulseAudio (or PipeWire's PulseAudio server, the default on current distros):
//! master volume, per-app volume (sink inputs) and the microphone, using the `pactl` command.
//! All calls are made from the UI thread and only when the Audio tab needs them.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, PartialEq)]
pub struct AppVolume {
    pub name: String,
    pub volume: f32,
    pub muted: bool,
}

pub struct Audio;

fn pactl(args: &[&str]) -> Option<String> {
    let out = Command::new("pactl").args(args).env("LC_ALL", "C").stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The first "NN%" in a volume line, as 0..1.
fn percent(text: &str) -> Option<f32> {
    let i = text.find('%')?;
    let digits: String = text[..i].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse::<f32>().ok().map(|p| p / 100.0)
}

fn yes(text: &str) -> bool {
    text.split(':').nth(1).is_some_and(|v| v.trim() == "yes")
}

/// Blocks of `pactl list <what>`, one per device or stream.
fn blocks(what: &str) -> Vec<Vec<String>> {
    let text = pactl(&["list", what]).unwrap_or_default();
    let mut out: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) && !line.is_empty() {
            out.push(vec![line.to_string()]);
        } else if let Some(b) = out.last_mut() {
            b.push(line.trim().to_string());
        }
    }
    out
}

fn field<'a>(block: &'a [String], key: &str) -> Option<&'a str> {
    block.iter().find_map(|l| l.strip_prefix(key)).map(|v| v.trim())
}

/// A property line: application.name = "Firefox"
fn prop<'a>(block: &'a [String], key: &str) -> Option<&'a str> {
    block.iter().find_map(|l| l.strip_prefix(key)?.trim_start().strip_prefix('=')).map(|v| v.trim().trim_matches('"'))
}

struct Stream {
    id: String,
    name: String,
    volume: f32,
    muted: bool,
}

fn streams() -> Vec<Stream> {
    blocks("sink-inputs")
        .iter()
        .filter_map(|b| {
            let id = b[0].rsplit('#').next()?.trim().to_string();
            let name = prop(b, "application.name").or_else(|| prop(b, "application.process.binary")).unwrap_or("Unknown").to_string();
            Some(Stream { id, name, volume: field(b, "Volume:").and_then(percent).unwrap_or(1.0), muted: field(b, "Mute:").is_some_and(|m| m == "yes") })
        })
        .collect()
}

impl Audio {
    pub fn new() -> Option<Self> {
        pactl(&["info"]).map(|_| Self)
    }

    // ---------------------------------------------------------------- master
    pub fn master(&self) -> (f32, bool) {
        let v = pactl(&["get-sink-volume", "@DEFAULT_SINK@"]).and_then(|t| percent(&t)).unwrap_or(0.0);
        let m = pactl(&["get-sink-mute", "@DEFAULT_SINK@"]).is_some_and(|t| yes(&t));
        (v.min(1.0), m)
    }

    pub fn set_master(&self, v: f32) {
        let pct = format!("{}%", (v.clamp(0.0, 1.0) * 100.0).round() as i32);
        let _ = pactl(&["set-sink-volume", "@DEFAULT_SINK@", &pct]);
        if v > 0.0 {
            let _ = pactl(&["set-sink-mute", "@DEFAULT_SINK@", "0"]);
        }
    }

    pub fn toggle_master_mute(&self) {
        let _ = pactl(&["set-sink-mute", "@DEFAULT_SINK@", "toggle"]);
    }

    // ---------------------------------------------------------------- per app
    /// One row per app (a browser's many streams collapse into one), sorted by name.
    pub fn apps(&self) -> Vec<AppVolume> {
        let mut apps: Vec<AppVolume> = Vec::new();
        for s in streams() {
            if !apps.iter().any(|a| a.name == s.name) {
                apps.push(AppVolume { name: s.name, volume: s.volume.min(1.0), muted: s.muted });
            }
        }
        apps.sort_by(|a, b| a.name.cmp(&b.name));
        apps
    }

    pub fn set_app_volume(&self, name: &str, v: f32) {
        let pct = format!("{}%", (v.clamp(0.0, 1.0) * 100.0).round() as i32);
        for s in streams().into_iter().filter(|s| s.name == name) {
            let _ = pactl(&["set-sink-input-volume", &s.id, &pct]);
            if v > 0.0 {
                let _ = pactl(&["set-sink-input-mute", &s.id, "0"]);
            }
        }
    }

    pub fn toggle_app_mute(&self, name: &str) {
        let all: Vec<Stream> = streams().into_iter().filter(|s| s.name == name).collect();
        let Some(first) = all.first() else { return };
        let mute = if first.muted { "0" } else { "1" };
        for s in &all {
            let _ = pactl(&["set-sink-input-mute", &s.id, mute]);
        }
    }

    // ---------------------------------------------------------------- microphone
    /// None = no microphone
    pub fn mic_muted(&self) -> Option<bool> {
        pactl(&["get-source-mute", "@DEFAULT_SOURCE@"]).map(|t| yes(&t))
    }

    pub fn set_mic_muted(&self, mute: bool) {
        let _ = pactl(&["set-source-mute", "@DEFAULT_SOURCE@", if mute { "1" } else { "0" }]);
    }

    pub fn mic_name(&self) -> String {
        self.mics().into_iter().find(|m| m.2).map(|m| m.1).unwrap_or_else(|| "No microphone".into())
    }

    /// True while some app is recording from a microphone (a call, a recording...).
    pub fn mic_in_use(&self) -> bool {
        pactl(&["list", "short", "source-outputs"]).is_some_and(|t| t.lines().any(|l| !l.trim().is_empty()))
    }

    /// Every microphone: (device name, description, is the default). Speaker monitors are left out.
    pub fn mics(&self) -> Vec<(String, String, bool)> {
        let default = pactl(&["get-default-source"]).map(|s| s.trim().to_string()).unwrap_or_default();
        blocks("sources")
            .iter()
            .filter_map(|b| {
                let name = field(b, "Name:")?.to_string();
                if name.ends_with(".monitor") || prop(b, "device.class") == Some("monitor") {
                    return None;
                }
                let desc = field(b, "Description:").unwrap_or("Microphone").to_string();
                let on = name == default;
                Some((name, desc, on))
            })
            .collect()
    }

    /// Makes `id` the default microphone for every app.
    pub fn set_default_mic(&self, id: &str) -> bool {
        pactl(&["set-default-source", id]).is_some()
    }

    pub fn start_mic_test(&self) -> Option<MicTest> {
        MicTest::start()
    }
}

/// Records from the default microphone (`parec`) and measures the input level from the samples.
pub struct MicTest {
    child: Child,
    /// peak since the last read, as f32 bits
    peak: Arc<AtomicU32>,
    /// true once any non-silent sample arrived
    pub heard_anything: bool,
}

impl MicTest {
    fn start() -> Option<Self> {
        let mut child = Command::new("parec")
            .args(["--format=s16le", "--channels=1", "--rate=16000", "--latency-msec=50", "--client-name=Winklet"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = child.stdout.take()?;
        let peak = Arc::new(AtomicU32::new(0));
        let p = peak.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1600];
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let max = buf[..n & !1].chunks_exact(2).map(|s| (i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0).abs()).fold(0.0f32, f32::max);
                let _ = p.try_update(Ordering::Relaxed, Ordering::Relaxed, |old| Some(f32::from_bits(old).max(max).to_bits()));
            }
        });
        Some(Self { child, peak, heard_anything: false })
    }

    /// Peak level 0..1 since the last call.
    pub fn level(&mut self) -> f32 {
        self.levels().1
    }

    /// (meter, peak of the captured samples): both the same here.
    pub fn levels(&mut self) -> (f32, f32) {
        let p = f32::from_bits(self.peak.swap(0, Ordering::Relaxed)).min(1.0);
        if p > 0.0005 {
            self.heard_anything = true;
        }
        (p, p)
    }
}

impl Drop for MicTest {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
