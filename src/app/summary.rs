//! End-of-day summary: active time, focus sessions, tasks done, breaks, water,
//! gaming and the most used apps. Pops up once a day at the chosen hour.

use super::screen::{VIEW_SUMMARY, day_label, fmt_dur};
use super::{App, TAB_HEALTH, s};
use crate::{AppUseItem, Bridge};
use chrono::{Local, NaiveDate, Timelike};
use slint::{ComponentHandle, ModelRc, VecModel};

impl App {
    fn games_on(&self, day: NaiveDate) -> (u32, Option<String>) {
        let mut best: Option<(u32, String)> = None;
        let mut total = 0;
        for g in self.data.games.values() {
            let v = g.days.get(&day).copied().unwrap_or(0);
            total += v;
            if v > 0 && best.as_ref().is_none_or(|b| v > b.0) {
                best = Some((v, g.name.clone()));
            }
        }
        (total, best.map(|b| b.1))
    }

    pub fn refresh_summary(&mut self) {
        let day = self.screen.day;
        let today = Local::now().date_naive();
        let h = self.data.health.get(&day).cloned().unwrap_or_default();
        let focus = self.data.focus_sessions.get(&day).copied().unwrap_or(0);
        let tasks = self.data.tasks_done.get(&day).copied().unwrap_or(0);
        let water = self.data.water.get(&day).copied().unwrap_or(0);
        let (games, top_game) = self.games_on(day);
        let apps = self.apps_of(day);
        let screen: u32 = apps.iter().map(|a| a.1.secs).sum();
        let max = apps.first().map(|a| a.1.secs).unwrap_or(1);
        let top: Vec<AppUseItem> = apps.iter().take(3).map(|(k, u)| self.app_item(k, u, max)).collect();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_screen_day_label(s(day_label(day, today)));
        b.set_screen_can_next(day < today);
        b.set_sum_title(s(day.format("%A, %-d %B").to_string()));
        b.set_sum_active(s(fmt_dur(h.active_s)));
        b.set_sum_focus(s(focus.to_string()));
        b.set_sum_tasks(s(tasks.to_string()));
        b.set_sum_breaks(s(h.breaks.to_string()));
        b.set_sum_longest(s(fmt_dur(h.longest_s)));
        b.set_sum_water(s(format!("{:.1} L", water as f32 / 1000.0)));
        b.set_sum_games(s(match top_game {
            Some(g) if games >= 60 => format!("{} · {g}", fmt_dur(games)),
            _ => "–".into(),
        }));
        b.set_sum_screen(s(if screen > 0 { fmt_dur(screen) } else { "–".into() }));
        b.set_sum_top(ModelRc::new(VecModel::from(top)));
        self.refresh_summary_setting();
    }

    pub fn refresh_summary_setting(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_summary_on(self.data.settings.summary_on);
        b.set_summary_hour(self.data.settings.summary_h as i32);
        b.set_summary_hour_text(s(self.clock_at(self.data.settings.summary_h * 3600)));
    }

    /// Runs every minute: shows today's summary once, in the first 10 minutes of the chosen hour.
    pub fn check_summary(&mut self) {
        let st = &self.data.settings;
        let now = Local::now();
        let today = now.date_naive();
        if !st.summary_on || now.hour() != st.summary_h || now.minute() >= 10 || self.data.summary_day == Some(today) {
            return;
        }
        let h = self.data.health.get(&today).cloned().unwrap_or_default();
        if h.active_s < 30 * 60 || !self.gear_allows(17) || self.hidden {
            return;
        }
        self.data.summary_day = Some(today);
        self.data.save();
        let focus = self.data.sessions_today();
        let tasks = self.data.tasks_done.get(&today).copied().unwrap_or(0);
        let mut sub = format!("{focus} focus · {tasks} tasks · {} breaks", h.breaks);
        if let Some((_, u)) = self.apps_of(today).first() {
            sub.push_str(&format!(" · {} {}", u.name, fmt_dur(u.secs)));
        }
        self.peek(17, &format!("Day wrapped · {} active", fmt_dur(h.active_s)), &sub, 20);
    }

    /// Opens the island on Health → Summary for today.
    pub fn open_summary(&mut self) {
        self.screen.day = Local::now().date_naive();
        self.expand(); // (in game mode this picks the Game tab, so set the tab after)
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_tab(TAB_HEALTH);
        b.set_health_view(VIEW_SUMMARY);
        self.refresh_summary();
    }
}
