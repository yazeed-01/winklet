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
                sys::open_document(&p);
            }
        }
    });
    // ---- Send
    on!(b, app, on_shelf_view_changed, |a, _v| { a.refresh_send(); });
    // the file dialog is modal: open it after the click, unborrowed
    b.on_send_add(|folders| defer(move |s| crate::app::send_pick(s, folders)));
    on!(b, app, on_send_remove, |a, i| { a.send_remove(i.max(0) as usize); });
    on!(b, app, on_send_clear, |a| { a.send_clear(); });
    on!(b, app, on_send_start, |a| { a.send_start(); });
    on!(b, app, on_send_stop, |a| { a.send_stop(); });
    on!(b, app, on_send_copy_link, |a| {
        let url = a.send.server.as_ref().map(|s| s.url.clone()).unwrap_or_default();
        if !url.is_empty() && sys::set_clipboard(&url) {
            a.toast("Link copied");
        }
    });
    on!(b, app, on_send_open_inbox, |a| {
        let dir = crate::app::folder_for(&a, "downloads");
        sys::open(&dir, "");
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
