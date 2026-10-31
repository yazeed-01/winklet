//! Pushes clock, water and settings state into the UI.

use super::{App, s};
use crate::{Bar, Bridge};
use chrono::{Duration as CDuration, Local};
use slint::{ComponentHandle, ModelRc, VecModel};

impl App {
    pub fn refresh_big_clock(&self) {
        let now = Local::now();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let h24 = self.data.settings.clock_24h;
        b.set_clock_main(s(now.format(if h24 { "%H:%M" } else { "%-I:%M" }).to_string()));
        b.set_clock_sec(s(now.format("%S").to_string()));
        b.set_clock_ampm(s(if h24 { String::new() } else { now.format("%p").to_string() }));
    }

    pub fn refresh_clock(&mut self) {
        let now = Local::now();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let time = if self.data.settings.clock_24h {
            now.format("%H:%M").to_string()
        } else {
            now.format("%-I:%M %p").to_string()
        };
        b.set_time_text(s(time));
        // the game pill has no room for AM/PM
        b.set_time_short(s(now.format(if self.data.settings.clock_24h { "%H:%M" } else { "%-I:%M" }).to_string()));
        b.set_date_text(s(now.format("%A, %-d %B").to_string()));
        b.set_day_text(s(now.format("%a %-d").to_string()));
    }

    pub fn refresh_water(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let st = &self.data.settings;
        b.set_water_ml(self.data.water_today());
        b.set_water_goal(st.water_goal_ml);
        b.set_cup_ml(st.cup_ml);
        let today = Local::now().date_naive();
        let week: Vec<Bar> = (0..7)
            .rev()
            .map(|i| {
                let d = today - CDuration::days(i);
                let ml = *self.data.water.get(&d).unwrap_or(&0);
                Bar {
                    label: s(d.format("%a").to_string().chars().take(2).collect::<String>()),
                    value: ml as f32 / st.water_goal_ml.max(1) as f32,
                    today: i == 0,
                }
            })
            .collect();
        b.set_water_week(ModelRc::new(VecModel::from(week)));
        self.refresh_drinks(today);
    }

    pub fn refresh_settings(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let st = &self.data.settings;
        b.set_autostart(st.autostart);
        b.set_clock_24h(st.clock_24h);
        b.set_hide_fullscreen(st.hide_fullscreen);
        b.set_water_reminders(st.water_reminders);
        b.set_water_interval(st.water_interval_min);
        b.set_workspace(s(st.workspace_dir.clone()));
        b.set_car_enabled(st.car_show);
        b.set_island_home_start(st.island_home_on_start);
        b.set_theme(st.theme);
        b.set_ui_scale(st.ui_scale);
        b.set_island_alpha(st.island_alpha);
        b.set_eye_color(st.eye_color);
        b.set_bubble(st.bubble);
        b.set_data_dir(s(crate::store::data_dir().to_string_lossy().to_string()));
        b.set_car_model(st.car_model);
        b.set_eyes_lively(st.eyes_lively);
        b.set_stand_enabled(st.stand_reminders);
        b.set_stand_interval(st.stand_interval_min);
        b.set_eye_rest(st.eye_rest);
        b.set_wait_pause(st.wait_for_pause);
        b.set_pause_calls(st.pause_in_calls);
        b.set_work_hours(st.work_hours_only);
        b.set_work_hours_text(s(format!("{:02}–{:02}", st.work_start_h, st.work_end_h)));
    }
}
