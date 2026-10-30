//! Actions that pause the app borrow: screenshots and the workspace folder picker.

use super::{Shared, schedule_collapse, with_app};
use crate::{Bridge, capture, sys};
use slint::{ComponentHandle, SharedString, Timer};
use std::time::Duration;

/// Hide the island, grab all screens, show it again; encode + save off the UI thread.
pub fn take_screenshot(shared: &Shared) {
    {
        let mut a = shared.borrow_mut();
        a.hover_timer.stop();
        a.collapse_timer.stop();
        a.peek_timer.stop();
        if a.ui().global::<Bridge>().get_expanded() {
            a.collapse();
        }
        a.ui().global::<Bridge>().set_peeking(false);
        sys::set_visible(a.hwnd, false);
    }
    // give DWM a frame to remove the island from the screen
    Timer::single_shot(Duration::from_millis(120), || {
        let shot = capture::grab();
        with_app(|s| {
            let mut a = s.borrow_mut();
            a.resize(false);
            if !a.hidden {
                sys::set_visible(a.hwnd, true);
            }
        });
        let Some(shot) = shot else { return };
        std::thread::spawn(move || {
            let path = capture::save_and_copy(shot);
            let _ = slint::invoke_from_event_loop(move || {
                with_app(|s| {
                    let mut a = s.borrow_mut();
                    match path {
                        Some(p) => {
                            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            a.last_shot = Some(p);
                            a.peek(8, "Screenshot copied", &name, 4);
                        }
                        None => a.peek(8, "Screenshot copied", "Couldn't save the file", 4),
                    }
                });
            });
        });
    });
}

/// Folder picker for the "4 Terminals" workspace. The dialog is modal, so the app
/// must not stay borrowed while it is open.
pub fn pick_workspace(s: &Shared) {
    let hwnd = {
        let mut a = s.borrow_mut();
        a.hovering = true;
        a.hwnd
    };
    let picked = sys::pick_paths(hwnd, true).into_iter().next();
    let mut a = s.borrow_mut();
    if let Some(dir) = picked {
        a.data.settings.workspace_dir = dir.to_string_lossy().to_string();
        a.data.save();
        let ui = a.ui();
        ui.global::<Bridge>().set_workspace(SharedString::from(a.data.settings.workspace_dir.as_str()));
        a.toast("Workspace folder changed");
    }
    a.hovering = false;
    schedule_collapse(s, &mut a, 3000);
}
