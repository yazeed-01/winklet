//! Water tracking.

use crate::Bridge;
use crate::app::Shared;
use chrono::Local;
use std::time::Instant;

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_water_add, |a, ml| {
        *a.data.water.entry(Local::now().date_naive()).or_default() += ml;
        a.data.water_log.push(ml);
        a.last_drink = Instant::now();
        a.data.save();
        a.refresh_water();
        let (now, goal) = (a.data.water_today(), a.data.settings.water_goal_ml);
        if now >= goal && now - ml < goal {
            a.toast("Daily goal reached!");
            a.eye_event(10, 4.0);
        }
    });
    on!(b, app, on_water_undo, |a| {
        if let Some(ml) = a.data.water_log.pop() {
            let e = a.data.water.entry(Local::now().date_naive()).or_default();
            *e = (*e - ml).max(0);
            a.data.save();
            a.refresh_water();
        }
    });
    on!(b, app, on_water_goal_adjust, |a, d| {
        a.data.settings.water_goal_ml = (a.data.settings.water_goal_ml + d * 250).clamp(500, 6000);
        a.data.save();
        a.refresh_water();
    });
    on!(b, app, on_cup_adjust, |a, d| {
        a.data.settings.cup_ml = (a.data.settings.cup_ml + d * 50).clamp(50, 1000);
        a.data.save();
        a.refresh_water();
    });
}
