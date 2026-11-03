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
    /// good morning / evening, welcome back and little lines through the day
    pub greetings: bool,
    /// where the island's centre sits (physical px) after it was moved; None = top centre
    pub island_pos: Option<(i32, i32)>,
    /// every launch puts the island back at the top centre
    pub island_home_on_start: bool,
    /// colour theme (0 = Classic)
    pub theme: i32,
    /// size of the panel and the pill, on top of the Windows display scale (0.8 .. 1.4)
    pub ui_scale: f32,
    /// opacity of the island's background (0.4 .. 1); text and icons stay solid
    pub island_alpha: f32,
    /// colour of the eyes (0 = Ice, the original white)
    pub eye_color: i32,
    /// island style: a floating ball with just the eyes instead of the notch
    pub bubble: bool,
    /// light mode: the open panel in soft, low-glare light colours (the pill stays dark)
    pub light: bool,
    /// Wallpaper tab: your own picture folders (Windows' wallpapers are always there)
    pub wallpaper_dirs: Vec<String>,
    /// the folder shown: 0 = Windows' wallpapers, then wallpaper_dirs
    pub wallpaper_source: i32,
    /// change the wallpaper to a random one from that folder every N minutes (0 = off)
    pub wallpaper_every: i32,
    /// how the pictures are shown: 0 = grid, 1 = carousel (the default; the last one picked sticks)
    pub wallpaper_view: i32,
    /// the More button in the tab bar
    pub more_on: bool,
    /// tabs (by index) that wait behind More instead of in the bar
    pub more_tabs: Vec<i32>,
    /// Clipboard tab: stop saving copies for now
    pub clip_paused: bool,
    /// Dev tools folders; empty = found automatically
    pub projects_dir: String,
    pub downloads_dir: String,
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
    /// Deck > Phone: serve the Deck to the Winklet phone app
    pub phone_deck: bool,
    /// the phone deck's secret (kept so a paired phone reconnects; "New link" replaces it)
    pub phone_deck_token: String,
    /// the key every phone-link message is sealed with; only ever shown in the QR code
    pub phone_deck_key: String,
    /// the port it last used, tried first so the phone finds it again
    pub phone_deck_port: u16,
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
            workspace_dir: std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default(),
            car_show: true,
            car_model: 0,
            eyes_lively: true,
            greetings: true,
            island_pos: None,
            island_home_on_start: true,
            theme: 0,
            ui_scale: 1.0,
            island_alpha: 1.0,
            eye_color: 0,
            bubble: false,
            light: false,
            wallpaper_dirs: Vec::new(),
            wallpaper_source: 0,
            wallpaper_every: 0,
            wallpaper_view: 1,
            more_on: false,
            more_tabs: Vec::new(),
            clip_paused: false,
            projects_dir: String::new(),
            downloads_dir: String::new(),
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
            phone_deck: false,
            phone_deck_token: String::new(),
            phone_deck_key: String::new(),
            phone_deck_port: 0,
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

/// One key on the Deck tab.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct DeckKey {
    pub id: String,
    pub page: i32,
    /// position on its page, row by row
    pub slot: i32,
    /// "open", "multi", "command", "script", "hotkey", "text" or "action"
    pub kind: String,
    /// open: program, file, folder or link · multi: one of those (or "> command") per line ·
    /// command: a command line · script: the script's lines · hotkey: "ctrl+shift+esc" ·
    /// text: what to type · action: its id
    pub target: String,
    pub title: String,
    /// Segoe Fluent glyph; "" = the program's own icon
    pub glyph: String,
    /// key colour, "#rrggbb"
    pub color: String,
    /// command, script: keep a window open with the output · text: press Enter after typing
    pub flag: bool,
    /// script: "powershell" (the default) or "batch"
    pub lang: String,
}

/// A kind of drink on the Water tab.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct DrinkType {
    pub id: String,
    pub name: String,
    /// "#rrggbb"
    pub color: String,
    /// usual cup size
    pub ml: i32,
}

/// The Deck keys everyone starts with: a few apps, media keys and handy actions.
pub fn default_deck() -> Vec<DeckKey> {
    let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let key = |slot: i32, kind: &str, target: String, title: &str, glyph: &str, color: &str| DeckKey {
        id: new_id(),
        page: 0,
        slot,
        kind: kind.into(),
        target,
        title: title.into(),
        glyph: glyph.into(),
        color: color.into(),
        flag: false,
        lang: String::new(),
    };
    vec![
        key(0, "open", format!(r"{win}\explorer.exe"), "Explorer", "", "#2563eb"),
        key(1, "open", "wt.exe".into(), "Terminal", "\u{E756}", "#2b2b33"),
        key(2, "open", format!(r"{win}\System32\calc.exe"), "Calculator", "", "#475569"),
        key(3, "open", format!(r"{win}\System32\Taskmgr.exe"), "Task Manager", "", "#059669"),
        key(4, "open", "ms-settings:".into(), "Settings", "\u{E713}", "#4f46e5"),
        key(5, "hotkey", "win+d".into(), "Desktop", "\u{E7F4}", "#0891b2"),
        key(8, "action", "prev".into(), "Previous", "\u{E892}", "#9333ea"),
        key(9, "action", "playpause".into(), "Play / Pause", "\u{E768}", "#9333ea"),
        key(10, "action", "next".into(), "Next", "\u{E893}", "#9333ea"),
        key(11, "action", "mute".into(), "Mute", "\u{E74F}", "#db2777"),
        key(12, "action", "mic".into(), "Mic", "\u{E720}", "#db2777"),
        key(16, "action", "screenshot".into(), "Screenshot", "\u{E722}", "#ea580c"),
        key(17, "action", "focus".into(), "Focus", "\u{E916}", "#ca8a04"),
        key(18, "action", "lock".into(), "Lock", "\u{E72E}", "#dc2626"),
    ]
}

/// The drinks everyone starts with (they can be edited or removed, except water).
pub fn default_drinks() -> Vec<DrinkType> {
    [
        ("water", "Water", "#38bdf8", 250),
        ("tea", "Tea", "#d97706", 200),
        ("green-tea", "Green tea", "#a3e635", 200),
        ("coffee", "Coffee", "#b07a4f", 200),
        ("espresso", "Espresso", "#8b5a3c", 40),
        ("iced-latte", "Iced latte", "#e8c9a0", 350),
        ("spanish-latte", "Spanish latte", "#f2b880", 300),
        ("matcha", "Matcha latte", "#4ade80", 300),
        ("juice", "Juice", "#fb923c", 250),
        ("milk", "Milk", "#e5e7eb", 250),
        ("sparkling", "Sparkling", "#67e8f9", 330),
        ("hot-chocolate", "Hot chocolate", "#a0522d", 250),
    ]
    .into_iter()
    .map(|(id, name, color, ml)| DrinkType { id: id.into(), name: name.into(), color: color.into(), ml })
    .collect()
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
    /// where it came from when found before being played: "Steam", "Epic", "GOG", "Added"
    pub source: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Data {
    pub todos: Vec<Todo>,
    pub alarms: Vec<Alarm>,
    /// ml drunk per day
    pub water: BTreeMap<NaiveDate, i32>,
    /// individual sips of today, so they can be undone (before drinks existed)
    pub water_log: Vec<i32>,
    /// the drinks on the Water tab
    pub drinks: Vec<DrinkType>,
    /// ml per drink per day (the day's total stays in `water`)
    pub drink_days: BTreeMap<NaiveDate, BTreeMap<String, i32>>,
    /// today's drinks, newest last, so they can be undone
    pub drink_undo: Vec<(NaiveDate, String, i32)>,
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
    /// Deck tab: the keys on every page
    pub deck: Vec<DeckKey>,
    /// how many Deck pages there are (at least 1)
    pub deck_pages: i32,
    /// the starter keys were put in once, so a deck you emptied stays empty
    pub deck_seeded: bool,
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

/// Folder holding everything Winklet saves (%APPDATA%\\Winklet).
pub fn data_dir() -> PathBuf {
    path().parent().map(PathBuf::from).unwrap_or_default()
}

/// Deletes everything Winklet saved (data.json, screen.json and their temp files).
pub fn delete_all() {
    for name in ["data.json", "screen.json", "data.tmp", "screen.tmp"] {
        let _ = std::fs::remove_file(file(name));
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
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    // ~/.config/Winklet
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
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
        if data.drinks.is_empty() {
            data.drinks = default_drinks();
        }
        if !data.deck_seeded {
            data.deck = default_deck();
            data.deck_seeded = true;
        }
        data.deck_pages = data.deck_pages.max(1);
        // keep two months of wellbeing history
        if let Some(cutoff) = Local::now().date_naive().checked_sub_days(chrono::Days::new(60)) {
            data.health.retain(|d, _| *d >= cutoff);
            data.gear_time.retain(|d, _| *d >= cutoff);
            data.drink_days.retain(|d, _| *d >= cutoff);
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
