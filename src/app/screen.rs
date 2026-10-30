//! Screen time per app: which program is in front, for how long, and each time it was used.
//! Also the 2 s "who's in front" tick that feeds game detection and game-mode stats.

use super::{App, TAB_HEALTH, s};
use crate::{AppUseItem, Bridge, SpanItem, fg, store, sys};
use chrono::{Duration as CDuration, Local, NaiveDate, Timelike};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Health tab views
pub const VIEW_SUMMARY: i32 = 1;
pub const VIEW_APPS: i32 = 2;
pub const VIEW_HISTORY: i32 = 3;

/// Back within this many seconds = the same sitting with the app.
const MERGE_GAP_S: u32 = 120;
const MAX_SPANS: usize = 40;

pub struct ScreenTrack {
    pub log: store::Screen,
    pub unsaved_s: u32,
    /// day shown in the Apps and Summary views
    pub day: NaiveDate,
    /// selected app (file name key)
    pub sel: String,
    pub icons: HashMap<String, Option<Image>>,
    pub last: Option<Instant>,
    pub refreshed: Option<Instant>,
}

impl ScreenTrack {
    pub fn load() -> Self {
        Self {
            log: store::Screen::load(),
            unsaved_s: 0,
            day: Local::now().date_naive(),
            sel: String::new(),
            icons: HashMap::new(),
            last: None,
            refreshed: None,
        }
    }
}

/// "2h 05m", "14m", "<1m"
pub fn fmt_dur(secs: u32) -> String {
    let m = secs / 60;
    if m >= 60 {
        format!("{}h {:02}m", m / 60, m % 60)
    } else if m > 0 {
        format!("{m}m")
    } else {
        "<1m".into()
    }
}

/// Extends the last span when the app came back soon, else starts a new one.
/// Keeps at most MAX_SPANS by joining the two closest.
pub fn add_span(spans: &mut Vec<[u32; 2]>, start: u32, end: u32) {
    match spans.last_mut() {
        Some(last) if start <= last[1] + MERGE_GAP_S && start >= last[0] => last[1] = last[1].max(end),
        _ => spans.push([start, end]),
    }
    if spans.len() > MAX_SPANS {
        let i = (1..spans.len()).min_by_key(|&i| spans[i][0].saturating_sub(spans[i - 1][1])).unwrap_or(1);
        spans[i - 1][1] = spans[i][1];
        spans.remove(i);
    }
}

pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Today".into()
    } else if day == today - CDuration::days(1) {
        "Yesterday".into()
    } else {
        day.format("%a %-d %b").to_string()
    }
}

impl App {
    /// Every 2 s: who's in front → screen time, game detection, game-mode stats.
    pub fn front_tick(&mut self) {
        let dt = self.screen.last.map(|t| t.elapsed().as_secs() as u32).unwrap_or(2);
        self.screen.last = Some(Instant::now());
        let fg = fg::foreground(self.hwnd);
        let pad = self.poll_pad();
        self.game_tick(fg.as_ref(), dt);
        // a longer gap means the PC was asleep
        if dt <= 90 {
            if let Some(f) = &fg {
                if !f.is_lock() && (sys::idle_seconds() < 60 || pad || f.fullscreen) {
                    self.screen_add(f, dt.min(10));
                }
            }
        }
        if self.data.settings.game_mode {
            self.game_stats_tick(fg.as_ref());
        }
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let stale = self.screen.refreshed.is_none_or(|t| t.elapsed() > Duration::from_secs(10));
        if b.get_expanded() && b.get_tab() == TAB_HEALTH && stale {
            match b.get_health_view() {
                VIEW_APPS => self.refresh_screen(),
                VIEW_SUMMARY => self.refresh_summary(),
                _ => {}
            }
        }
    }

    fn screen_add(&mut self, f: &fg::Fg, dt: u32) {
        let now = Local::now();
        let end = now.num_seconds_from_midnight();
        let name = match &self.play.run {
            Some(r) if r.pid == f.pid => r.name.clone(),
            _ => fg::app_name(&f.path),
        };
        let e = self.screen.log.days.entry(now.date_naive()).or_default().entry(f.exe.clone()).or_default();
        if e.name != name {
            e.name = name;
        }
        if e.path != f.path {
            e.path = f.path.clone();
        }
        e.secs += dt;
        add_span(&mut e.spans, end.saturating_sub(dt), end);
        self.screen.unsaved_s += dt;
        if self.screen.unsaved_s >= 300 {
            self.screen.unsaved_s = 0;
            self.screen.log.save();
        }
    }

    /// The program's icon, cached by path.
    pub fn icon_for(&mut self, path: &str) -> Image {
        if path.is_empty() {
            return Image::default();
        }
        self.screen
            .icons
            .entry(path.to_string())
            .or_insert_with(|| {
                fg::icon_rgba(path).map(|(w, h, px)| {
                    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&px, w, h))
                })
            })
            .clone()
            .unwrap_or_default()
    }

    pub fn clock_at(&self, sec: u32) -> String {
        let (h, m) = (sec / 3600 % 24, sec / 60 % 60);
        if self.data.settings.clock_24h {
            format!("{h:02}:{m:02}")
        } else {
            format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
        }
    }

    /// Apps of `day`, most used first.
    pub fn apps_of(&self, day: NaiveDate) -> Vec<(String, store::AppUse)> {
        let mut apps: Vec<_> =
            self.screen.log.days.get(&day).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
        apps.sort_by(|a, b| b.1.secs.cmp(&a.1.secs));
        apps
    }

    pub fn app_item(&mut self, key: &str, u: &store::AppUse, max: u32) -> AppUseItem {
        let range = match (u.spans.first(), u.spans.last()) {
            (Some(a), Some(b)) => format!("{} – {}", self.clock_at(a[0]), self.clock_at(b[1])),
            _ => String::new(),
        };
        AppUseItem {
            key: s(key),
            name: s(u.name.clone()),
            time: s(fmt_dur(u.secs)),
            sessions: u.spans.len() as i32,
            frac: u.secs as f32 / max.max(1) as f32,
            range: s(range),
            icon: self.icon_for(&u.path),
        }
    }

    pub fn refresh_screen(&mut self) {
        self.screen.refreshed = Some(Instant::now());
        let today = Local::now().date_naive();
        let day = self.screen.day;
        let apps = self.apps_of(day);
        let total: u32 = apps.iter().map(|a| a.1.secs).sum();
        let max = apps.first().map(|a| a.1.secs).unwrap_or(1);
        let items: Vec<AppUseItem> = apps.iter().map(|(k, u)| self.app_item(k, u, max)).collect();
        if !apps.iter().any(|(k, _)| *k == self.screen.sel) {
            self.screen.sel = apps.first().map(|a| a.0.clone()).unwrap_or_default();
        }
        let sel = apps.iter().find(|(k, _)| *k == self.screen.sel).cloned();
        let (sel_item, spans) = match &sel {
            Some((k, u)) => {
                let item = self.app_item(k, u, max);
                let spans: Vec<SpanItem> = u
                    .spans
                    .iter()
                    .rev()
                    .map(|sp| SpanItem {
                        start: sp[0] as f32 / 86400.0,
                        end: sp[1] as f32 / 86400.0,
                        text: s(format!("{} – {}", self.clock_at(sp[0]), self.clock_at(sp[1]))),
                        len: s(fmt_dur(sp[1].saturating_sub(sp[0]))),
                    })
                    .collect();
                (item, spans)
            }
            None => (AppUseItem::default(), Vec::new()),
        };
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_screen_day_label(s(day_label(day, today)));
        b.set_screen_can_next(day < today);
        b.set_screen_total(s(if total > 0 { format!("{} on screen · {} apps", fmt_dur(total), apps.len()) } else { String::new() }));
        b.set_screen_apps(ModelRc::new(VecModel::from(items)));
        b.set_screen_selected(s(self.screen.sel.clone()));
        b.set_screen_sel(sel_item);
        b.set_screen_spans(ModelRc::new(VecModel::from(spans)));
    }

    pub fn screen_shift(&mut self, d: i32) {
        let today = Local::now().date_naive();
        let day = self.screen.day + CDuration::days(d as i64);
        self.screen.day = day.min(today);
        self.refresh_screen();
        self.refresh_summary();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_merge_when_close() {
        let mut v = Vec::new();
        add_span(&mut v, 100, 102);
        add_span(&mut v, 102, 104);
        add_span(&mut v, 200, 202); // 96 s gap: same sitting
        assert_eq!(v, vec![[100, 202]]);
        add_span(&mut v, 1000, 1002);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn spans_are_capped() {
        let mut v = Vec::new();
        for i in 0..100u32 {
            add_span(&mut v, i * 1000, i * 1000 + 10);
        }
        assert_eq!(v.len(), MAX_SPANS);
        assert_eq!(v[0][0], 0);
        assert_eq!(v.last().unwrap()[1], 99 * 1000 + 10);
    }
}
