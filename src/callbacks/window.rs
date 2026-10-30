//! Hover, click, peek dismissal, tab switches and quit.

use crate::app::{Shared, TAB_AUDIO, TAB_DEV, schedule_collapse, trace};
use crate::{Bridge, sys};
use slint::{ComponentHandle, TimerMode};
use std::rc::Rc;
use std::time::{Duration, Instant};

pub fn wire(b: &Bridge, app: &Shared) {
    {
        let shared = app.clone();
        b.on_hover(move |inside| {
            trace(&format!("hover {inside}"));
            let mut a = shared.borrow_mut();
            a.hovering = inside;
            let peeking = a.ui().global::<Bridge>().get_peeking();
            if peeking {
                // keep notifications (and their buttons) put while hovered; click to open the island
                if inside {
                    a.peek_timer.stop();
                } else if a.ui().global::<Bridge>().get_peek_kind() != 12 {
                    a.arm_peek_timer(5);
                }
                return;
            }
            if a.drag_from.is_some() {
                return; // being moved: don't open
            }
            if inside {
                a.collapse_timer.stop();
                if !a.ui().global::<Bridge>().get_expanded() {
                    a.shrink_timer.stop();
                    a.resize(true);
                    let weak = Rc::downgrade(&shared);
                    a.hover_timer.start(TimerMode::SingleShot, Duration::from_millis(140), move || {
                        if let Some(s) = weak.upgrade() {
                            let mut a = s.borrow_mut();
                            if a.hovering {
                                a.expand();
                            }
                        }
                    });
                }
            } else {
                a.hover_timer.stop();
                if !a.ui().global::<Bridge>().get_expanded() && a.drag_from.is_none() {
                    // passed over without opening: back to the pill size
                    a.resize(false);
                    return;
                }
                schedule_collapse(&shared, &mut a, 380);
            }
        });
    }
    on!(b, app, on_clicked, |a| {
        let ui = a.ui();
        let b = ui.global::<Bridge>();
        if !b.get_expanded() && !b.get_dragging() && a.drag_from.is_none() {
            a.expand();
        }
    });
    {
        let shared = app.clone();
        b.on_dismiss_peek(move || {
            let mut a = shared.borrow_mut();
            a.well.guide_start = None;
            let ui = a.ui();
            let b = ui.global::<Bridge>();
            b.set_peeking(false);
            a.collapsed_at = Some(Instant::now());
            let weak = Rc::downgrade(&shared);
            a.shrink_timer.start(TimerMode::SingleShot, Duration::from_millis(560), move || {
                let Some(shared) = weak.upgrade() else { return };
                let mut a = shared.borrow_mut();
                let ui = a.ui();
                let b = ui.global::<Bridge>();
                if !b.get_expanded() && !b.get_peeking() {
                    a.resize(false);
                }
            });
        });
    }
    on!(b, app, on_tab_changed, |a, tab| {
        if tab == TAB_DEV {
            sys::cpu_usage(); // prime the delta
        }
        if tab != TAB_AUDIO {
            a.stop_mic_test();
        }
        a.refresh_tab(tab);
    });

    on!(b, app, on_quit, |a| {
        a.data.save();
        let _ = slint::quit_event_loop();
    });
}
