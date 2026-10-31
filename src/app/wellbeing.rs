//! Sitting time, stand-up and eye-rest reminders, the guided stretch and the Today view.

use super::{App, TAB_GEAR, TAB_HEALTH, s};
use crate::{Bar, Bridge, sys};
use chrono::{Duration as CDuration, Local, Timelike};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::time::{Duration, Instant};

/// Idle this long = you got up. Shorter idle (reading, watching) keeps the sitting session going.
const BREAK_IDLE_S: u32 = 180;
/// Input within this many seconds counts as actively at the computer.
const ACTIVE_IDLE_S: u32 = 60;

const STAND_TIPS: [&str; 6] = [
    "Stand up and stretch your back",
    "Take a 2-minute walk",
    "Stand, reach up high, then touch your toes",
    "Walk to get some water",
    "Stand up and do 10 slow squats",
    "Open a window and breathe for a minute",
];
/// gentler asks for someone who keeps snoozing
const LIGHT_TIPS: [&str; 3] = [
    "Just roll your shoulders back 5 times",
    "Stretch your neck side to side",
    "Sit up straight and unclench your jaw",
];
const GUIDE_STEPS: [&str; 4] = [
    "Stand up tall",
    "Reach both arms overhead",
    "Roll your shoulders back ×5",
    "Twist gently left and right",
];

#[derive(Default)]
pub struct Wellbeing {
    /// (sitting minutes, active minutes, breaks) last shown on the Health tab
    pub shown: (u32, u32, u32),
    /// seconds of activity since the last real break
    pub session_s: u32,
    /// reminders already shown this session (drives escalation)
    pub stage: u32,
    /// a reminder is due and we're waiting for a natural pause
    pub pending_since: Option<Instant>,
    pub snooze_until: Option<Instant>,
    /// snoozes in a row; shortens snooze + softens the ask
    pub snoozes: u32,
    pub eye_rest_s: u32,
    pub guide_start: Option<Instant>,
    pub last_check: Option<Instant>,
    pub tip: usize,
    pub unsaved_s: u32,
}

impl App {
    /// Runs every 2 s. Tracks how long you've been sitting and decides when
    /// (and how firmly) to suggest a break.
    pub fn wellbeing_tick(&mut self) {
        // playing with a controller isn't idle
        let idle = if self.pad_recent() { 0 } else { sys::idle_seconds() };
        let now_i = Instant::now();
        let dt = self.well.last_check.map(|t| t.elapsed().as_secs()).unwrap_or(2) as u32;
        self.well.last_check = Some(now_i);
        let now = Local::now();

        // Away for a while (or the PC slept) = a real break.
        if idle >= BREAK_IDLE_S || dt > 90 {
            if self.well.session_s >= 120 {
                let reminded = self.well.stage > 0;
                self.record_break();
                if reminded {
                    self.peek(14, "Welcome back", "Break counted. Nice stretch!", 3);
                }
            } else {
                self.reset_session();
            }
            return;
        }

        if idle < ACTIVE_IDLE_S {
            let add = dt.min(10);
            self.well.session_s += add;
            self.well.eye_rest_s += add;
            let hour = now.hour() as usize;
            let session = self.well.session_s;
            let day = self.data.health.entry(now.date_naive()).or_default();
            day.hours.resize(24, 0);
            day.active_s += add;
            day.hours[hour] += add;
            day.longest_s = day.longest_s.max(session);
            self.well.unsaved_s += add;
            self.gear_count(add);
            if self.well.unsaved_s >= 60 {
                self.well.unsaved_s = 0;
                self.data.save();
            }
        }

        let ui = self.ui();
        let b = ui.global::<Bridge>();
        // only when something on it changed: every refresh re-animates the ring and the bars
        if b.get_expanded() && b.get_tab() == TAB_HEALTH {
            let day = self.data.health.get(&Local::now().date_naive());
            let shown = (self.well.session_s / 60, day.map_or(0, |d| d.active_s / 60), day.map_or(0, |d| d.breaks));
            if shown != self.well.shown {
                self.well.shown = shown;
                self.refresh_health();
            }
        }
        if b.get_expanded() && b.get_tab() == TAB_GEAR {
            self.refresh_gear_time();
        }
        if self.well.guide_start.is_some() {
            return;
        }

        let st = self.data.settings.clone();
        if st.work_hours_only && !(st.work_start_h..st.work_end_h).contains(&now.hour()) {
            return;
        }
        // a natural pause: stopped typing for a few seconds but still here
        let paused = (4..ACTIVE_IDLE_S).contains(&idle);
        let busy = self.hidden
            || self.game_in_front()
            || b.get_expanded()
            || (self.focus.running && !self.focus.is_break)
            || !self.gear_allows(11)
            || (st.pause_in_calls && self.audio.as_ref().is_some_and(|a| a.mic_in_use()));

        // 20-20-20: every 20 min, look 20 ft away for 20 s
        // a gear that holds a reminder delays it (it shows after you shift down) instead of dropping it
        if st.eye_rest && self.gear_allows(13) && self.well.eye_rest_s >= 20 * 60 && paused && !busy && !b.get_peeking() {
            self.well.eye_rest_s = 0;
            self.peek(13, "Rest your eyes", "Look at something far away for 20 seconds", 20);
            return;
        }

        if !st.stand_reminders || self.well.snooze_until.is_some_and(|u| now_i < u) {
            return;
        }
        let interval = st.stand_interval_min.max(10) as u32 * 60;
        // 50 → 75 → 90 min, then every half interval
        let due = match self.well.stage {
            0 => interval,
            1 => interval * 3 / 2,
            2 => interval * 9 / 5,
            n => interval * 9 / 5 + (n - 2) * interval / 2,
        };
        if self.well.session_s < due || busy {
            return;
        }
        let waiting = *self.well.pending_since.get_or_insert(now_i);
        // don't interrupt mid-sentence, but don't wait forever either
        if st.wait_for_pause && !paused && waiting.elapsed() < Duration::from_secs(180) {
            return;
        }
        self.fire_stand_reminder();
    }

    pub fn fire_stand_reminder(&mut self) {
        let mins = self.well.session_s / 60;
        let sitting = if mins >= 60 { format!("{}h {:02}m", mins / 60, mins % 60) } else { format!("{mins} min") };
        let mut tip = if self.well.snoozes >= 2 {
            LIGHT_TIPS[self.well.tip % LIGHT_TIPS.len()].to_string()
        } else {
            STAND_TIPS[self.well.tip % STAND_TIPS.len()].to_string()
        };
        self.well.tip += 1;
        // fold a water reminder that's nearly due into this one
        let st = &self.data.settings;
        let water_soon = Duration::from_secs(st.water_interval_min.max(5) as u64 * 60 * 7 / 10);
        if st.water_reminders && self.data.water_today() < st.water_goal_ml && self.last_drink.elapsed() >= water_soon {
            tip.push_str(" · drink water too");
            self.last_drink = Instant::now();
        }
        let (title, secs) = match self.well.stage {
            0 => (format!("{sitting} sitting"), 12),
            1 => (format!("{sitting} sitting. Time to move"), 20),
            _ => (format!("{sitting} without a break"), 120),
        };
        if self.well.stage >= 1 {
            sys::play_sound("Notification.Reminder");
        }
        if self.well.stage >= 2 {
            self.eye_event(11, 120.0); // tired eyes
        }
        self.well.stage += 1;
        self.well.pending_since = None;
        self.peek(11, &title, &tip, secs);
    }

    pub fn reset_session(&mut self) {
        let w = &mut self.well;
        w.session_s = 0;
        w.stage = 0;
        w.pending_since = None;
        w.snooze_until = None;
        w.eye_rest_s = 0;
    }

    pub fn record_break(&mut self) {
        let day = self.data.health.entry(Local::now().date_naive()).or_default();
        day.breaks += 1;
        self.reset_session();
        self.well.snoozes = 0;
        self.data.save();
        self.eye_event(4, 4.0);
        self.refresh_health();
    }

    pub fn start_guide(&mut self) {
        self.well.guide_start = Some(Instant::now());
        self.guide_tick();
        self.peek(12, GUIDE_STEPS[0], "Step 1 of 4", 75);
    }

    /// 60 s guided stretch, 4 steps of 15 s, shown in the peek.
    pub fn guide_tick(&mut self) {
        let Some(start) = self.well.guide_start else { return };
        let e = start.elapsed().as_secs_f32();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        if e >= 60.0 {
            self.well.guide_start = None;
            self.record_break();
            self.eye_event(10, 4.0);
            sys::play_sound("Notification.Default");
            self.peek(14, "Great job!", "Stretch done · break logged", 4);
            return;
        }
        let step = ((e / 15.0) as usize).min(3);
        b.set_guide_progress(e / 60.0);
        b.set_guide_secs((60.0 - e).ceil() as i32);
        b.set_peek_title(s(GUIDE_STEPS[step]));
        b.set_peek_sub(s(format!("Step {} of 4", step + 1)));
    }

    pub fn refresh_health(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let today = Local::now().date_naive();
        let day = self.data.health.get(&today).cloned().unwrap_or_default();
        let fmt = |sec: u32| {
            let m = sec / 60;
            if m >= 60 { format!("{}h {:02}m", m / 60, m % 60) } else { format!("{m}m") }
        };
        let interval = self.data.settings.stand_interval_min.max(10) as f32 * 60.0;
        b.set_sit_min((self.well.session_s / 60) as i32);
        b.set_sit_progress(self.well.session_s as f32 / interval);
        b.set_active_text(s(fmt(day.active_s)));
        b.set_breaks_today(day.breaks as i32);
        b.set_longest_text(s(fmt(day.longest_s)));
        let mut hours = day.hours.clone();
        hours.resize(24, 0);
        let bars: Vec<f32> = hours.iter().map(|&x| x as f32 / 3600.0).collect();
        b.set_hour_bars(ModelRc::new(VecModel::from(bars)));
        // 7 days, scaled to an 8 h day
        let week: Vec<Bar> = (0..7)
            .rev()
            .map(|i| {
                let d = today - CDuration::days(i);
                let active = self.data.health.get(&d).map(|h| h.active_s).unwrap_or(0);
                Bar {
                    label: s(d.format("%a").to_string().chars().take(2).collect::<String>()),
                    value: active as f32 / (8.0 * 3600.0),
                    today: i == 0,
                }
            })
            .collect();
        b.set_health_week(ModelRc::new(VecModel::from(week)));
    }
}
