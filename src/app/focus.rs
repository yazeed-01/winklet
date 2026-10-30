//! The focus (pomodoro) timer.

use super::{App, s};
use crate::{Bridge, sys};
use chrono::Local;
use slint::ComponentHandle;

#[derive(Default)]
pub struct Focus {
    pub running: bool,
    pub is_break: bool,
    pub left: i32,
    pub total: i32,
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
    }

    pub fn focus_reset_to(&mut self, is_break: bool) {
        let min = if is_break { self.data.settings.break_min } else { self.data.settings.focus_min };
        self.focus = Focus { running: false, is_break, left: min * 60, total: min * 60 };
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
}
