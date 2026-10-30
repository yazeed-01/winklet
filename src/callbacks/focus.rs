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
}
