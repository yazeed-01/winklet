//! Shelf: add, open, copy, reveal and remove items.

use crate::app::{Shared, defer, schedule_collapse};
use crate::{Bridge, sys};

pub fn wire(b: &Bridge, app: &Shared) {
    b.on_shelf_add(|folders| {
        // The dialog is modal and pumps messages, so the app must not stay borrowed while it is open.
        defer(move |s| {
            let hwnd = {
                let mut a = s.borrow_mut();
                a.hovering = true;
                a.collapse_timer.stop();
                a.hwnd
            };
            let paths = sys::pick_paths(hwnd, folders);
            let mut a = s.borrow_mut();
            let n = a.shelf_add(paths);
            if n > 0 {
                a.toast(&format!("Added {n} to shelf"));
            }
            a.hovering = false;
            schedule_collapse(s, &mut a, 3000);
        });
    });
    on!(b, app, on_shelf_open, |a, id| {
        if let Some(p) = a.shelf_path(&id) {
            if !std::path::Path::new(&p).exists() {
                a.toast("That file was moved or deleted");
            } else {
                sys::open(&p, "");
            }
        }
    });
    on!(b, app, on_shelf_copy, |a, id| {
        if let Some(p) = a.shelf_path(&id) {
            if sys::set_clipboard(&p) {
                a.toast("Path copied");
            }
        }
    });
    on!(b, app, on_shelf_reveal, |a, id| {
        if let Some(p) = a.shelf_path(&id) {
            sys::open("explorer.exe", &format!("/select,\"{p}\""));
        }
    });
    on!(b, app, on_shelf_remove, |a, id| {
        a.data.shelf.retain(|x| x.id != id.as_str());
        a.data.save();
        a.refresh_shelf();
    });
    on!(b, app, on_shelf_clear, |a| {
        a.data.shelf.clear();
        a.data.save();
        a.refresh_shelf();
    });
}
