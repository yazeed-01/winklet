//! To-dos, the calendar and alarms: list refreshes plus the per-minute checks.

use super::{App, s};
use crate::store::Todo;
use crate::{AlarmItem, Bridge, DayCell, TodoItem, sys};
use chrono::{Datelike, Duration as CDuration, Local, Timelike};
use slint::{ComponentHandle, ModelRc, VecModel};

impl App {
    pub fn refresh_todos(&self) {
        let now = Local::now().naive_local();
        let today = now.date();
        let ui = self.ui();
        let b = ui.global::<Bridge>();

        let mut todos: Vec<&Todo> = self.data.todos.iter().collect();
        todos.sort_by_key(|t| (t.done, t.date.is_none(), t.date, t.time));

        let item = |t: &Todo| {
            let due = match (t.date, t.time) {
                (None, _) => String::new(),
                (Some(d), tm) => {
                    let day = if d == today {
                        "Today".to_string()
                    } else if Some(d) == today.succ_opt() {
                        "Tomorrow".to_string()
                    } else if Some(d) == today.pred_opt() {
                        "Yesterday".to_string()
                    } else {
                        d.format("%a %-d %b").to_string()
                    };
                    match tm {
                        Some(tm) => format!("{day} {}", tm.format("%H:%M")),
                        None => day,
                    }
                }
            };
            let overdue = !t.done && t.due().map(|d| d < now).unwrap_or(t.date.is_some_and(|d| d < today));
            TodoItem { id: s(t.id.clone()), text: s(t.text.clone()), done: t.done, due: s(due), overdue }
        };

        let items: Vec<TodoItem> = todos.iter().map(|t| item(t)).collect();
        b.set_todos(ModelRc::new(VecModel::from(items)));
        b.set_todo_open(self.data.todos.iter().filter(|t| !t.done).count() as i32);
        b.set_next_todo(s(todos.iter().find(|t| !t.done).map(|t| t.text.clone()).unwrap_or_default()));

        // calendar
        let first = self.cal_month;
        let start = first - CDuration::days(first.weekday().num_days_from_monday() as i64);
        let days: Vec<DayCell> = (0..42)
            .map(|i| {
                let d = start + CDuration::days(i);
                DayCell {
                    day: d.day() as i32,
                    date: s(d.to_string()),
                    in_month: d.month() == first.month(),
                    today: d == today,
                    selected: d == self.cal_selected,
                    has_items: self.data.todos.iter().any(|t| t.date == Some(d) && !t.done),
                    future: false,
                }
            })
            .collect();
        b.set_cal_days(ModelRc::new(VecModel::from(days)));
        b.set_cal_title(s(first.format("%B %Y").to_string()));
        let label = if self.cal_selected == today {
            "Today".to_string()
        } else {
            self.cal_selected.format("%A, %-d %B").to_string()
        };
        b.set_cal_selected_label(s(label));
        let day_items: Vec<TodoItem> = todos.iter().filter(|t| t.date == Some(self.cal_selected)).map(|t| item(t)).collect();
        b.set_cal_items(ModelRc::new(VecModel::from(day_items)));
    }

    pub fn refresh_alarms(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let mut alarms = self.data.alarms.clone();
        alarms.sort_by_key(|a| a.time);
        let now = Local::now().time();
        let next = alarms
            .iter()
            .filter(|a| a.enabled)
            .min_by_key(|a| if a.time > now { (0, a.time) } else { (1, a.time) })
            .map(|a| format!("{} · {}", a.time.format("%H:%M"), a.label))
            .unwrap_or_default();
        let items: Vec<AlarmItem> = alarms
            .iter()
            .map(|a| AlarmItem {
                id: s(a.id.clone()),
                time: s(a.time.format("%H:%M").to_string()),
                label: s(a.label.clone()),
                enabled: a.enabled,
            })
            .collect();
        b.set_alarms(ModelRc::new(VecModel::from(items)));
        b.set_next_alarm(s(next));
    }

    pub fn check_alarms(&mut self) {
        let now = Local::now();
        let (today, hm) = (now.date_naive(), (now.hour(), now.minute()));
        let mut fired = None;
        for a in self.data.alarms.iter_mut() {
            if a.enabled && (a.time.hour(), a.time.minute()) == hm && a.last_fired != Some(today) {
                a.last_fired = Some(today);
                fired = Some(a.clone());
            }
        }
        if let Some(a) = fired {
            self.data.save();
            sys::play_sound("Notification.Looping.Alarm");
            let title = if a.label.is_empty() { "Alarm".to_string() } else { a.label.clone() };
            self.peek(5, &title, &format!("It's {}", a.time.format("%H:%M")), 20);
        }
    }

    pub fn check_reminders(&mut self) {
        // in N and gear 6, task reminders wait until you shift down
        if !self.gear_allows(6) {
            return;
        }
        let now = Local::now().naive_local();
        let mut due = Vec::new();
        for t in self.data.todos.iter_mut().filter(|t| !t.done && !t.notified) {
            if let Some(d) = t.due() {
                if d <= now {
                    t.notified = true;
                    // don't replay reminders that went stale while the PC was off
                    if now - d < CDuration::hours(2) {
                        due.push(t.clone());
                    }
                }
            }
        }
        if !due.is_empty() || self.data.todos.iter().any(|t| t.notified && t.due().is_some_and(|d| d <= now)) {
            self.data.save();
        }
        if let Some(t) = due.first() {
            sys::play_sound("Notification.Reminder");
            let sub = format!("Reminder · {}", t.time.map(|x| x.format("%H:%M").to_string()).unwrap_or_default());
            self.peek(6, &t.text, &sub, 12);
            self.refresh_todos();
        }
    }
}
