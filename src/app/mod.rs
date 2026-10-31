//! The island's shared state (`App`) and the core moves between collapsed,
//! peeking and expanded. Each feature adds its own `impl App` in a submodule.

mod car;
mod dev;
mod drag;
mod drinks;
mod eyes;
mod focus;
mod game;
mod gear;
mod heartbeat;
mod history;
mod now_playing;
mod planner;
mod playlists;
mod refresh;
mod screen;
mod send;
mod shelf;
mod sound;
mod summary;
mod tools;
mod wellbeing;
mod workspaces;

pub use car::{CAR_PUFFS, CAR_SKIDS};
pub use focus::Focus;
pub use game::{Play, TAB_GAME, games_pick};
pub use send::send_pick;
pub use screen::ScreenTrack;
pub use tools::{folder_for, open_folder, pick_folder, pick_workspace, take_screenshot};
pub use wellbeing::Wellbeing;
pub use workspaces::ws_browse;

use crate::{AppVolume, AppWindow, Bridge, Puff, Skid, audio, media, store::Data, sys};
use car::CarShow;
use chrono::{Local, NaiveDate};
use slint::{ComponentHandle, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;

/// Window sizes in logical px. The window hugs the pill when collapsed so the
/// transparent area around it never steals clicks from apps underneath.
pub const SMALL: (f32, f32) = (340.0, 44.0);
/// game mode's pill is wider (time, FPS, CPU, eyes, GPU, RAM, battery)
const SMALL_GAME: (f32, f32) = (444.0, 44.0);
const BIG: (f32, f32) = (620.0, 364.0);
/// the pill docked on the left or right edge stands upright
const VERT: (f32, f32) = (52.0, 180.0);
const BIG_CAR: (f32, f32) = (620.0, 484.0);

pub const TAB_HEALTH: i32 = 4;
pub const TAB_SHELF: i32 = 7;
pub const TAB_AUDIO: i32 = 8;
pub const TAB_DEV: i32 = 9;
/// drive modes; drawn after Focus in the tab bar but numbered last so older tabs keep their index
pub const TAB_GEAR: i32 = 11;

pub struct App {
    pub ui: slint::Weak<AppWindow>,
    pub hwnd: HWND,
    /// window that had focus before the island opened
    pub prev_fg: HWND,
    pub data: Data,
    pub hovering: bool,
    pub hidden: bool,
    pub focus: Focus,
    pub cal_month: NaiveDate,
    pub cal_selected: NaiveDate,
    pub last_drink: Instant,
    pub plugged: Option<bool>,
    pub collapsed_at: Option<Instant>,
    pub trimmed: bool,
    pub last_track: String,
    pub last_minute: u32,
    pub ticks: u64,
    pub want_pos: Arc<AtomicBool>,
    pub media_tx: std::sync::mpsc::Sender<media::Cmd>,
    pub hover_timer: Timer,
    pub collapse_timer: Timer,
    pub shrink_timer: Timer,
    pub peek_timer: Timer,
    pub toast_timer: Timer,
    pub audio: Option<audio::Audio>,
    pub app_volumes: Rc<VecModel<AppVolume>>,
    pub mic_test: Option<audio::MicTest>,
    pub mic_timer: Timer,
    pub mic_test_started: Instant,
    pub last_shot: Option<std::path::PathBuf>,
    pub cur_size: (f32, f32),
    pub car: Option<CarShow>,
    pub car_timer: Timer,
    pub last_car_show: Option<Instant>,
    pub skids: Rc<VecModel<Skid>>,
    pub puffs: Rc<VecModel<Puff>>,
    pub rng: u64,
    pub eye_next: Instant,
    pub eye_end: Option<Instant>,
    pub pill_car_timer: Timer,
    pub pill_car_start: Instant,
    pub well: Wellbeing,
    /// seconds the PC was on per day (from the Windows event log)
    pub pc_on: std::collections::BTreeMap<NaiveDate, u32>,
    pub pc_on_at: Option<Instant>,
    pub coffee_day: Option<NaiveDate>,
    pub ports_system: bool,
    /// (when, bytes received, bytes sent) for the network speedometer
    pub net_prev: Option<(Instant, u64, u64)>,
    /// the ignition sweep plays once per launch, not on every open
    pub dash_swept: bool,
    /// right-drag in progress: (cursor, island anchor) when the button went down
    pub drag_from: Option<((i32, i32), (i32, i32))>,
    pub drag_timer: Timer,
    /// "Move" from settings: the island follows the pointer until a click drops it
    pub moving: bool,
    pub move_pressed: bool,
    /// gear before the last shift (R goes back to it)
    pub prev_gear: i32,
    /// gear 6 muted the mic, so shifting out unmutes it
    pub gear_muted_mic: bool,
    /// workspace shown on the right in Developer → Workspaces
    pub dev_ws_sel: String,
    /// screen time per app
    pub screen: ScreenTrack,
    /// game mode, game detection and the library
    pub play: Play,
    /// Shelf > Send: the basket and the open share link
    pub send: send::Send,
}

pub type Shared = Rc<RefCell<App>>;

pub fn s(v: impl Into<SharedString>) -> SharedString {
    v.into()
}

pub fn trace(msg: &str) {
    if std::env::var_os("WINKLET_TRACE").is_some() {
        println!("{} {msg}", Local::now().format("%H:%M:%S%.3f"));
    }
}

thread_local! {
    pub static APP: RefCell<std::rc::Weak<RefCell<App>>> = RefCell::new(std::rc::Weak::new());
}

pub fn with_app(f: impl FnOnce(&Shared)) {
    if let Some(shared) = APP.with(|a| a.borrow().upgrade()) {
        f(&shared);
    }
}

/// Runs `f` on the next event-loop turn, after any current borrow of the app has ended.
pub fn defer(f: impl FnOnce(&Shared) + 'static) {
    Timer::single_shot(Duration::ZERO, move || with_app(f));
}

/// Collapses after `ms` unless the pointer came back (or the user is typing).
pub fn schedule_collapse(shared: &Shared, app: &mut App, ms: u64) {
    let weak = Rc::downgrade(shared);
    app.collapse_timer.start(TimerMode::SingleShot, Duration::from_millis(ms), move || {
        let Some(shared) = weak.upgrade() else { return };
        let mut a = shared.borrow_mut();
        if a.hovering || a.drag_from.is_some() {
            return;
        }
        if a.ui().global::<Bridge>().get_editing() && ms < 3000 {
            // typing: be patient
            schedule_collapse(&shared, &mut a, 3000);
            return;
        }
        a.collapse();
        schedule_shrink(&shared, &mut a);
    });
}

/// Shrinks the window back to the pill once the collapse animation has finished.
pub fn schedule_shrink(shared: &Shared, app: &mut App) {
    let weak = Rc::downgrade(shared);
    app.shrink_timer.start(TimerMode::SingleShot, Duration::from_millis(560), move || {
        let Some(shared) = weak.upgrade() else { return };
        let mut a = shared.borrow_mut();
        let ui = a.ui();
        let b = ui.global::<Bridge>();
        if !b.get_expanded() && !b.get_peeking() {
            a.resize(false);
        }
    });
}

impl App {
    pub fn ui(&self) -> AppWindow {
        self.ui.upgrade().expect("ui alive")
    }

    // ------------------------------------------------------------ window
    pub fn resize(&mut self, big: bool) {
        // the car show needs extra room under the island
        let small = if self.data.settings.game_mode { SMALL_GAME } else { SMALL };
        let target = if !big { small } else if self.car.is_some() { BIG_CAR } else { BIG };
        if self.cur_size == target {
            return;
        }
        self.cur_size = target;
        self.place();
    }

    pub fn place(&mut self) {
        let ui = self.ui();
        // the Size setting rides on top of the display scale: everything is laid out in
        // logical px, so changing the factor grows the pill and the panel together
        let want = sys::dpi_scale(self.hwnd) * self.data.settings.ui_scale.clamp(0.8, 1.4);
        if (ui.window().scale_factor() - want).abs() > 0.001 {
            ui.window().dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: want });
            // Slint answers a new factor by resizing the window to its preferred size on the
            // next frame; put it back once that has happened
            Timer::single_shot(Duration::from_millis(80), || with_app(|s| s.borrow_mut().place()));
        }
        let scale = ui.window().scale_factor();
        let anchor = self.data.settings.island_pos;
        let edge = sys::island_edge(anchor, scale);
        let pill = self.cur_size == SMALL || self.cur_size == SMALL_GAME;
        let (w, h) = if pill && (edge == sys::EDGE_LEFT || edge == sys::EDGE_RIGHT) { VERT } else { self.cur_size };
        sys::place_island(self.hwnd, (w * scale).round() as i32, (h * scale).round() as i32, anchor, edge, scale);
        let b = ui.global::<Bridge>();
        b.set_edge(edge);
        b.set_docked(edge == sys::EDGE_TOP);
    }

    /// "Top centre" in settings: back to where it started.
    pub fn island_home(&mut self) {
        self.data.settings.island_pos = None;
        self.data.save();
        self.place();
        self.toast("Back at the top centre");
    }

    pub fn rand(&mut self) -> f32 {
        // xorshift64: plenty for picking animations
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn expand(&mut self) {
        let fg = sys::foreground();
        if fg != self.hwnd {
            self.prev_fg = fg;
        }
        self.shrink_timer.stop();
        self.peek_timer.stop();
        self.resize(true);
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_peeking(false);
        b.set_expanded(true);
        self.collapsed_at = None;
        self.want_pos.store(true, Ordering::Relaxed);
        // game mode opens on the Game tab
        if self.data.settings.game_mode && b.get_tab() != TAB_GAME {
            self.play.tab_before = b.get_tab();
            b.set_tab(TAB_GAME);
        }
        self.refresh_tab(b.get_tab());
        let cooled = self.last_car_show.is_none_or(|t| t.elapsed() > Duration::from_secs(10));
        if self.data.settings.car_show && cooled && self.car.is_none() {
            self.last_car_show = Some(Instant::now());
            defer(App::start_car_show);
        }
    }

    /// Data that is only fetched while its tab is on screen.
    pub fn refresh_tab(&mut self, tab: i32) {
        match tab {
            0 => self.refresh_big_clock(),
            TAB_DEV => {
                self.refresh_dev();
                match self.ui().global::<Bridge>().get_dev_view() {
                    1 => self.refresh_ports(),
                    2 => self.ignition_sweep(),
                    workspaces::VIEW_WORKSPACES => self.refresh_workspaces(),
                    _ => {}
                }
            }
            TAB_HEALTH => {
                self.refresh_health();
                let view = self.ui().global::<Bridge>().get_health_view();
                if view == screen::VIEW_APPS {
                    self.refresh_screen();
                }
                if view == screen::VIEW_SUMMARY {
                    self.refresh_summary();
                }
                if view == screen::VIEW_HISTORY {
                    self.reload_pc_on();
                    self.refresh_history();
                }
            }
            TAB_AUDIO => self.refresh_audio(),
            TAB_SHELF => {
                self.refresh_shelf();
                self.refresh_send();
            }
            TAB_GEAR => self.refresh_gear_time(),
            TAB_GAME => {
                self.refresh_game();
                if self.ui().global::<Bridge>().get_game_view() == 1 {
                    self.refresh_games();
                }
            }
            _ => {}
        }
    }

    pub fn collapse(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_expanded(false);
        b.set_editing(false);
        self.stop_mic_test();
        sys::restore_foreground(self.hwnd, self.prev_fg);
        self.want_pos.store(false, Ordering::Relaxed);
        self.collapsed_at = Some(Instant::now());
        self.trimmed = false;
        self.stop_car_show();
    }

    pub fn peek(&mut self, kind: i32, title: &str, sub: &str, secs: u64) {
        if self.hidden || !self.gear_allows(kind) {
            return;
        }
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_peek_kind(kind);
        b.set_peek_title(s(title));
        b.set_peek_sub(s(sub));
        if b.get_expanded() {
            self.toast(title);
            return;
        }
        self.shrink_timer.stop();
        self.resize(true);
        b.set_peeking(true);
        self.collapsed_at = None;
        self.arm_peek_timer(secs);
    }

    pub fn arm_peek_timer(&mut self, secs: u64) {
        let w = self.ui.clone();
        self.peek_timer.start(TimerMode::SingleShot, Duration::from_secs(secs), move || {
            if let Some(ui) = w.upgrade() {
                ui.global::<Bridge>().set_peeking(false);
                ui.global::<Bridge>().invoke_dismiss_peek();
            }
        });
    }

    pub fn toast(&mut self, msg: &str) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_toast(s(msg));
        let w = self.ui.clone();
        self.toast_timer.start(TimerMode::SingleShot, Duration::from_millis(1800), move || {
            if let Some(ui) = w.upgrade() {
                ui.global::<Bridge>().set_toast(s(""));
            }
        });
    }

    /// Settings > Reset: forgets everything and starts fresh (the island keeps running).
    pub fn reset_all_data(&mut self) {
        self.stop_mic_test();
        self.stop_car_show();
        crate::store::delete_all();
        self.play.run = None;
        crate::fps::set_target(0);
        self.data = crate::store::Data::default();
        self.data.drinks = crate::store::default_drinks();
        // launch-at-startup lives in the registry: keep showing what it really is
        self.data.settings.autostart = sys::autostart_enabled();
        self.screen.log = Default::default();
        self.data.save();
        self.screen.log.save();
        self.place();
        self.refresh_all();
        self.refresh_shelf();
        self.toast("Everything was reset");
    }

    pub fn refresh_all(&mut self) {
        self.refresh_clock();
        self.refresh_todos();
        self.refresh_alarms();
        self.refresh_water();
        self.refresh_focus();
        self.refresh_settings();
        self.refresh_gear();
        self.refresh_game();
        self.refresh_summary_setting();
        self.refresh_playlists();
    }
}
