//! The focus (pomodoro) timer, plus a plain countdown timer and a stopwatch.

use super::{App, s, with_app};
use crate::{Bridge, LapItem, sys};
use chrono::Local;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::time::{Duration, Instant};

pub const TAB_FOCUS: i32 = 3;

#[derive(Default)]
pub struct Focus {
    pub running: bool,
    pub is_break: bool,
    pub left: i32,
    pub total: i32,
    pub timer: Countdown,
    pub watch: Stopwatch,
}

pub struct Countdown {
    pub running: bool,
    pub minutes: i32,
    pub left: i32,
    pub total: i32,
}

impl Default for Countdown {
    fn default() -> Self {
        Countdown { running: false, minutes: 5, left: 300, total: 300 }
    }
}

#[derive(Default)]
pub struct Stopwatch {
    pub banked: Duration,           // time from earlier runs
    pub since: Option<Instant>,     // set while running
    pub laps: Vec<Duration>,        // lap end times, oldest first
    pub fast: Timer,                // redraws the hundredths while the page is open
}

impl Stopwatch {
    pub fn elapsed(&self) -> Duration {
        self.banked + self.since.map(|t| t.elapsed()).unwrap_or_default()
    }
}

fn mm_ss(secs: i32) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}

fn lap_text(d: Duration) -> String {
    format!("{}.{:02}", mm_ss(d.as_secs() as i32), d.subsec_millis() / 10)
}

impl App {
    pub fn refresh_focus(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let f = &self.focus;
        b.set_focus_running(f.running);
        b.set_focus_active(f.running || f.left != f.total);
        b.set_focus_break(f.is_break);
        b.set_focus_left(s(format!("{:02}:{:02}", f.left / 60, f.left % 60)));
        b.set_focus_progress(if f.total > 0 { 1.0 - f.left as f32 / f.total as f32 } else { 0.0 });
        b.set_focus_minutes(self.data.settings.focus_min);
        b.set_break_minutes(self.data.settings.break_min);
        b.set_focus_sessions(self.data.sessions_today());

        let t = &f.timer;
        b.set_timer_running(t.running);
        b.set_timer_active(t.running || t.left != t.total);
        b.set_timer_left(s(mm_ss(t.left)));
        b.set_timer_progress(if t.total > 0 { 1.0 - t.left as f32 / t.total as f32 } else { 0.0 });
        b.set_timer_minutes(t.minutes);

        self.refresh_watch();
    }

    /// Just the stopwatch readout; cheap enough to run every few frames.
    pub fn refresh_watch(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let w = &self.focus.watch;
        let e = w.elapsed();
        b.set_sw_running(w.since.is_some());
        b.set_sw_active(w.since.is_some() || !e.is_zero());
        b.set_sw_main(s(mm_ss(e.as_secs() as i32)));
        b.set_sw_frac(s(format!(".{:02}", e.subsec_millis() / 10)));
        b.set_sw_progress((e.as_millis() % 60_000) as f32 / 60_000.0);
    }

    pub fn refresh_laps(&self) {
        let w = &self.focus.watch;
        let splits: Vec<Duration> =
            w.laps.iter().enumerate().map(|(i, &end)| end - if i == 0 { Duration::ZERO } else { w.laps[i - 1] }).collect();
        // only mark best/worst once there is something to compare
        let fastest = splits.iter().min().copied().filter(|_| splits.len() > 2);
        let slowest = splits.iter().max().copied().filter(|_| splits.len() > 2);
        let items: Vec<LapItem> = splits
            .iter()
            .enumerate()
            .rev()
            .map(|(i, &d)| LapItem {
                num: i as i32 + 1,
                split: s(lap_text(d)),
                total: s(lap_text(w.laps[i])),
                tag: if Some(d) == fastest { 1 } else if Some(d) == slowest { 2 } else { 0 },
            })
            .collect();
        self.ui().global::<Bridge>().set_sw_laps(ModelRc::new(VecModel::from(items)));
    }

    pub fn focus_reset_to(&mut self, is_break: bool) {
        let min = if is_break { self.data.settings.break_min } else { self.data.settings.focus_min };
        let f = &mut self.focus;
        f.running = false;
        f.is_break = is_break;
        f.left = min * 60;
        f.total = min * 60;
    }

    pub fn timer_reset_to(&mut self, minutes: i32) {
        let secs = minutes * 60;
        self.focus.timer = Countdown { running: false, minutes, left: secs, total: secs };
    }

    pub fn focus_finish(&mut self) {
        if self.focus.is_break {
            self.focus_reset_to(false);
            self.peek(4, "Break's over", "Ready for another focus session?", 8);
        } else {
            *self.data.focus_sessions.entry(Local::now().date_naive()).or_default() += 1;
            self.data.save();
            self.focus_reset_to(true);
            self.focus.running = true;
            let msg = format!("Nice work — {} min break started", self.data.settings.break_min);
            self.peek(4, "Focus session complete", &msg, 8);
        }
        sys::play_sound("Notification.Reminder");
        self.refresh_focus();
    }

    pub fn sw_toggle(&mut self) {
        let w = &mut self.focus.watch;
        if let Some(t) = w.since.take() {
            w.banked += t.elapsed();
            w.fast.stop();
        } else {
            w.since = Some(Instant::now());
            w.fast.start(TimerMode::Repeated, Duration::from_millis(33), || {
                with_app(|s| {
                    let Ok(a) = s.try_borrow() else { return };
                    let ui = a.ui();
                    let b = ui.global::<Bridge>();
                    if b.get_expanded() && b.get_tab() == TAB_FOCUS && b.get_focus_mode() == 2 {
                        a.refresh_watch();
                    }
                });
            });
        }
        self.refresh_watch();
    }

    pub fn sw_lap(&mut self) {
        let w = &mut self.focus.watch;
        if w.since.is_none() {
            return;
        }
        let e = w.elapsed();
        w.laps.push(e);
        self.refresh_laps();
    }

    pub fn sw_reset(&mut self) {
        let w = &mut self.focus.watch;
        w.fast.stop();
        w.since = None;
        w.banked = Duration::ZERO;
        w.laps.clear();
        self.refresh_watch();
        self.refresh_laps();
    }

    pub fn timer_finish(&mut self) {
        let min = self.focus.timer.minutes;
        self.timer_reset_to(min);
        let msg = format!("Your {min} min timer is up");
        self.peek(4, "Time's up", &msg, 10);
        sys::play_sound("Notification.Looping.Alarm");
        self.refresh_focus();
    }
}
