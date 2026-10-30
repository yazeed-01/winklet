//! Files dropped from Explorer go on the shelf; right-drag moves the island.

use crate::app::{App, TAB_SHELF, defer, schedule_collapse};
use crate::{AppWindow, Bridge};
use slint::ComponentHandle;

pub fn wire_file_drop(ui: &AppWindow) {
    use slint::winit_030::{
        EventResult, WinitWindowAccessor,
        winit::event::{MouseButton, WindowEvent},
    };
    ui.window().on_winit_window_event(|_, event| {
        match event {
            // hold the right button anywhere on the island and drag to move it
            WindowEvent::MouseInput { state, button: MouseButton::Right, .. } if state.is_pressed() => {
                defer(App::start_drag);
            }
            WindowEvent::HoveredFile(_) => defer(|s| {
                let mut a = s.borrow_mut();
                let ui = a.ui();
                let b = ui.global::<Bridge>();
                if !b.get_drop_hover() {
                    b.set_drop_hover(true);
                    if b.get_tab() != TAB_SHELF {
                        b.set_tab_dir(if b.get_tab() < TAB_SHELF { 1 } else { -1 });
                        b.set_tab(TAB_SHELF);
                    }
                    a.hovering = true;
                    a.collapse_timer.stop();
                    a.expand();
                }
            }),
            WindowEvent::DroppedFile(path) => {
                let path = path.clone();
                defer(move |s| {
                    let mut a = s.borrow_mut();
                    let ui = a.ui();
                    let b = ui.global::<Bridge>();
                    b.set_drop_hover(false);
                    // Shelf > Send collects files to send; the Shelf keeps shortcuts
                    if b.get_shelf_view() == 1 {
                        if a.send_add([path]) > 0 {
                            a.toast("Added to the basket");
                        }
                    } else if a.shelf_add([path]) > 0 {
                        a.toast("Added to shelf");
                    }
                    a.hovering = false;
                    schedule_collapse(s, &mut a, 2500);
                });
            }
            WindowEvent::HoveredFileCancelled => defer(|s| {
                let mut a = s.borrow_mut();
                a.ui().global::<Bridge>().set_drop_hover(false);
                a.hovering = false;
                schedule_collapse(s, &mut a, 800);
            }),
            _ => {}
        }
        EventResult::Propagate
    });
}
