//! Moving the island: right-drag it anywhere (the panel closes and the pill follows the
//! pointer), or "Move" in settings (it follows the pointer until a click drops it). Near a
//! screen edge it docks; on the left or right edge the pill stands upright.

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
        let c = sys::cursor_pos();
        a.drag_from = Some(c);
        App::run_drag_timer(shared, &mut a);
    }

    /// Picks the island up: it follows the pointer until the next click.
    pub fn start_move(shared: &Shared) {
        let mut a = shared.borrow_mut();
        if a.drag_from.is_some() || a.hwnd == HWND::default() {
            return;
        }
        a.collapse();
        a.collapse_timer.stop();
        a.shrink_timer.stop();
        a.moving = true;
        a.move_pressed = sys::left_button_down();
        let c = sys::cursor_pos();
        a.drag_from = Some(c);
        a.ui().global::<Bridge>().set_dragging(true);
        a.resize(false);
        App::run_drag_timer(shared, &mut a);
    }

    fn run_drag_timer(shared: &Shared, a: &mut App) {
        let weak = Rc::downgrade(shared);
        a.drag_timer.start(TimerMode::Repeated, Duration::from_millis(15), move || {
            if let Some(s) = weak.upgrade() {
                App::drag_frame(&s);
            }
        });
    }

    pub fn drag_frame(shared: &Shared) {
        let mut a = shared.borrow_mut();
        let Some(c0) = a.drag_from else { return };
        let ui = a.ui();
        let b = ui.global::<Bridge>();
        let c = sys::cursor_pos();
        if a.moving {
            let down = sys::left_button_down() || sys::right_button_down();
            if down {
                a.move_pressed = true;
            }
            if !(a.move_pressed && !down) && !sys::escape_down() {
                a.data.settings.island_pos = Some(c);
                a.place();
                return;
            }
            a.moving = false;
            App::end_drag(shared, &mut a);
            return;
        }
        if !sys::right_button_down() {
            if b.get_dragging() {
                App::end_drag(shared, &mut a);
            } else {
                a.drag_timer.stop();
                a.drag_from = None;
            }
            return;
        }
        let (dx, dy) = (c.0 - c0.0, c.1 - c0.1);
        if !b.get_dragging() {
            if dx.abs().max(dy.abs()) < 6 {
                return;
            }
            // picked up: like "Move", the panel closes and the pill rides under the pointer
            if b.get_expanded() || b.get_peeking() {
                b.set_peeking(false);
                a.collapse();
            }
            a.hover_timer.stop();
            a.collapse_timer.stop();
            a.shrink_timer.stop();
            b.set_dragging(true);
            a.resize(false);
        }
        a.data.settings.island_pos = Some(c);
        a.place();
    }

    fn end_drag(shared: &Shared, a: &mut App) {
        a.drag_timer.stop();
        a.drag_from = None;
        a.data.save();
        // let the button release reach the UI first so it isn't taken as a click
        let w = a.ui.clone();
        Timer::single_shot(Duration::from_millis(200), move || {
            if let Some(ui) = w.upgrade() {
                ui.global::<Bridge>().set_dragging(false);
            }
        });
        if !a.hovering {
            schedule_collapse(shared, a, 380);
        }
    }
}
