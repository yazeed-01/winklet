//! Right-drag anywhere on the island to move it.

use super::{App, Shared, schedule_collapse};
use crate::{Bridge, sys};
use slint::{ComponentHandle, Timer, TimerMode};
use std::rc::Rc;
use std::time::Duration;
use windows::Win32::Foundation::HWND;

impl App {
    pub fn start_drag(shared: &Shared) {
        let mut a = shared.borrow_mut();
        if a.drag_from.is_some() || a.hwnd == HWND::default() {
            return;
        }
        a.drag_from = Some((sys::cursor_pos(), sys::window_anchor(a.hwnd)));
        let weak = Rc::downgrade(shared);
        a.drag_timer.start(TimerMode::Repeated, Duration::from_millis(15), move || {
            if let Some(s) = weak.upgrade() {
                App::drag_frame(&s);
            }
        });
    }

    pub fn drag_frame(shared: &Shared) {
        let mut a = shared.borrow_mut();
        let Some((c0, p0)) = a.drag_from else { return };
        let ui = a.ui();
        let b = ui.global::<Bridge>();
        if !sys::right_button_down() {
            a.drag_timer.stop();
            a.drag_from = None;
            if b.get_dragging() {
                a.data.save();
                // let the button release reach the UI first so it isn't taken as a right-click
                let w = a.ui.clone();
                Timer::single_shot(Duration::from_millis(150), move || {
                    if let Some(ui) = w.upgrade() {
                        ui.global::<Bridge>().set_dragging(false);
                    }
                });
                if !a.hovering {
                    schedule_collapse(shared, &mut a, 380);
                }
            }
            return;
        }
        let c = sys::cursor_pos();
        let (dx, dy) = (c.0 - c0.0, c.1 - c0.1);
        if !b.get_dragging() && dx.abs().max(dy.abs()) < 6 {
            return;
        }
        b.set_dragging(true);
        a.data.settings.island_pos = Some((p0.0 + dx, p0.1 + dy));
        a.place();
    }
}
