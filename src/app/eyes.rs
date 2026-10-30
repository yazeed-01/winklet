//! "Lively eyes": random moods, the morning coffee, and the tiny car crossing the pill.

use super::{App, with_app};
use crate::Bridge;
use chrono::{Local, Timelike};
use slint::{ComponentHandle, TimerMode};
use std::time::{Duration, Instant};

impl App {
    /// One-shot eye animation (only when "Lively eyes" is on).
    pub fn eye_event(&mut self, mood: i32, secs: f32) {
        // game mode: the eyes are busy with the controller
        if !self.data.settings.eyes_lively || self.data.settings.game_mode {
            return;
        }
        self.ui().global::<Bridge>().set_eye_mood(mood);
        self.eye_end = Some(Instant::now() + Duration::from_secs_f32(secs));
        if mood == 8 {
            self.pill_car_start = Instant::now();
            self.pill_car_timer.start(TimerMode::Repeated, Duration::from_millis(16), || {
                with_app(|s| s.borrow_mut().pill_car_frame());
            });
        }
    }

    /// The tiny car crossing the pill; the eyes follow it in Slint.
    pub fn pill_car_frame(&mut self) {
        let u = self.pill_car_start.elapsed().as_secs_f32() / 2.6;
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        if u >= 1.0 {
            self.pill_car_timer.stop();
            b.set_pill_car_x(-60.0);
            return;
        }
        // accelerate in, cruise past the eyes, speed off
        let e = if u < 0.5 { 4.0 * u * u * u } else { 1.0 - (-2.0 * u + 2.0).powi(3) / 2.0 };
        b.set_pill_car_x(-48.0 + 270.0 * e);
    }

    pub fn eyes_tick(&mut self) {
        let now = Instant::now();
        if let Some(end) = self.eye_end {
            if now >= end {
                self.ui().global::<Bridge>().set_eye_mood(0);
                self.eye_end = None;
                let wait = 7.0 + self.rand() * 10.0;
                self.eye_next = now + Duration::from_secs_f32(wait);
            }
            return;
        }
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        if !self.data.settings.eyes_lively || now < self.eye_next || b.get_peeking() || self.hidden {
            return;
        }
        if b.get_focus_active() || b.get_user_idle() {
            return;
        }
        // first thing in the morning: coffee
        let today = Local::now().date_naive();
        if (6..11).contains(&Local::now().hour()) && self.coffee_day != Some(today) {
            self.coffee_day = Some(today);
            self.eye_event(12, 8.0);
            return;
        }
        if b.get_media_playing() {
            // vibe to the music for a bit, then go back to showing the track
            self.eye_event(7, 6.0);
            return;
        }
        let r = self.rand();
        let (mood, secs) = match r {
            r if r < 0.18 => (1, 2.0),  // look left
            r if r < 0.36 => (2, 2.0),  // look right
            r if r < 0.48 => (5, 1.0),  // wink
            r if r < 0.62 => (4, 2.0),  // happy
            r if r < 0.72 => (6, 2.0),  // surprised
            r if r < 0.82 => (8, 3.0),  // watch a tiny car drive by
            r if r < 0.92 => (12, 6.0), // coffee break
            _ => (10, 2.0),             // love
        };
        self.eye_event(mood, secs);
    }
}
