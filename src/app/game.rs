//! Game mode: finds the game in front, tracks play time per game, and shows
//! FPS / CPU / GPU / RAM / battery in a wider pill that stays over fullscreen games.

use super::screen::fmt_dur;
use super::{App, defer, gear, s, schedule_shrink};
use super::{schedule_collapse, with_app};
use crate::{Bridge, GameItem, fg, fps, games_scan, gpu, gpu_info, pad, sys};
use chrono::{Datelike, Duration as CDuration, Local};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

pub const TAB_GAME: i32 = 12;
/// first break nudge after this much play, then every hour
const FIRST_BREAK_S: u32 = 2 * 3600;
const FPS_HISTORY: usize = 40;

pub struct GameRun {
    pub key: String,
    pub pid: u32,
    pub name: String,
    pub played_s: u32,
    pub next_break_s: u32,
    /// the game is the window in front
    pub front: bool,
}

#[derive(Default)]
pub struct Play {
    pub run: Option<GameRun>,
    pub fps: Option<f32>,
    pub fps_src: &'static str,
    pub fps_hist: VecDeque<f32>,
    pub fps_at: Option<Instant>,
    pub pads: pad::Pads,
    pub pad: pad::PadState,
    pub pad_at: Option<Instant>,
    /// programs Windows itself flags as games
    pub known: HashSet<String>,
    pub known_at: Option<Instant>,
    /// (pid, fullscreen) last classified, so a non-game isn't re-checked every tick
    pub checked: (u32, bool),
    pub tab_before: i32,
    /// game mode was switched on by a game starting (so it switches off when it closes)
    pub auto_on: bool,
    pub power_set: bool,
    pub gpu_name: String,
    pub vram_total: f64,
    pub unsaved_s: u32,
    /// installed games were looked for this run (done once when the library opens)
    pub scanned: bool,
    /// put back on top once since the game came to the front (doing it every tick fights the game)
    pub topped: bool,
    pub scanning: bool,
    /// library search text
    pub query: String,
}

/// Sets a Bridge property only when it changed (each change can cost a redraw).
macro_rules! upd {
    ($b:expr, $get:ident, $set:ident, $v:expr) => {{
        let v = $v;
        if $b.$get() != v {
            $b.$set(v);
        }
    }};
}

impl App {
    pub fn game_in_front(&self) -> bool {
        self.play.run.as_ref().is_some_and(|r| r.front)
    }

    /// Controller input in the last minute (counts as being at the PC).
    pub fn pad_recent(&self) -> bool {
        self.play.pad_at.is_some_and(|t| t.elapsed() < Duration::from_secs(60))
    }

    /// Polls controllers while a game runs or game mode is on. Returns `pad_recent()`.
    pub fn poll_pad(&mut self) -> bool {
        if self.play.run.is_some() || self.data.settings.game_mode {
            self.play.pad = self.play.pads.poll();
            if self.play.pad.active {
                self.play.pad_at = Some(Instant::now());
            }
        }
        self.pad_recent()
    }

    pub fn game_tick(&mut self, front: Option<&fg::Fg>, dt: u32) {
        if self.play.known_at.is_none_or(|t| t.elapsed() > Duration::from_secs(600)) {
            self.play.known = fg::windows_known_games();
            self.play.known_at = Some(Instant::now());
        }
        if self.play.run.as_ref().is_some_and(|r| !fg::alive(r.pid)) {
            self.end_run(true);
        }
        let Some(f) = front else {
            if let Some(r) = &mut self.play.run {
                r.front = false;
            }
            return;
        };
        let in_front = self.play.run.as_ref().is_some_and(|r| r.pid == f.pid);
        if let Some(r) = &mut self.play.run {
            r.front = in_front;
        }
        if in_front {
            // AFK (no input and no controller for 10 min) doesn't count as playing
            if dt <= 90 && (sys::idle_seconds() < 600 || self.pad_recent()) {
                self.add_play(dt.min(10));
            }
            return;
        }
        if self.play.checked == (f.pid, f.fullscreen) {
            return;
        }
        self.play.checked = (f.pid, f.fullscreen);
        let Some(hit) = self.library_hit(f).or_else(|| fg::classify(f, &self.play.known)) else { return };
        if self.data.settings.game_ignore.contains(&hit.key) {
            return;
        }
        // same game, new process (it restarted itself)
        if let Some(r) = &mut self.play.run {
            if r.key == hit.key {
                r.pid = f.pid;
                r.front = true;
                fps::set_target(f.pid);
                return;
            }
        }
        if self.play.run.is_some() {
            self.end_run(false);
        }
        self.start_run(f, hit);
    }

    /// The program in front belongs to a game already in the library (found by a scan or added by hand).
    fn library_hit(&self, f: &fg::Fg) -> Option<fg::GameHit> {
        if fg::is_not_game(&f.exe) || f.is_lock() {
            return None;
        }
        let lower = f.path.to_lowercase();
        let (key, g) = self
            .data
            .games
            .iter()
            .find(|(k, _)| lower == **k || lower.strip_prefix(k.as_str()).is_some_and(|r| r.starts_with('\\')))?;
        let launch = if g.launch.is_empty() { f.path.clone() } else { g.launch.clone() };
        Some(fg::GameHit { key: key.clone(), name: g.name.clone(), launch })
    }

    fn start_run(&mut self, f: &fg::Fg, hit: fg::GameHit) {
        let g = self.data.games.entry(hit.key.clone()).or_default();
        if g.name.is_empty() {
            g.name = hit.name;
        }
        if g.launch.is_empty() || hit.launch.starts_with("steam://") && !g.launch.contains("://") {
            g.launch = hit.launch;
        }
        g.exe = f.path.clone();
        g.sessions += 1;
        g.last = Some(Local::now().naive_local());
        let (name, total, sessions) = (g.name.clone(), g.total_s, g.sessions);
        self.play.run = Some(GameRun { key: hit.key, pid: f.pid, name: name.clone(), played_s: 0, next_break_s: FIRST_BREAK_S, front: true });
        self.play.fps_hist.clear();
        self.play.fps = None;
        fps::set_target(f.pid);
        self.data.save();
        if self.data.settings.game_auto && !self.data.settings.game_mode {
            self.set_game_mode(true, true);
        }
        let sub = if total >= 60 { format!("{} played · session {sessions}", fmt_dur(total)) } else { "First session · have fun!".into() };
        self.peek(16, &format!("Playing {name}"), &sub, 3);
        self.refresh_game();
    }

    fn end_run(&mut self, exited: bool) {
        let Some(run) = self.play.run.take() else { return };
        fps::set_target(0);
        self.play.fps = None;
        self.play.checked = (0, false);
        self.data.save();
        if exited && run.played_s >= 60 {
            let total = self.data.games.get(&run.key).map(|g| g.total_s).unwrap_or(0);
            let sub = format!("This session {} · {} in total", fmt_dur(run.played_s), fmt_dur(total));
            self.peek(16, &format!("{} closed", run.name), &sub, 4);
        }
        if exited && self.play.auto_on {
            self.set_game_mode(false, true);
        }
        self.refresh_game();
    }

    fn add_play(&mut self, dt: u32) {
        let now = Local::now();
        let Some(run) = &mut self.play.run else { return };
        run.played_s += dt;
        if let Some(g) = self.data.games.get_mut(&run.key) {
            g.total_s += dt;
            *g.days.entry(now.date_naive()).or_default() += dt;
            g.last = Some(now.naive_local());
        }
        let due = self.data.settings.game_break && run.played_s >= run.next_break_s;
        if due {
            run.next_break_s += 3600;
        }
        let (name, played) = (run.name.clone(), run.played_s);
        if due {
            self.peek(16, &format!("{} of {name}", fmt_dur(played)), "Good moment to stretch, drink water and rest your eyes", 8);
        }
        self.play.unsaved_s += dt;
        if self.play.unsaved_s >= 120 {
            self.play.unsaved_s = 0;
            self.data.save();
        }
    }

    /// Turns game mode on or off. `auto` = because a game started or closed.
    pub fn set_game_mode(&mut self, on: bool, auto: bool) {
        if self.data.settings.game_mode == on {
            return;
        }
        self.data.settings.game_mode = on;
        self.play.auto_on = on && auto;
        self.data.save();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_game_mode(on);
        let (expanded, peeking) = (b.get_expanded(), b.get_peeking());
        if on {
            let tab = b.get_tab();
            if tab != TAB_GAME {
                self.play.tab_before = tab;
                b.set_tab_dir(1);
                b.set_tab(TAB_GAME);
            }
            b.set_eye_mood(0);
            self.eye_end = None;
            if self.play.gpu_name.is_empty() {
                (self.play.gpu_name, self.play.vram_total) = gpu_info::main_adapter();
            }
            sys::cpu_usage();
            gpu::usage(); // prime the rate counters
            fps::start();
            if self.data.settings.game_power {
                self.play.power_set = sys::set_power_mode(sys::PowerMode::Performance);
            }
            if !expanded && !peeking {
                self.resize(false); // grow before the pill widens
            }
        } else {
            if b.get_tab() == TAB_GAME {
                b.set_tab_dir(-1);
                b.set_tab(self.play.tab_before);
            }
            fps::stop();
            if std::mem::take(&mut self.play.power_set) {
                let mode = if self.data.settings.gear_power { gear::power_for(self.gear()) } else { sys::PowerMode::Balanced };
                sys::set_power_mode(mode);
            }
            if !expanded && !peeking {
                defer(|s| {
                    let mut a = s.borrow_mut();
                    schedule_shrink(s, &mut a); // shrink after the pill has narrowed
                });
            }
        }
        if !auto || expanded {
            let (title, sub) = if on { ("Game mode on", "Stats pill · stays over fullscreen games") } else { ("Game mode off", "Back to normal") };
            self.peek(16, title, sub, 2);
        }
        let tab = b.get_tab();
        self.refresh_tab(tab);
        self.refresh_game();
    }

    fn sample_fps(&mut self) {
        let dt = self.play.fps_at.map(|t| t.elapsed().as_secs_f32()).unwrap_or(2.0).max(0.2);
        self.play.fps_at = Some(Instant::now());
        let presents = fps::take();
        let fps = match &self.play.run {
            None => None,
            Some(r) => match fps::rtss(r.pid) {
                Some(v) => {
                    self.play.fps_src = "RTSS";
                    Some(v)
                }
                None if fps::state() == fps::RUNNING => {
                    self.play.fps_src = "Windows";
                    Some(presents as f32 / dt)
                }
                None => {
                    self.play.fps_src = "";
                    None
                }
            },
        };
        self.play.fps = fps;
        if let Some(v) = fps {
            self.play.fps_hist.push_back(v);
            while self.play.fps_hist.len() > FPS_HISTORY {
                self.play.fps_hist.pop_front();
            }
        }
    }

    /// SVG path for the FPS sparkline (viewbox 100 × 30).
    fn fps_path(&self) -> String {
        let h = &self.play.fps_hist;
        if h.len() < 2 {
            return String::new();
        }
        let max = h.iter().copied().fold(60.0f32, f32::max) * 1.1;
        let step = 100.0 / (FPS_HISTORY - 1) as f32;
        let x0 = 100.0 - step * (h.len() - 1) as f32;
        h.iter()
            .enumerate()
            .map(|(i, v)| format!("{}{:.1} {:.1}", if i == 0 { "M" } else { " L" }, x0 + i as f32 * step, 29.0 - v / max * 27.0))
            .collect()
    }

    /// Every 2 s while game mode is on: stats for the pill (and the Game tab when it's open).
    pub fn game_stats_tick(&mut self, front: Option<&fg::Fg>) {
        let cpu = sys::cpu_usage();
        let (ram, used, total) = sys::memory();
        let (gpu, vram) = gpu::usage();
        self.sample_fps();
        // a fullscreen game may push itself above us: step back on top once when it comes
        // to the front, not every tick (that steals the game's fullscreen)
        let game_full = front.is_some_and(|f| f.fullscreen) && self.game_in_front();
        if !game_full {
            self.play.topped = false;
        } else if !self.hidden && !self.play.topped && sys::fullscreen_state() != sys::FS_EXCLUSIVE {
            sys::keep_on_top(self.hwnd);
            self.play.topped = true;
        }
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let pct = |v: f32| format!("{:.0}%", v * 100.0);
        let (fps_text, fps_n) = match self.play.fps {
            Some(v) => (format!("{:.0}", v), v.round() as i32),
            None => ("–".to_string(), -1),
        };
        upd!(b, get_game_fps, set_game_fps, s(fps_text));
        upd!(b, get_game_fps_n, set_game_fps_n, fps_n);
        upd!(b, get_game_cpu, set_game_cpu, (cpu * 50.0).round() / 50.0);
        upd!(b, get_game_cpu_text, set_game_cpu_text, s(pct(cpu)));
        upd!(b, get_game_gpu, set_game_gpu, (gpu * 50.0).round() / 50.0);
        upd!(b, get_game_gpu_text, set_game_gpu_text, s(pct(gpu)));
        upd!(b, get_game_ram, set_game_ram, ram);
        upd!(b, get_game_ram_text, set_game_ram_text, s(pct(ram)));
        if !(b.get_expanded() && b.get_tab() == TAB_GAME) {
            return;
        }
        // Game tab only
        let vt = self.play.vram_total;
        b.set_game_vram(if vt > 0.0 { (vram / vt) as f32 } else { 0.0 });
        b.set_game_vram_text(s(if vt > 0.0 { pct((vram / vt) as f32) } else if vram > 0.0 { format!("{vram:.1}") } else { "–".into() }));
        b.set_game_vram_gb(s(if vt > 0.0 { format!("{vram:.1} / {vt:.0} GB") } else { "video memory".into() }));
        b.set_game_ram_gb(s(format!("{used:.1} / {total:.0} GB")));
        b.set_game_fps_path(s(self.fps_path()));
        b.set_game_fps_src(s(self.play.fps_src));
        b.set_game_fps_locked(self.play.run.is_some() && self.play.fps.is_none() && fps::state() == fps::DENIED);
        let p = self.play.pad;
        b.set_game_pad(if !p.connected { -1 } else if p.battery < 0 { 5 } else { p.battery });
        let (pct_b, plugged) = sys::battery();
        b.set_game_battery_text(s(if pct_b < 0 {
            "No battery".into()
        } else if plugged {
            "Plugged in".into()
        } else {
            match sys::battery_secs_left() {
                Some(sec) => format!("{} left", fmt_dur(sec)),
                None => "On battery".into(),
            }
        }));
        self.refresh_game_header();
    }

    fn refresh_game_header(&mut self) {
        let today = Local::now().date_naive();
        let (running, name, sub, exe) = match &self.play.run {
            Some(r) => {
                let g = self.data.games.get(&r.key);
                let today_s = g.and_then(|g| g.days.get(&today)).copied().unwrap_or(0);
                let state = if r.front { "Playing" } else { "In the background" };
                (true, r.name.clone(), format!("{state} · {} · today {}", fmt_dur(r.played_s), fmt_dur(today_s)), g.map(|g| g.exe.clone()).unwrap_or_default())
            }
            None => (false, "No game running".into(), "Start a game and it's detected automatically".into(), String::new()),
        };
        let icon = self.icon_for(&exe);
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        upd!(b, get_game_running, set_game_running, running);
        upd!(b, get_game_front, set_game_front, self.game_in_front());
        upd!(b, get_game_name, set_game_name, s(name));
        upd!(b, get_game_sub, set_game_sub, s(sub));
        b.set_game_icon(icon);
    }

    /// Everything on the Game tab that isn't a live number.
    pub fn refresh_game(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let st = &self.data.settings;
        b.set_game_mode(st.game_mode);
        b.set_game_auto(st.game_auto);
        b.set_game_power(st.game_power);
        b.set_game_break(st.game_break);
        b.set_game_gpu_name(s(self.play.gpu_name.clone()));
        if !st.game_mode {
            b.set_game_fps(s("–"));
            b.set_game_fps_n(-1);
        }
        self.refresh_game_header();
        if b.get_expanded() && b.get_tab() == TAB_GAME && b.get_game_view() == 1 {
            self.refresh_games();
        }
    }

    /// The library: every game played, most recent first.
    pub fn refresh_games(&mut self) {
        if !self.play.scanned {
            self.game_scan(false);
        }
        let now = Local::now();
        let today = now.date_naive();
        let week_start = today - CDuration::days(today.weekday().num_days_from_monday() as i64);
        let mut games: Vec<_> = self.data.games.iter().map(|(k, g)| (k.clone(), g.clone())).collect();
        games.sort_by(|a, b| b.1.last.cmp(&a.1.last).then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase())));
        let max = games.iter().map(|g| g.1.total_s).max().unwrap_or(1).max(1);
        let running = self.play.run.as_ref().map(|r| r.key.clone());
        let mut items = Vec::new();
        let (mut total, mut week) = (0u32, 0u32);
        let query = self.play.query.trim().to_lowercase();
        for (key, g) in &games {
            total += g.total_s;
            week += g.days.iter().filter(|(d, _)| **d >= week_start).map(|(_, v)| v).sum::<u32>();
            if !query.is_empty() && !g.name.to_lowercase().contains(&query) {
                continue;
            }
            let last = match g.last {
                Some(l) if l.date() == today => "today".to_string(),
                Some(l) if l.date() == today - CDuration::days(1) => "yesterday".to_string(),
                Some(l) => format!("{} days ago", (today - l.date()).num_days()),
                None => "never".to_string(),
            };
            let sessions = if g.sessions == 1 { "1 session".to_string() } else { format!("{} sessions", g.sessions) };
            let sub = match (g.sessions, g.source.as_str()) {
                (0, "") => "Not played yet".to_string(),
                (0, "Added") => "Added by you · not played yet".to_string(),
                (0, src) => format!("Installed · {src} · not played yet"),
                _ => format!("Last played {last} · {sessions}"),
            };
            items.push(GameItem {
                key: s(key.clone()),
                name: s(g.name.clone()),
                total: s(fmt_dur(g.total_s)),
                sub: s(sub),
                frac: g.total_s as f32 / max as f32,
                icon: self.icon_for(&g.exe),
                running: running.as_deref() == Some(key.as_str()),
            });
        }
        let top = games.iter().filter(|g| g.1.total_s > 0).max_by_key(|g| g.1.total_s).map(|g| g.1.name.clone()).unwrap_or_else(|| "–".into());
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_games(ModelRc::new(VecModel::from(items)));
        b.set_games_scanning(self.play.scanning);
        b.set_games_total(s(fmt_dur(total)));
        b.set_games_week(s(fmt_dur(week)));
        b.set_games_count(s(games.len().to_string()));
        b.set_games_top(s(top));
    }

    pub fn game_launch(&mut self, key: &str) {
        let Some(g) = self.data.games.get(key) else { return };
        let launch = g.launch.clone();
        let name = g.name.clone();
        if sys::open(&launch, "") {
            self.toast(&format!("Starting {name}…"));
        } else {
            self.toast("Couldn't start it");
        }
    }

    /// Looks for installed games off the UI thread and adds new ones to the library.
    /// `manual` = the Scan button (always reports the result).
    pub fn game_scan(&mut self, manual: bool) {
        if self.play.scanning {
            return;
        }
        self.play.scanned = true;
        self.play.scanning = true;
        self.ui().global::<Bridge>().set_games_scanning(true);
        std::thread::spawn(move || {
            let found = games_scan::installed();
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|s| {
                    let mut a = s.borrow_mut();
                    a.play.scanning = false;
                    let n = a.games_merge(found);
                    if n > 0 {
                        a.toast(&format!("Found {n} new game{}", if n == 1 { "" } else { "s" }));
                    } else if manual {
                        a.toast("No new games found");
                    }
                    a.refresh_games();
                })
            });
        });
    }

    /// Adds scanned games that aren't in the library yet. Returns how many are new.
    fn games_merge(&mut self, found: Vec<games_scan::Found>) -> usize {
        let mut added = 0;
        for f in found {
            if self.data.settings.game_ignore.contains(&f.key) {
                continue;
            }
            match self.data.games.get_mut(&f.key) {
                Some(g) => {
                    if g.exe.is_empty() {
                        g.exe = f.exe;
                    }
                    if g.launch.is_empty() || !g.launch.contains("://") && f.launch.contains("://") {
                        g.launch = f.launch;
                    }
                }
                None => {
                    let g = crate::store::Game { name: f.name, exe: f.exe, launch: f.launch, source: f.source.into(), ..Default::default() };
                    self.data.games.insert(f.key, g);
                    added += 1;
                }
            }
        }
        self.data.save();
        added
    }

    /// Adds games picked by hand (programs or shortcuts). Returns how many are new.
    pub fn games_add(&mut self, paths: Vec<std::path::PathBuf>) -> usize {
        let mut added = 0;
        for p in paths {
            let path = p.to_string_lossy().to_string();
            let key = fg::game_key(&path);
            self.data.settings.game_ignore.retain(|k| *k != key);
            if self.data.games.contains_key(&key) {
                continue;
            }
            let is_exe = path.to_lowercase().ends_with(".exe");
            let name = if is_exe { fg::app_name(&path) } else { fg::clean(&fg::stem_name(&path)) };
            let exe = if is_exe { path.clone() } else { String::new() };
            let g = crate::store::Game { name, exe, launch: path, source: "Added".into(), ..Default::default() };
            self.data.games.insert(key, g);
            added += 1;
        }
        self.data.save();
        self.refresh_games();
        added
    }

    pub fn games_search(&mut self, q: &str) {
        self.play.query = q.to_string();
        self.refresh_games();
    }

    /// Removes a game from the library and stops detecting it.
    pub fn game_forget(&mut self, key: &str) {
        if self.play.run.as_ref().is_some_and(|r| r.key == key) {
            self.play.run = None;
            fps::set_target(0);
        }
        self.data.games.remove(key);
        if !self.data.settings.game_ignore.iter().any(|k| k == key) {
            self.data.settings.game_ignore.push(key.to_string());
        }
        self.data.save();
        self.refresh_games();
        self.refresh_game();
    }

    /// "Not a game" on the running one.
    pub fn game_not_game(&mut self) {
        if let Some(key) = self.play.run.as_ref().map(|r| r.key.clone()) {
            let auto = self.play.auto_on;
            self.game_forget(&key);
            self.toast("Got it, not a game");
            if auto {
                self.set_game_mode(false, true);
            }
        }
    }

    pub fn game_allow_fps(&mut self) {
        if fps::request_permission() {
            self.toast("Approve it, then sign out and back in once");
        } else {
            self.toast("Permission not granted");
        }
    }
}

/// "Add game": pick programs. The dialog is modal, so the app isn't borrowed meanwhile.
pub fn games_pick(shared: &super::Shared) {
    let hwnd = {
        let mut a = shared.borrow_mut();
        a.hovering = true;
        a.collapse_timer.stop();
        a.hwnd
    };
    let picked = sys::pick_paths_titled(hwnd, false, "Add games (pick the game's program or shortcut)");
    let mut a = shared.borrow_mut();
    let n = if picked.is_empty() { 0 } else { a.games_add(picked) };
    if n > 0 {
        a.toast(&format!("Added {n} game{}", if n == 1 { "" } else { "s" }));
    }
    a.hovering = false;
    schedule_collapse(shared, &mut a, 3000);
}
