//! The Power tab: lock, sleep, restart, sign out or shut down, now or after a while.

use super::{App, defer, s};
use crate::{Bridge, sys};
use chrono::{DateTime, Local};
use slint::ComponentHandle;

/// the peek a minute before a timed action (see the peek-kind list in bridge.slint)
pub const PEEK_POWER: i32 = 18;

const NAMES: [&str; 5] = ["Lock", "Sleep", "Restart", "Sign out", "Shut down"];
const DOING: [&str; 5] = ["Locking", "Going to sleep", "Restarting", "Signing out", "Shutting down"];

/// A timed power action. The deadline is wall-clock time so a PC that slept through it
/// can tell it's late instead of firing the moment it wakes up.
pub struct PowerPlan {
    pub action: i32,
    pub at: DateTime<Local>,
    pub warned: bool,
}

impl App {
    /// "Shut down in 30 min" (0 = right now). A new plan replaces the old one.
    pub fn power_go(&mut self, action: i32, minutes: i32) {
        let Some(name) = NAMES.get(action as usize) else { return };
        if minutes <= 0 {
            self.power = None;
            self.refresh_power();
            self.power_run(action);
            return;
        }
        let at = Local::now() + chrono::Duration::minutes(minutes as i64);
        self.power = Some(PowerPlan { action, at, warned: false });
        self.refresh_power();
        let when = self.power_clock(at);
        self.toast(&format!("{name} at {when}"));
    }

    pub fn power_cancel(&mut self) {
        if self.power.take().is_none() {
            return;
        }
        // dismissing borrows the app again, so after this borrow ends
        defer(|s| {
            let ui = s.borrow().ui();
            let b = ui.global::<Bridge>();
            if b.get_peeking() && b.get_peek_kind() == PEEK_POWER {
                b.invoke_dismiss_peek();
            }
        });
        self.refresh_power();
        self.toast("Cancelled");
    }

    /// Every second while something is planned: warn a minute ahead, then do it.
    pub fn power_tick(&mut self) {
        let Some(plan) = &mut self.power else { return };
        let left = (plan.at - Local::now()).num_seconds();
        let action = plan.action;
        if left <= 0 {
            let late = -left > 120;
            let at = plan.at;
            self.power = None;
            let when = self.power_clock(at);
            self.refresh_power();
            if late {
                // the PC was asleep or off when it was due: don't spring it on the user now
                self.peek(PEEK_POWER, &format!("Skipped: {} at {when}", NAMES[action as usize].to_lowercase()), "The PC wasn't awake when it was due", 8);
            } else {
                self.power_run(action);
            }
            return;
        }
        if left <= 60 && !plan.warned {
            plan.warned = true;
            let title = format!("{} in a minute", DOING[action as usize]);
            self.peek(PEEK_POWER, &title, "Save your work, or cancel it", 60);
            self.eye_event(6, 3.0);
        }
        self.refresh_power();
    }

    fn power_run(&mut self, action: i32) {
        let Some(what) = sys::PowerAction::from_index(action) else { return };
        // restart, sign out and shut down end this session: write everything down first
        self.data.save();
        self.screen.log.save();
        if !sys::power(what) {
            self.toast(&format!("Couldn't {}", NAMES[action as usize].to_lowercase()));
        }
    }

    fn power_clock(&self, at: DateTime<Local>) -> String {
        at.format(if self.data.settings.clock_24h { "%H:%M" } else { "%-I:%M %p" }).to_string()
    }

    pub fn refresh_power(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let (pending, left, at) = match &self.power {
            Some(p) => {
                let secs = (p.at - Local::now()).num_seconds().max(0);
                let left = if secs >= 3600 {
                    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
                } else {
                    format!("{}:{:02}", secs / 60, secs % 60)
                };
                (p.action, left, self.power_clock(p.at))
            }
            None => (-1, String::new(), String::new()),
        };
        // change-only: this runs every second while a plan is set
        if b.get_power_pending() != pending {
            b.set_power_pending(pending);
        }
        if b.get_power_left() != left.as_str() {
            b.set_power_left(s(left));
        }
        if b.get_power_at() != at.as_str() {
            b.set_power_at(s(at));
        }
    }
}
