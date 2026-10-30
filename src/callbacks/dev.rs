//! Developer tools, screenshots and the port killer.

use crate::app::{Shared, defer, pick_workspace, take_screenshot, with_app};
use crate::{Bridge, ports, sys};
use slint::Timer;
use std::time::Duration;

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_ws_create, |a, name| { a.ws_create(&name); });
    on!(b, app, on_ws_select, |a, id| { a.ws_select(&id); });
    on!(b, app, on_ws_launch, |a, id| { a.ws_launch(&id); });
    on!(b, app, on_ws_delete, |a, id| { a.ws_delete(&id); });
    on!(b, app, on_ws_add, |a, t| { a.ws_add(vec![t.to_string()]); });
    on!(b, app, on_ws_remove_app, |a, i| { a.ws_remove_app(i); });
    b.on_ws_browse(|| defer(crate::app::ws_browse));
    on!(b, app, on_dev_action, |a, action| {
        match action.as_str() {
            "terminal" => {
                if !sys::open("wt.exe", "") {
                    sys::open("cmd.exe", "");
                }
            }
            "taskmgr" => {
                sys::open("taskmgr.exe", "");
            }
            "localhost" => {
                sys::open("http://localhost:3000", "");
            }
            "projects" => {
                let home = std::env::var("USERPROFILE").unwrap_or_default();
                let repos = format!("{home}\\source\\repos");
                let dir = if std::path::Path::new(&repos).exists() { repos } else { home };
                sys::open(&dir, "");
            }
            "wt4" => {
                // 2x2 grid of Windows Terminal panes, all starting in the workspace folder
                let d = a.data.settings.workspace_dir.clone();
                let args = format!(
                    "-d \"{d}\" ; split-pane -V -d \"{d}\" ; move-focus left ; split-pane -H -d \"{d}\" ; move-focus right ; split-pane -H -d \"{d}\""
                );
                if !sys::open("wt.exe", &args) {
                    a.toast("Windows Terminal not found");
                }
            }
            "wt4-folder" => defer(pick_workspace),
            "envvars" => {
                sys::open("rundll32.exe", "sysdm.cpl,EditEnvironmentVariables");
            }
            "screenshot" => defer(take_screenshot),
            "snip" => {
                sys::open("ms-screenclip:", "");
            }
            "hosts" => {
                sys::open("notepad.exe", r"C:\Windows\System32\drivers\etc\hosts");
            }
            _ => {}
        }
    });

    b.on_screenshot(|| defer(take_screenshot));
    b.on_snip(|| {
        sys::open("ms-screenclip:", "");
    });
    on!(b, app, on_open_last_shot, |a| {
        if let Some(p) = &a.last_shot {
            sys::open(&p.to_string_lossy(), "");
        }
    });
    b.on_pick_workspace(|| defer(pick_workspace));

    on!(b, app, on_dev_view_changed, |a, v| {
        if v == 1 {
            a.refresh_ports();
        }
        if v == 2 {
            a.ignition_sweep();
        }
        if v == 3 {
            a.refresh_workspaces();
        }
    });
    on!(b, app, on_ports_refresh, |a| { a.refresh_ports(); });
    on!(b, app, on_ports_toggle_system, |a| {
        a.ports_system = !a.ports_system;
        a.refresh_ports();
    });
    on!(b, app, on_port_kill, |a, pid, port| {
        let name = ports::listeners().into_iter().find(|l| l.pid == pid as u32).map(|l| l.name).unwrap_or_default();
        match ports::kill(pid as u32) {
            Ok(()) => {
                a.toast(&format!("Freed port {port} ({name})"));
                // give Windows a moment to release the socket
                Timer::single_shot(Duration::from_millis(400), || with_app(|s| s.borrow().refresh_ports()));
            }
            Err(e) => a.toast(&format!("Can't stop {name}: {e}")),
        }
    });
}
