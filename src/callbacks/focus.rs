//! Focus timer controls.

use crate::Bridge;
use crate::app::Shared;

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_focus_toggle, |a| {
        a.focus.running = !a.focus.running;
        a.refresh_focus();
    });
    on!(b, app, on_focus_reset, |a| {
        let is_break = a.focus.is_break;
        a.focus_reset_to(is_break);
        a.refresh_focus();
    });
    on!(b, app, on_focus_skip, |a| {
        let next = !a.focus.is_break;
        a.focus_reset_to(next);
        a.refresh_focus();
    });
    on!(b, app, on_focus_adjust, |a, d| {
        let st = &mut a.data.settings;
        st.focus_min = (st.focus_min + d * 5).clamp(5, 120);
        a.data.save();
        if !a.focus.is_break && !a.focus.running {
            a.focus_reset_to(false);
        }
        a.refresh_focus();
    });
    on!(b, app, on_break_adjust, |a, d| {
        let st = &mut a.data.settings;
        st.break_min = (st.break_min + d).clamp(1, 60);
        a.data.save();
        if a.focus.is_break && !a.focus.running {
            a.focus_reset_to(true);
        }
        a.refresh_focus();
    });

    on!(b, app, on_timer_toggle, |a| {
        a.focus.timer.running = !a.focus.timer.running;
        a.refresh_focus();
    });
    on!(b, app, on_timer_reset, |a| {
        let min = a.focus.timer.minutes;
        a.timer_reset_to(min);
        a.refresh_focus();
    });
    on!(b, app, on_timer_set, |a, min| {
        a.timer_reset_to(min.clamp(1, 180));
        a.focus.timer.running = true;
        a.refresh_focus();
    });
    on!(b, app, on_timer_adjust, |a, d| {
        let t = &mut a.focus.timer;
        if t.running || t.left != t.total {
            // mid-countdown: add or take whole minutes off what is left
            t.left = (t.left + d * 60).max(1);
            t.total = t.total.max(t.left);
        } else {
            let min = (t.minutes + d).clamp(1, 180);
            a.timer_reset_to(min);
        }
        a.refresh_focus();
    });

    on!(b, app, on_sw_toggle, |a| { a.sw_toggle(); });
    on!(b, app, on_sw_lap, |a| { a.sw_lap(); });
    on!(b, app, on_sw_reset, |a| { a.sw_reset(); });
}
