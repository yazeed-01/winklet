//! To-dos, calendar and alarms.

use crate::Bridge;
use crate::app::Shared;
use crate::store::{self, Alarm, Todo};
use chrono::{Datelike, NaiveDate};

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_todo_add, |a, text| {
        let (text, date, time) = store::parse_task(&text, None);
        a.data.todos.push(Todo { id: store::new_id(), text, done: false, date, time, notified: false });
        a.data.save();
        a.refresh_todos();
    });
    on!(b, app, on_cal_add, |a, text| {
        let day = a.cal_selected;
        let (text, date, time) = store::parse_task(&text, Some(day));
        a.data.todos.push(Todo { id: store::new_id(), text, done: false, date, time, notified: false });
        a.data.save();
        a.refresh_todos();
    });
    on!(b, app, on_todo_toggle, |a, id| {
        let mut done = None;
        if let Some(t) = a.data.todos.iter_mut().find(|t| t.id == id.as_str()) {
            t.done = !t.done;
            done = Some(t.done);
        }
        // count it for today's summary (survives "clear done")
        if let Some(done) = done {
            let n = a.data.tasks_done.entry(chrono::Local::now().date_naive()).or_default();
            *n = if done { *n + 1 } else { (*n - 1).max(0) };
        }
        a.data.save();
        a.refresh_todos();
    });
    on!(b, app, on_todo_delete, |a, id| {
        a.data.todos.retain(|t| t.id != id.as_str());
        a.data.save();
        a.refresh_todos();
    });
    on!(b, app, on_todo_clear_done, |a| {
        a.data.todos.retain(|t| !t.done);
        a.data.save();
        a.refresh_todos();
    });
    on!(b, app, on_cal_shift, |a, d| {
        let m = a.cal_month;
        let idx = m.year() * 12 + m.month0() as i32 + d;
        a.cal_month = NaiveDate::from_ymd_opt(idx.div_euclid(12), idx.rem_euclid(12) as u32 + 1, 1).unwrap_or(m);
        a.refresh_todos();
    });
    on!(b, app, on_cal_select, |a, date| {
        if let Ok(d) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
            a.cal_selected = d;
            if d.month() != a.cal_month.month() || d.year() != a.cal_month.year() {
                a.cal_month = d.with_day(1).unwrap();
            }
            a.refresh_todos();
        }
    });

    on!(b, app, on_alarm_add, |a, text| {
        let text = text.trim().to_string();
        let (time_part, label) = text.split_once(' ').unwrap_or((text.as_str(), ""));
        match store::parse_time(time_part) {
            Some(time) => {
                a.data.alarms.push(Alarm {
                    id: store::new_id(),
                    time,
                    label: if label.trim().is_empty() { "Alarm".into() } else { label.trim().into() },
                    enabled: true,
                    last_fired: None,
                });
                a.data.save();
                a.refresh_alarms();
            }
            None => a.toast("Start with a time, e.g. 07:30"),
        }
    });
    on!(b, app, on_alarm_toggle, |a, id| {
        if let Some(x) = a.data.alarms.iter_mut().find(|x| x.id == id.as_str()) {
            x.enabled = !x.enabled;
        }
        a.data.save();
        a.refresh_alarms();
    });
    on!(b, app, on_alarm_delete, |a, id| {
        a.data.alarms.retain(|x| x.id != id.as_str());
        a.data.save();
        a.refresh_alarms();
    });
}
