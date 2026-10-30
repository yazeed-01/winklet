//! Developer → Workspaces: named sets of apps, files, folders and links that open
//! together with one button ("Coding": VS Code, Terminal, the repo folder, localhost).

use super::{App, Shared, s, schedule_collapse};
use crate::{Bridge, WsApp, WsItem, fg, store, sys};
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::path::Path;

pub const VIEW_WORKSPACES: i32 = 3;

/// A readable name for an item: the app's name, a file or folder name, or a link's site.
pub fn item_name(target: &str) -> String {
    if let Some(rest) = target.strip_prefix("https://").or_else(|| target.strip_prefix("http://")) {
        let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        return host.trim_start_matches("www.").to_string();
    }
    let p = Path::new(target);
    match p.extension().map(|e| e.to_string_lossy().to_lowercase()) {
        Some(e) if e == "exe" => fg::app_name(target),
        _ => p.file_stem().or(p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| target.to_string()),
    }
}

impl App {
    fn ws_icon(&mut self, target: &str) -> Image {
        if target.starts_with("http") { Image::default() } else { self.icon_for(target) }
    }

    pub fn refresh_workspaces(&mut self) {
        if self.dev_ws_sel.is_empty() || !self.data.workspaces.iter().any(|w| w.id == self.dev_ws_sel) {
            self.dev_ws_sel = self.data.workspaces.first().map(|w| w.id.clone()).unwrap_or_default();
        }
        let list = self.data.workspaces.clone();
        let items: Vec<WsItem> = list
            .iter()
            .map(|w| WsItem {
                id: s(w.id.clone()),
                name: s(w.name.clone()),
                count: w.apps.len() as i32,
                icons: ModelRc::new(VecModel::from(w.apps.iter().take(5).map(|a| self.ws_icon(&a.path)).collect::<Vec<_>>())),
            })
            .collect();
        let sel = list.iter().find(|w| w.id == self.dev_ws_sel).cloned();
        let apps: Vec<WsApp> = sel
            .as_ref()
            .map(|w| {
                w.apps
                    .iter()
                    .map(|a| WsApp { name: s(a.name.clone()), path: s(a.path.clone()), icon: self.ws_icon(&a.path) })
                    .collect()
            })
            .unwrap_or_default();
        // apps open right now that aren't in the workspace yet
        let have: Vec<String> = sel.as_ref().map(|w| w.apps.iter().map(|a| a.path.to_lowercase()).collect()).unwrap_or_default();
        let running: Vec<WsApp> = fg::open_apps(self.hwnd)
            .into_iter()
            .filter(|(_, p)| !have.contains(&p.to_lowercase()))
            .map(|(n, p)| WsApp { name: s(n), icon: self.icon_for(&p), path: s(p) })
            .collect();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_workspaces(ModelRc::new(VecModel::from(items)));
        b.set_ws_selected(s(self.dev_ws_sel.clone()));
        b.set_ws_name(s(sel.map(|w| w.name).unwrap_or_default()));
        b.set_ws_apps(ModelRc::new(VecModel::from(apps)));
        b.set_ws_running(ModelRc::new(VecModel::from(running)));
    }

    fn ws_mut(&mut self) -> Option<&mut store::Workspace> {
        let id = self.dev_ws_sel.clone();
        self.data.workspaces.iter_mut().find(|w| w.id == id)
    }

    pub fn ws_create(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let w = store::Workspace { id: store::new_id(), name: name.to_string(), apps: Vec::new() };
        self.dev_ws_sel = w.id.clone();
        self.data.workspaces.push(w);
        self.data.save();
        self.refresh_workspaces();
        self.toast("Now add its apps");
    }

    pub fn ws_select(&mut self, id: &str) {
        self.dev_ws_sel = id.to_string();
        self.refresh_workspaces();
    }

    /// Adds apps, files, folders or links to the selected workspace.
    pub fn ws_add(&mut self, targets: Vec<String>) {
        let Some(w) = self.ws_mut() else {
            return;
        };
        let mut added = 0;
        for t in targets {
            let t = t.trim().trim_matches('"').to_string();
            if t.is_empty() || w.apps.iter().any(|a| a.path.eq_ignore_ascii_case(&t)) {
                continue;
            }
            w.apps.push(store::WsApp { name: item_name(&t), path: t });
            added += 1;
        }
        if added > 0 {
            self.data.save();
        }
        self.refresh_workspaces();
    }

    pub fn ws_remove_app(&mut self, index: i32) {
        if let Some(w) = self.ws_mut() {
            if (0..w.apps.len() as i32).contains(&index) {
                w.apps.remove(index as usize);
            }
        }
        self.data.save();
        self.refresh_workspaces();
    }

    pub fn ws_delete(&mut self, id: &str) {
        self.data.workspaces.retain(|w| w.id != id);
        self.data.save();
        self.refresh_workspaces();
    }

    /// Opens everything in the workspace.
    pub fn ws_launch(&mut self, id: &str) {
        let Some(w) = self.data.workspaces.iter().find(|w| w.id == id).cloned() else { return };
        if w.apps.is_empty() {
            self.toast("Add some apps first");
            return;
        }
        let failed = w.apps.iter().filter(|a| !sys::open(&a.path, "")).count();
        let opened = w.apps.len() - failed;
        self.toast(&if failed == 0 { format!("{} · {opened} opened", w.name) } else { format!("{opened} opened, {failed} failed") });
    }
}

/// "Browse…": pick files or programs. The dialog is modal, so the app isn't borrowed meanwhile.
pub fn ws_browse(shared: &Shared) {
    let hwnd = {
        let mut a = shared.borrow_mut();
        a.hovering = true;
        a.hwnd
    };
    let picked = sys::pick_paths_titled(hwnd, false, "Add apps or files to the workspace");
    let mut a = shared.borrow_mut();
    a.ws_add(picked.into_iter().map(|p| p.to_string_lossy().to_string()).collect());
    a.hovering = false;
    schedule_collapse(shared, &mut a, 3000);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_items() {
        assert_eq!(item_name("https://www.github.com/me/repo"), "github.com");
        assert_eq!(item_name(r"C:\code\my-repo"), "my-repo");
        assert_eq!(item_name(r"C:\Users\me\Desktop\Notes.lnk"), "Notes");
    }
}
