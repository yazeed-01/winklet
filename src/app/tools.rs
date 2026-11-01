//! Actions that pause the app borrow: screenshots and the workspace folder picker.

use super::{Shared, schedule_collapse, with_app};
use crate::{Bridge, capture, clip, snip, sys};
use slint::{ComponentHandle, SharedString, Timer};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

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
                a.repaint_all();
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

/// Bumped by every new snip so an older one still waiting gives up.
static SNIP_GEN: AtomicU32 = AtomicU32::new(0);

/// Opens the Snipping Tool's area picker, then uses the snip it puts on the clipboard:
/// `lens` searches it with Google Lens, otherwise its text replaces it on the clipboard.
pub fn snip_for(shared: &Shared, lens: bool) {
    let before = clip::seq();
    {
        let mut a = shared.borrow_mut();
        a.hover_timer.stop();
        a.collapse_timer.stop();
        if a.ui().global::<Bridge>().get_expanded() {
            a.collapse();
        }
        if !sys::open("ms-screenclip:", "") {
            a.toast("Snipping Tool isn't installed");
            return;
        }
    }
    let gen_now = SNIP_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    std::thread::spawn(move || {
        // up to a minute to pick the area; Esc in the picker leaves the clipboard alone
        let deadline = Instant::now() + Duration::from_secs(60);
        let picture = loop {
            std::thread::sleep(Duration::from_millis(200));
            if SNIP_GEN.load(Ordering::Relaxed) != gen_now || Instant::now() > deadline {
                return;
            }
            if clip::seq() != before {
                // the Snipping Tool may still be writing the picture
                std::thread::sleep(Duration::from_millis(150));
                match clip::read_image() {
                    Some(p) => break p,
                    None => return,
                }
            }
        };
        let (w, h, rgba) = picture;
        let (title, sub) = if lens {
            if snip::lens(w, h, &rgba) {
                ("Searching with Google Lens".to_string(), "Opening your browser".to_string())
            } else {
                ("Couldn't open Google Lens".to_string(), String::new())
            }
        } else {
            match snip::ocr(w, h, &rgba) {
                Ok(t) if !t.is_empty() && sys::set_clipboard(&t) => {
                    let first = t.lines().next().unwrap_or_default();
                    let sub = if first.chars().count() > 40 { format!("{}…", first.chars().take(40).collect::<String>()) } else { first.to_string() };
                    ("Text copied".to_string(), sub)
                }
                Ok(_) => ("No text found".to_string(), "Try a bigger or sharper area".to_string()),
                Err(_) => ("Couldn't read text".to_string(), "Add an OCR language in Windows Settings".to_string()),
            }
        };
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|s| s.borrow_mut().peek(8, &title, &sub, 4));
        });
    });
}

/// Folder for a Dev tools row ("projects" / "downloads"): the one picked, else a good guess.
/// Empty when there is nothing sensible to guess.
pub fn folder_for(app: &super::App, kind: &str) -> String {
    let st = &app.data.settings;
    let set = if kind == "projects" { &st.projects_dir } else { &st.downloads_dir };
    if !set.is_empty() && std::path::Path::new(set).is_dir() {
        return set.clone();
    }
    if kind == "downloads" {
        return sys::downloads_dir();
    }
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    ["source\\repos", "Projects", "Documents\\GitHub", "Documents\\Projects", "repos", "dev", "code", "src"]
        .iter()
        .map(|d| format!("{home}\\{d}"))
        .find(|d| std::path::Path::new(d).is_dir())
        .unwrap_or_default()
}

/// Opens a Dev tools folder, asking where it is first when it isn't known.
pub fn open_folder(s: &Shared, kind: &'static str) {
    let dir = folder_for(&s.borrow(), kind);
    if dir.is_empty() {
        pick_folder(s, kind, true);
    } else {
        sys::open(&dir, "");
    }
}

/// Asks where the projects / Downloads folder is. The dialog is modal, so the app
/// must not stay borrowed while it is open.
pub fn pick_folder(s: &Shared, kind: &'static str, open_after: bool) {
    let hwnd = {
        let mut a = s.borrow_mut();
        a.hovering = true;
        a.collapse_timer.stop();
        a.hwnd
    };
    let title = if kind == "projects" { "Where is your projects folder?" } else { "Where is your Downloads folder?" };
    let picked = sys::pick_paths_titled(hwnd, true, title).into_iter().next();
    let mut a = s.borrow_mut();
    if let Some(dir) = picked {
        let dir = dir.to_string_lossy().to_string();
        if kind == "projects" {
            a.data.settings.projects_dir = dir.clone();
        } else {
            a.data.settings.downloads_dir = dir.clone();
        }
        a.data.save();
        a.refresh_dev();
        a.toast(if kind == "projects" { "Projects folder saved" } else { "Downloads folder saved" });
        if open_after {
            sys::open(&dir, "");
        }
    }
    a.hovering = false;
    schedule_collapse(s, &mut a, 3000);
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
