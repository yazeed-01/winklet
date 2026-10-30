//! Activity history and stand-up reminders.

use crate::Bridge;
use crate::app::{Shared, defer, schedule_shrink};
use slint::ComponentHandle;
use std::time::{Duration, Instant};

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_health_view_changed, |a, v| {
        match v {
            1 => a.refresh_summary(),
            2 => a.refresh_screen(),
            3 => {
                a.reload_pc_on();
                a.refresh_history();
            }
            _ => {}
        }
    });
    on!(b, app, on_screen_shift, |a, d| { a.screen_shift(d); });
    on!(b, app, on_screen_pick, |a, key| {
        a.screen.sel = key.to_string();
        a.refresh_screen();
    });
    on!(b, app, on_summary_hour_adjust, |a, d| {
        a.data.settings.summary_h = (a.data.settings.summary_h as i32 + d).clamp(12, 23) as u32;
        a.data.save();
        a.refresh_summary_setting();
    });
    b.on_open_summary(|| defer(|s| s.borrow_mut().open_summary()));
    on!(b, app, on_history_mode_changed, |a, _m| { a.refresh_history(); });

    on!(b, app, on_stand_done, |a| {
        a.peek_timer.stop();
        a.ui().global::<Bridge>().set_peeking(false);
        a.record_break();
        a.peek(14, "Nice!", "Break logged · timer reset", 2);
    });
    on!(b, app, on_stand_snooze, |a| {
        // each snooze in a row is shorter: 10, 7, 4, 3 min
        let mins = 10u64.saturating_sub(3 * a.well.snoozes as u64).max(3);
        a.well.snooze_until = Some(Instant::now() + Duration::from_secs(mins * 60));
        a.well.snoozes += 1;
        a.well.pending_since = None;
        a.ui().global::<Bridge>().set_eye_mood(0);
        a.eye_end = None;
        a.peek(14, &format!("Snoozed {mins} min"), "I'll check back soon", 2);
    });
    b.on_stand_guide(|| {
        defer(|s| {
            let mut a = s.borrow_mut();
            if a.ui().global::<Bridge>().get_expanded() {
                a.collapse();
                schedule_shrink(s, &mut a);
            }
            a.start_guide();
        });
    });
    on!(b, app, on_stand_interval_adjust, |a, d| {
        a.data.settings.stand_interval_min = (a.data.settings.stand_interval_min + d * 5).clamp(20, 120);
        a.data.save();
        a.refresh_settings();
        a.refresh_health();
    });
}
