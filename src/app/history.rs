//! The activity heatmap (PC on-time or tracked activity over 30 weeks).

use super::{App, s, with_app};
use crate::{Bridge, HeatCell, uptime};
use chrono::{Datelike, Duration as CDuration, Local, NaiveDate};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::time::{Duration, Instant};

impl App {
    /// Re-reads PC on-time from the event log off the UI thread (~0.1 s), at most every 10 min.
    pub fn reload_pc_on(&mut self) {
        if self.pc_on_at.is_some_and(|t| t.elapsed() < Duration::from_secs(600)) {
            return;
        }
        self.pc_on_at = Some(Instant::now());
        std::thread::spawn(|| {
            let data = uptime::pc_on_per_day(220);
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|s| {
                    let mut a = s.borrow_mut();
                    a.pc_on = data;
                    a.refresh_history();
                })
            });
        });
    }

    pub fn refresh_history(&self) {
        const WEEKS: i64 = 30;
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let pc_mode = b.get_history_mode() == 0;
        let today = Local::now().date_naive();
        let value = |d: NaiveDate| -> u32 {
            if pc_mode {
                *self.pc_on.get(&d).unwrap_or(&0)
            } else {
                self.data.health.get(&d).map(|h| h.active_s).unwrap_or(0)
            }
        };
        let level = |sec: u32| -> i32 {
            let h = sec as f32 / 3600.0;
            let steps = if pc_mode { [0.02, 2.0, 5.0, 8.0] } else { [0.02, 1.0, 3.0, 5.0] };
            match h {
                h if h < steps[0] => 0,
                h if h < steps[1] => 1,
                h if h < steps[2] => 2,
                h if h < steps[3] => 3,
                _ => 4,
            }
        };
        let dur = |sec: u32| {
            let m = sec / 60;
            if m >= 60 { format!("{}h {:02}m", m / 60, m % 60) } else { format!("{m}m") }
        };
        let start = today - CDuration::days(today.weekday().num_days_from_monday() as i64 + (WEEKS - 1) * 7);
        let mut cells = Vec::new();
        let mut months = Vec::new();
        let (mut total, mut days_used, mut best) = (0u64, 0u32, (0u32, today));
        for w in 0..WEEKS {
            let week_start = start + CDuration::days(w * 7);
            let label = if w == 0 || week_start.month() != (week_start - CDuration::days(7)).month() {
                week_start.format("%b").to_string()
            } else {
                String::new()
            };
            months.push(s(label));
            for d in 0..7 {
                let day = week_start + CDuration::days(d);
                let v = value(day);
                if day <= today {
                    total += v as u64;
                    if v > 60 {
                        days_used += 1;
                    }
                    if v > best.0 {
                        best = (v, day);
                    }
                }
                let what = if pc_mode { "PC on" } else { "active" };
                let tip = if v < 60 {
                    format!("{} · no activity", day.format("%a %-d %b"))
                } else {
                    format!("{} · {} {what}", day.format("%a %-d %b"), dur(v))
                };
                cells.push(HeatCell { level: level(v), tip: s(tip), future: day > today });
            }
        }
        // streak: consecutive days (ending today, or yesterday if today is still empty) with > 15 min
        let mut streak = 0;
        let mut d = if value(today) > 900 { today } else { today - CDuration::days(1) };
        while value(d) > 900 {
            streak += 1;
            d -= CDuration::days(1);
        }
        b.set_heat(ModelRc::new(VecModel::from(cells)));
        b.set_heat_months(ModelRc::new(VecModel::from(months)));
        b.set_hist_total(s(format!("{}h", total / 3600)));
        b.set_hist_avg(s(if days_used > 0 { dur((total / days_used as u64) as u32) } else { "–".into() }));
        b.set_hist_streak(s(format!("{streak} day{}", if streak == 1 { "" } else { "s" })));
        b.set_hist_best(s(if best.0 > 0 { format!("{} · {}", best.1.format("%-d %b"), dur(best.0)) } else { "–".into() }));
        let first = self.data.health.keys().next().copied();
        b.set_hist_note(s(if pc_mode {
            "From Windows logs".to_string()
        } else {
            match first {
                Some(f) => format!("Since {}", f.format("%-d %b")),
                None => "Since today".to_string(),
            }
        }));
    }
}
