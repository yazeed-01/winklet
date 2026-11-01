//! Finds installed games from Steam's own records, so the library fills up before a game
//! has ever been played.

use crate::fg;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct Found {
    pub key: String,
    pub name: String,
    pub launch: String,
    /// a program inside the game, for its icon
    pub exe: String,
    pub source: &'static str,
}

/// Every installed game Steam knows about. Runs off the UI thread (it walks folders).
pub fn installed() -> Vec<Found> {
    let mut out = Vec::new();
    steam(&mut out);
    let mut seen = HashSet::new();
    out.retain(|g| seen.insert(g.key.clone()));
    out
}

/// Proton, the Steam runtimes and redistributables show up as apps too.
fn is_tool(name: &str) -> bool {
    let n = name.to_lowercase();
    ["proton", "steam linux runtime", "steamworks", "redistributable", "soundtrack", "dedicated server", "sdk"].iter().any(|t| n.contains(t))
}

fn steam(out: &mut Vec<Found>) {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let roots = [
        home.join(".local/share/Steam"),
        home.join(".steam/steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
    ];
    let mut libs: Vec<PathBuf> = Vec::new();
    for root in roots.iter().filter(|r| r.is_dir()) {
        libs.push(root.clone());
        if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
            for line in vdf.lines() {
                let parts: Vec<&str> = line.split('"').collect();
                if parts.len() >= 4 && parts[1] == "path" {
                    libs.push(PathBuf::from(parts[3]));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    for lib in libs {
        let Ok(real) = lib.canonicalize() else { continue };
        if !seen.insert(real.clone()) {
            continue;
        }
        let apps = real.join("steamapps");
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
                exe: main_exe(&dir),
                source: "Steam",
            });
        }
    }
}

/// The biggest program file at the top of the game folder (for its icon); may be empty.
fn main_exe(dir: &Path) -> String {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe") || x == "x86_64"))
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}
