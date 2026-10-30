//! Finds installed games from the launchers' own records (Steam, Epic, GOG),
//! so the library fills up before a game has ever been played.

use crate::fg;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::core::{HSTRING, PWSTR};

pub struct Found {
    pub key: String,
    pub name: String,
    pub launch: String,
    /// a program inside the game, for its icon
    pub exe: String,
    pub source: &'static str,
}

/// Every installed game the launchers know about. Runs off the UI thread (it walks folders).
pub fn installed() -> Vec<Found> {
    let mut out = Vec::new();
    steam(&mut out);
    epic(&mut out);
    gog(&mut out);
    let mut seen = HashSet::new();
    out.retain(|g| seen.insert(g.key.clone()));
    out
}

fn steam(out: &mut Vec<Found>) {
    let Some(root) = reg_str(HKEY_CURRENT_USER, "Software\\Valve\\Steam", "SteamPath") else { return };
    let root = root.replace('/', "\\");
    let mut libs = vec![root.clone()];
    if let Ok(vdf) = std::fs::read_to_string(Path::new(&root).join("steamapps").join("libraryfolders.vdf")) {
        for line in vdf.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 4 && parts[1] == "path" {
                libs.push(parts[3].replace("\\\\", "\\"));
            }
        }
    }
    let mut seen = HashSet::new();
    for lib in libs {
        if !seen.insert(lib.to_lowercase()) {
            continue;
        }
        let apps = Path::new(&lib).join("steamapps");
        let Ok(dir) = std::fs::read_dir(&apps) else { continue };
        for entry in dir.flatten() {
            let fname = entry.file_name().to_string_lossy().to_lowercase();
            if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
            let (mut id, mut name, mut folder) = (None, None, None);
            for line in text.lines() {
                let parts: Vec<&str> = line.split('"').collect();
                if parts.len() < 4 {
                    continue;
                }
                match parts[1] {
                    "appid" if id.is_none() => id = Some(parts[3].to_string()),
                    "name" if name.is_none() => name = Some(parts[3].to_string()),
                    "installdir" if folder.is_none() => folder = Some(parts[3].to_string()),
                    _ => {}
                }
            }
            let (Some(id), Some(name), Some(folder)) = (id, name, folder) else { continue };
            if is_tool(&name) || id == "228980" {
                continue;
            }
            let dir = apps.join("common").join(&folder);
            if !dir.is_dir() {
                continue;
            }
            out.push(Found {
                key: fg::game_key(&dir.to_string_lossy()),
                name: fg::clean(&name),
                launch: format!("steam://rungameid/{id}"),
                exe: main_exe(&dir, &folder),
                source: "Steam",
            });
        }
    }
}

fn epic(out: &mut Vec<Found>) {
    let Ok(dir) = std::fs::read_dir("C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests") else { return };
    for entry in dir.flatten() {
        if !entry.file_name().to_string_lossy().to_lowercase().ends_with(".item") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let (name, loc, exe) = (get("DisplayName"), get("InstallLocation"), get("LaunchExecutable"));
        // DLC and add-ons carry their parent's id
        let is_app = v.get("AppCategories").and_then(|c| c.as_array()).is_none_or(|c| c.iter().any(|x| x.as_str() == Some("games")));
        if name.is_empty() || loc.is_empty() || !is_app || !Path::new(&loc).is_dir() {
            continue;
        }
        let exe_path = if exe.is_empty() { String::new() } else { Path::new(&loc).join(&exe).to_string_lossy().to_string() };
        let launch = format!(
            "com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true",
            get("CatalogNamespace"),
            get("CatalogItemId"),
            get("AppName")
        );
        out.push(Found { key: fg::game_key(&loc), name: fg::clean(&name), launch, exe: exe_path, source: "Epic" });
    }
}

fn gog(out: &mut Vec<Found>) {
    for ids in sub_keys(HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\GOG.com\\Games") {
        let k = format!("SOFTWARE\\WOW6432Node\\GOG.com\\Games\\{ids}");
        let (Some(name), Some(path)) = (reg_str(HKEY_LOCAL_MACHINE, &k, "gameName"), reg_str(HKEY_LOCAL_MACHINE, &k, "path")) else {
            continue;
        };
        if !Path::new(&path).is_dir() {
            continue;
        }
        let exe = reg_str(HKEY_LOCAL_MACHINE, &k, "exe").unwrap_or_default();
        let launch = if exe.is_empty() { path.clone() } else { exe.clone() };
        out.push(Found { key: fg::game_key(&path), name: fg::clean(&name), launch, exe, source: "GOG" });
    }
}

/// Redistributables, runtimes and other non-games Steam installs.
fn is_tool(name: &str) -> bool {
    let n = name.to_lowercase();
    ["redistributable", "steam linux runtime", "proton", "steamvr", "soundtrack", "dedicated server", "sdk"]
        .iter()
        .any(|t| n.contains(t))
}

/// Best guess at a game's main program: an .exe named like the folder, else the biggest one.
fn main_exe(dir: &Path, folder: &str) -> String {
    let want: String = folder.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let mut best: Option<(bool, u64, PathBuf)> = None;
    let mut stack = vec![(dir.to_path_buf(), 0)];
    let mut budget = 4000;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            budget -= 1;
            if budget == 0 {
                break;
            }
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let fname = e.file_name().to_string_lossy().to_lowercase();
            if !fname.ends_with(".exe") || fg::is_not_game(&fname) {
                continue;
            }
            let stem: String = fname.trim_end_matches(".exe").chars().filter(|c| c.is_alphanumeric()).collect();
            let named = !stem.is_empty() && (want.contains(&stem) || stem.contains(&want));
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if best.as_ref().is_none_or(|b| (named, size) > (b.0, b.1)) {
                best = Some((named, size, p));
            }
        }
        if budget == 0 {
            break;
        }
    }
    best.map(|b| b.2.to_string_lossy().to_string()).unwrap_or_default()
}

fn reg_str(root: HKEY, sub: &str, value: &str) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(root, &HSTRING::from(sub), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size))
            .ok()
            .ok()?;
    }
    let n = (size as usize / 2).saturating_sub(1);
    let s = String::from_utf16_lossy(&buf[..n]);
    (!s.is_empty()).then_some(s)
}

fn sub_keys(root: HKEY, path: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0..1024 {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = RegCloseKey(key);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "reads this PC's launchers"]
    fn probe_this_pc() {
        for g in super::installed() {
            println!("{} | {} | {} | {} | {}", g.source, g.name, g.key, g.launch, g.exe);
        }
    }
}
