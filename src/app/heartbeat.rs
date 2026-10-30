//! The 1 s heartbeat and the system checks it drives (idle, battery, fullscreen, water).

use super::{App, TAB_AUDIO, TAB_DEV, trace};
use crate::{Bridge, sys};
use chrono::{Local, Timelike};
use slint::ComponentHandle;
use std::time::{Duration, Instant};

impl App {
    pub fn tick(&mut self) {
        self.ticks += 1;
        let now = Local::now();

        if now.minute() != self.last_minute {
            if now.hour() == 0 && now.minute() == 0 {
                self.refresh_water();
                self.refresh_todos();
            }
            self.last_minute = now.minute();
            self.refresh_clock();
            self.check_alarms();
            self.check_reminders();
            self.check_summary();
            self.refresh_alarms();
        }

        if self.focus.running {
            self.focus.left -= 1;
            if self.focus.left <= 0 {
                self.focus_finish();
            } else {
                self.refresh_focus();
            }
        }

        if self.ticks % 2 == 0 {
            self.check_system();
        }
        if self.ticks % 4 == 0 {
            self.refresh_mic(); // catches mutes done from Teams/Discord/Windows
        }
        self.eyes_tick();
        self.guide_tick();
        if self.ui().global::<Bridge>().get_expanded() && self.ui().global::<Bridge>().get_tab() == 0 {
            self.refresh_big_clock();
        }
        {
            let ui = self.ui();
            let b = ui.global::<Bridge>();
            if b.get_expanded() && b.get_tab() == TAB_DEV && b.get_dev_view() == 1 && self.ticks % 3 == 0 {
                self.refresh_ports();
            }
            if b.get_expanded() && b.get_tab() == TAB_DEV && b.get_dev_view() == 2 && !b.get_dash_sweep() {
                self.refresh_dash();
            }
        }

        let ui = self.ui();

        let b = ui.global::<Bridge>();
        if b.get_expanded() && self.ticks % 2 == 0 && (b.get_tab() == TAB_DEV || b.get_tab() == TAB_AUDIO) {
            self.refresh_tab(b.get_tab());
        }

        // Give memory back after the island has been quiet for a long while
        // (sooner would make the next open page it back in and stutter on a busy PC).
        if let Some(t) = self.collapsed_at {
            if !self.trimmed && t.elapsed() > Duration::from_secs(300) && !b.get_peeking() && !self.hovering {
                sys::trim_memory();
                self.trimmed = true;
            }
        }
    }

    pub fn check_system(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();

        // controller play counts as being here
        let idle = sys::idle_seconds() > 60 && !self.pad_recent();
        if b.get_user_idle() != idle {
            trace(&format!("idle {idle}"));
            if idle {
                self.eye_event(9, 4.0);
            }
            b.set_user_idle(idle);
        }

        let (pct, plugged) = sys::battery();
        if b.get_battery() != pct {
            b.set_battery(pct);
        }
        if b.get_charging() != (plugged && pct >= 0) {
            b.set_charging(plugged && pct >= 0);
        }
        if pct >= 0 {
            if let Some(prev) = self.plugged {
                if prev != plugged {
                    if plugged {
                        self.peek(1, "Charging", &format!("{pct}% · plugged in"), 3);
                        self.eye_event(4, 6.0);
                    } else {
                        self.peek(2, "On battery", &format!("{pct}% remaining"), 3);
                    }
                }
            }
            self.plugged = Some(plugged);
        }

        // Fullscreen: game mode stays over the game it is tracking (borderless only;
        // on top of exclusive fullscreen it would knock the game out of it).
        // Anything else fullscreen follows "Hide in fullscreen".
        let fs = sys::fullscreen_state();
        let st = &self.data.settings;
        let over_game = st.game_mode && self.game_in_front();
        let hide = (fs == sys::FS_EXCLUSIVE && (st.hide_fullscreen || st.game_mode))
            || (fs == sys::FS_WINDOW && st.hide_fullscreen && !over_game);
        if hide != self.hidden {
            self.hidden = hide;
            sys::set_visible(self.hwnd, !hide);
            if hide {
                self.play.topped = false;
            }
        }

        // water reminder
        let st = &self.data.settings;
        let hour = Local::now().hour();
        if st.water_reminders
            && self.gear_allows(3)
            && (8..22).contains(&hour)
            && self.data.water_today() < st.water_goal_ml
            && !self.game_in_front()
            && self.last_drink.elapsed() >= Duration::from_secs(st.water_interval_min.max(5) as u64 * 60)
        {
            self.last_drink = Instant::now();
            let sub = format!("{} / {} ml today", self.data.water_today(), st.water_goal_ml);
            sys::play_sound("Notification.Default");
            self.peek(3, "Time for some water", &sub, 12);
        }

        self.front_tick();
        self.wellbeing_tick();
    }
}
