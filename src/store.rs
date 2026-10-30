//! Persistent user data, stored as JSON in %APPDATA%\Winklet\data.json.

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Todo {
    pub id: String,
    pub text: String,
    pub done: bool,
    /// Day the task belongs to (calendar). None = inbox.
    pub date: Option<NaiveDate>,
    /// Optional reminder time on `date`.
    pub time: Option<NaiveTime>,
    #[serde(default)]
    pub notified: bool,
}

impl Todo {
    pub fn due(&self) -> Option<NaiveDateTime> {
        Some(self.date?.and_time(self.time?))
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Alarm {
    pub id: String,
    pub time: NaiveTime,
    pub label: String,
    pub enabled: bool,
    #[serde(default)]
    pub last_fired: Option<NaiveDate>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub autostart: bool,
    pub clock_24h: bool,
    pub hide_fullscreen: bool,
    pub water_reminders: bool,
    pub water_interval_min: i32,
    pub water_goal_ml: i32,
    pub cup_ml: i32,
    pub focus_min: i32,
    pub break_min: i32,
    /// folder the "4 terminals" button opens
    pub workspace_dir: String,
    /// drive + drift animation under the island when it opens
    pub car_show: bool,
    /// 0 JDM, 1 Muscle, 2 German, 3 F1
    pub car_model: i32,
    /// random eye events (look around, wink, music, ...) instead of blink-only
    pub eyes_lively: bool,
    /// where the island's top centre sits (physical px), after a right-drag; None = top centre
    pub island_pos: Option<(i32, i32)>,
    // ---- wellbeing ----
    pub stand_reminders: bool,
    pub stand_interval_min: i32,
    /// 20-20-20 eye rest
    pub eye_rest: bool,
    /// wait until the user pauses typing before popping a reminder
    pub wait_for_pause: bool,
    /// hold reminders while another app uses the mic (calls) or something is fullscreen
    pub pause_in_calls: bool,
    pub work_hours_only: bool,
    pub work_start_h: u32,
    pub work_end_h: u32,
    /// last Lenovo Fn+Q mode seen (1 quiet, 2 auto, 3 performance); 0 = never seen
    pub perf_mode: i32,
    /// drive mode: 0 = N (away), 1 rest … 6 redline (deep work)
    pub gear: i32,
    /// shifting also switches the Windows power mode (efficiency / balanced / performance)
    pub gear_power: bool,
    // ---- game mode ----
    /// stats pill + Game tab as the default tab; stays visible over fullscreen games
    pub game_mode: bool,
    /// turn game mode on when a game starts and off when it closes
    pub game_auto: bool,
    /// Windows power mode "Best performance" while game mode is on
    pub game_power: bool,
    /// a gentle stretch/water nudge after 2 h of play, then every hour
    pub game_break: bool,
    /// programs marked "not a game" (game keys)
    pub game_ignore: Vec<String>,
    // ---- day summary ----
    pub summary_on: bool,
    /// hour the end-of-day summary pops up
    pub summary_h: u32,
    /// bumped when defaults change; 0 = file from before versioning
    #[serde(default)]
    pub version: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autostart: false,
            clock_24h: false,
            hide_fullscreen: true,
            water_reminders: true,
            water_interval_min: 60,
            water_goal_ml: 2500,
            cup_ml: 250,
            focus_min: 25,
            break_min: 5,
            // your home folder until you pick another (right-click "4 Terminals")
            workspace_dir: std::env::var("USERPROFILE").unwrap_or_default(),
            car_show: true,
            car_model: 0,
            eyes_lively: true,
            island_pos: None,
            stand_reminders: true,
            stand_interval_min: 50,
            eye_rest: false,
            wait_for_pause: true,
            pause_in_calls: true,
            work_hours_only: true,
            work_start_h: 8,
            work_end_h: 22,
            perf_mode: 0,
            gear: 3,
            gear_power: true,
            game_mode: false,
            game_auto: false,
            game_power: true,
            game_break: true,
            game_ignore: Vec::new(),
            summary_on: true,
            summary_h: 18,
            version: SETTINGS_VERSION,
        }
    }
}

const SETTINGS_VERSION: u32 = 2;

/// Screen-activity stats for one day. Only activity timing is recorded, never input content.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct DayHealth {
    pub active_s: u32,
    pub breaks: u32,
    pub longest_s: u32,
    /// active seconds per hour of the day (24 entries)
    pub hours: Vec<u32>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ShelfItem {
    pub id: String,
    pub path: String,
}

/// Developer → Workspaces: apps, files, folders and links that open together.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub apps: Vec<WsApp>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct WsApp {
    pub name: String,
    /// program, file, folder or https:// link
    pub path: String,
}

/// A saved playlist link (Home media card).
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub url: String,
}

/// One game in the library. Play time counts while the game is in front.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Game {
    pub name: String,
    /// last program seen for it (icon, and launch fallback)
    pub exe: String,
    /// steam://rungameid/… or the program path
    pub launch: String,
    pub total_s: u32,
    pub sessions: u32,
    pub last: Option<NaiveDateTime>,
    /// seconds played per day (last 60 days)
    pub days: BTreeMap<NaiveDate, u32>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Data {
    pub todos: Vec<Todo>,
    pub alarms: Vec<Alarm>,
    /// ml drunk per day
    pub water: BTreeMap<NaiveDate, i32>,
    /// individual sips of today, so they can be undone
    pub water_log: Vec<i32>,
    pub focus_sessions: BTreeMap<NaiveDate, i32>,
    /// shortcuts to files/folders (the files themselves are never moved)
    pub shelf: Vec<ShelfItem>,
    pub health: BTreeMap<NaiveDate, DayHealth>,
    /// active seconds per gear per day (index 0 = N, 1..6)
    pub gear_time: BTreeMap<NaiveDate, Vec<u32>>,
    /// games seen, keyed by game folder (or program path)
    pub games: BTreeMap<String, Game>,
    /// tasks ticked off per day (kept even after "clear done")
    pub tasks_done: BTreeMap<NaiveDate, i32>,
    pub workspaces: Vec<Workspace>,
    /// saved playlist links, most recently played first
    pub playlists: Vec<Playlist>,
    /// last day the end-of-day summary popped up
    pub summary_day: Option<NaiveDate>,
    pub settings: Settings,
}

/// How long each app was in front, per day. Only the program is recorded (never window
/// titles or input). Kept in its own compact file, screen.json, so data.json stays small.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct AppUse {
    pub name: String,
    pub path: String,
    pub secs: u32,
    /// times it was in use: [start, end] in seconds since midnight
    pub spans: Vec<[u32; 2]>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Screen {
    /// day -> app (lower-case file name, e.g. "chrome.exe") -> use
    pub days: BTreeMap<NaiveDate, BTreeMap<String, AppUse>>,
}

impl Screen {
    pub fn load() -> Self {
        let mut s: Self = std::fs::read(file("screen.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if let Some(cutoff) = Local::now().date_naive().checked_sub_days(chrono::Days::new(60)) {
            s.days.retain(|d, _| *d >= cutoff);
        }
        s
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_vec(self) {
            write_atomic(&file("screen.json"), &json);
        }
    }
}

fn file(name: &str) -> PathBuf {
    path().with_file_name(name)
}

/// write-then-rename so a crash never leaves a half-written file
fn write_atomic(p: &std::path::Path, bytes: &[u8]) {
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, p);
    }
}

fn path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let new = base.join("Winklet").join("data.json");
    // first run after the rename: bring the old DyIs data along (the old file stays as a backup)
    let old = base.join("DyIs").join("data.json");
    if !new.exists() && old.exists() {
        let _ = std::fs::create_dir_all(new.parent().unwrap());
        let _ = std::fs::copy(&old, &new);
    }
    new
}

impl Data {
    pub fn load() -> Self {
        let mut data: Self = std::fs::read(path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if data.settings.version < 2 {
            data.settings.clock_24h = false; // 12-hour clock became the default
        }
        data.settings.version = SETTINGS_VERSION;
        data.settings.gear = data.settings.gear.clamp(0, 6);
        // keep two months of wellbeing history
        if let Some(cutoff) = Local::now().date_naive().checked_sub_days(chrono::Days::new(60)) {
            data.health.retain(|d, _| *d >= cutoff);
            data.gear_time.retain(|d, _| *d >= cutoff);
            data.tasks_done.retain(|d, _| *d >= cutoff);
            for g in data.games.values_mut() {
                g.days.retain(|d, _| *d >= cutoff);
            }
        }
        data
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            write_atomic(&path(), &json);
        }
    }

    pub fn water_today(&self) -> i32 {
        *self.water.get(&Local::now().date_naive()).unwrap_or(&0)
    }

    pub fn sessions_today(&self) -> i32 {
        *self.focus_sessions.get(&Local::now().date_naive()).unwrap_or(&0)
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Parses "Buy milk @18:30" / "Deploy @2026-11-02 09:00" / "Gym @tomorrow 07:00".
/// Returns (clean text, date, time).
pub fn parse_task(input: &str, default_date: Option<NaiveDate>) -> (String, Option<NaiveDate>, Option<NaiveTime>) {
    let Some(at) = input.rfind('@') else {
        return (input.trim().to_string(), default_date, None);
    };
    let text = input[..at].trim().to_string();
    let spec = input[at + 1..].trim().to_lowercase();
    let today = Local::now().date_naive();
    let mut date = default_date;
    let mut time = None;
    for part in spec.split_whitespace() {
        if part == "today" {
            date = Some(today);
        } else if part == "tomorrow" || part == "tmr" {
            date = Some(today.succ_opt().unwrap_or(today));
        } else if let Ok(d) = NaiveDate::parse_from_str(part, "%Y-%m-%d") {
            date = Some(d);
        } else if let Some(t) = parse_time(part) {
            time = Some(t);
        }
    }
    if time.is_some() && date.is_none() {
        // "@18:30" without a day: today, or tomorrow if that time already passed
        let now = Local::now().naive_local();
        let t = time.unwrap();
        date = Some(if today.and_time(t) > now { today } else { today.succ_opt().unwrap_or(today) });
    }
    let text = if text.is_empty() { input.trim().to_string() } else { text };
    (text, date, time)
}

pub fn parse_time(s: &str) -> Option<NaiveTime> {
    let s = s.trim().to_lowercase();
    let (s, pm, am) = if let Some(x) = s.strip_suffix("pm") {
        (x.trim().to_string(), true, false)
    } else if let Some(x) = s.strip_suffix("am") {
        (x.trim().to_string(), false, true)
    } else {
        (s, false, false)
    };
    let (h, m) = match s.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (s.parse::<u32>().ok()?, 0),
    };
    let h = match (pm, am, h) {
        (true, _, h) if h < 12 => h + 12,
        (_, true, 12) => 0,
        _ => h,
    };
    NaiveTime::from_hms_opt(h, m, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times() {
        assert_eq!(parse_time("7:05"), NaiveTime::from_hms_opt(7, 5, 0));
        assert_eq!(parse_time("7pm"), NaiveTime::from_hms_opt(19, 0, 0));
        assert_eq!(parse_time("12am"), NaiveTime::from_hms_opt(0, 0, 0));
        assert_eq!(parse_time("25:00"), None);
    }

    #[test]
    fn parses_task() {
        let (t, d, tm) = parse_task("Ship it @2030-01-02 9:30", None);
        assert_eq!(t, "Ship it");
        assert_eq!(d, NaiveDate::from_ymd_opt(2030, 1, 2));
        assert_eq!(tm, NaiveTime::from_hms_opt(9, 30, 0));
        let (t, d, _) = parse_task("plain", None);
        assert_eq!((t.as_str(), d), ("plain", None));
    }
}
