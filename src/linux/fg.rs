//! The app in front: which program owns the active window, its friendly name (from its
//! .desktop entry), and whether it looks like a game. Only the program is looked at,
//! never what's inside its windows or what you type.

use crate::sys::{self, HWND};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Fg {
    pub hwnd: HWND,
    pub pid: u32,
    /// full path of the program
    pub path: String,
    /// lower-case file name, e.g. "firefox"
    pub exe: String,
    /// the window is fullscreen
    pub fullscreen: bool,
}

impl Fg {
    /// The lock screen counts as nobody at the PC.
    pub fn is_lock(&self) -> bool {
        matches!(self.exe.as_str(), "gnome-screensaver" | "xscreensaver" | "i3lock" | "kscreenlocker_greet" | "light-locker" | "xsecurelock")
    }
}

thread_local! {
    static NAMES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// The program behind a process: a Windows game under Wine/Proton reports the .exe it runs
/// (its first argument, `Z:\…` paths turned back into Linux paths), anything else its binary.
fn exe_path(pid: u32) -> Option<String> {
    let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let first = cmd.split(|b| *b == 0).next().map(|a| String::from_utf8_lossy(a).into_owned()).unwrap_or_default();
    if first.to_lowercase().ends_with(".exe") {
        let unix = first.replace('\\', "/");
        let unix = unix.strip_prefix("Z:").or_else(|| unix.strip_prefix("z:")).unwrap_or(&unix).to_string();
        return Some(unix);
    }
    std::fs::read_link(format!("/proc/{pid}/exe")).ok().map(|p| p.to_string_lossy().into_owned())
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// The program that owns the active window (`None` for the desktop, the island itself, or nothing).
pub fn foreground(ours: HWND) -> Option<Fg> {
    let hwnd = sys::foreground();
    if hwnd.is_invalid() || hwnd == ours || sys::is_shell_window(hwnd) {
        return None;
    }
    let pid = sys::window_pid(hwnd)?;
    if pid == std::process::id() {
        return None;
    }
    let path = exe_path(pid)?;
    Some(Fg { hwnd, pid, exe: file_name(&path), fullscreen: sys::is_fullscreen(hwnd), path })
}

/// Programs with a normal window open right now (for picking workspace apps), as (name, path).
pub fn open_apps(ours: HWND) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for h in sys::client_windows() {
        if h == ours || sys::is_shell_window(h) || sys::window_title(h).is_empty() {
            continue;
        }
        let Some(pid) = sys::window_pid(h) else { continue };
        if pid == std::process::id() {
            continue;
        }
        let Some(path) = exe_path(pid) else { continue };
        if !seen.insert(path.clone()) {
            continue;
        }
        out.push((app_name(&path), path));
    }
    out.sort_by_key(|a| a.0.to_lowercase());
    out
}

/// Still running?
pub fn alive(pid: u32) -> bool {
    // a zombie has no exe link left
    Path::new(&format!("/proc/{pid}/exe")).exists()
}

pub fn window_title(hwnd: HWND) -> String {
    sys::window_title(hwnd)
}

/// Folders holding the desktop's app entries (.desktop files).
fn app_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    std::iter::once(data_home)
        .chain(data_dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
        .chain([PathBuf::from("/var/lib/flatpak/exports/share"), home.join(".local/share/flatpak/exports/share")])
        .map(|d| d.join("applications"))
        .collect()
}

/// The Name= of the .desktop entry that starts `stem` (its Exec program or StartupWMClass).
fn desktop_name(stem: &str) -> Option<String> {
    for dir in app_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let (mut name, mut hit, mut in_main) = (None, false, false);
            for line in text.lines() {
                if line.starts_with('[') {
                    in_main = line == "[Desktop Entry]";
                    continue;
                }
                if !in_main {
                    continue;
                }
                if let Some(n) = line.strip_prefix("Name=") {
                    name.get_or_insert_with(|| n.trim().to_string());
                } else if let Some(x) = line.strip_prefix("Exec=") {
                    let prog = x.split_whitespace().find(|t| !t.contains('=') && *t != "env").unwrap_or("");
                    hit |= Path::new(prog.trim_matches('"')).file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(stem));
                } else if let Some(c) = line.strip_prefix("StartupWMClass=") {
                    hit |= c.trim().eq_ignore_ascii_case(stem);
                }
            }
            if hit && name.is_some() {
                return name;
            }
        }
    }
    None
}

/// "Firefox" for /usr/lib/firefox/firefox: the app's .desktop name, else the file name. Cached.
pub fn app_name(path: &str) -> String {
    if let Some(n) = NAMES.with(|m| m.borrow().get(path).cloned()) {
        return n;
    }
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let name = if path.to_lowercase().ends_with(".exe") { None } else { desktop_name(&stem) }
        .map(|n| clean(&n))
        .filter(|n| !n.is_empty() && n.len() <= 48)
        .unwrap_or_else(|| stem_name(path));
    NAMES.with(|m| m.borrow_mut().insert(path.to_string(), name.clone()));
    name
}

/// "my-game.exe" → "My-game"
pub fn stem_name(path: &str) -> String {
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "Unknown".into(),
    }
}

/// Drops ™ ® © and extra spaces.
pub fn clean(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '™' | '®' | '©' | '\0')).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The program's icon: not looked up on Linux yet (the app shows its glyph instead).
pub fn icon_rgba(_path: &str) -> Option<(u32, u32, Vec<u8>)> {
    None
}

// ------------------------------------------------------------------ games

/// Programs that are never games, even when fullscreen (browsers, players, tools, launchers).
const NOT_GAMES: &[&str] = &[
    "chrome", "chromium", "google-chrome", "firefox", "firefox-bin", "brave", "opera", "vivaldi", "zen", "msedge",
    "vlc", "mpv", "totem", "celluloid", "smplayer", "libreoffice", "soffice", "zoom", "slack", "discord", "code",
    "cursor", "idea", "pycharm", "studio", "gnome-terminal-server", "konsole", "alacritty", "kitty", "wezterm",
    "xterm", "nautilus", "dolphin", "thunar", "nemo", "steam", "steamwebhelper", "heroic", "lutris", "obs", "winklet",
    "wine64-preloader", "wine-preloader", "wineserver", "explorer",
];

/// Helper programs that live inside game folders.
const HELPERS: &[&str] = &[
    "crash", "unins", "setup", "redist", "helper", "updater", "bootstrap", "launcher", "easyanticheat", "battleye",
    "report", "install",
];

/// Folders that only hold games. The folder right after the marker is the game.
const LIBRARIES: &[&str] = &["/steamapps/common/", "/games/heroic/", "/heroic/"];

pub struct GameHit {
    /// stable id: the game's folder (library games) or the program path
    pub key: String,
    pub name: String,
    /// how to start it again: steam://rungameid/… or the program path
    pub launch: String,
}

pub fn is_not_game(exe: &str) -> bool {
    let stem = exe.trim_end_matches(".exe");
    NOT_GAMES.contains(&stem) || HELPERS.iter().any(|h| stem.contains(h))
}

/// Is the program in front a game? `known` is unused on Linux (no system game list).
pub fn classify(f: &Fg, known: &HashSet<String>) -> Option<GameHit> {
    if is_not_game(&f.exe) || f.is_lock() {
        return None;
    }
    let lower = f.path.to_lowercase();
    for &marker in LIBRARIES {
        let Some(i) = lower.find(marker) else { continue };
        let rest = &f.path[i + marker.len()..];
        let folder = rest.split('/').next().unwrap_or("");
        let fl = folder.to_lowercase();
        // Proton and the Steam runtimes sit in the same library
        if folder.is_empty() || fl.starts_with("proton") || fl.starts_with("steamlinuxruntime") || fl.starts_with("steam linux runtime") {
            return None;
        }
        let key = lower[..i + marker.len() + folder.len()].to_string();
        let (mut name, mut launch) = (clean(folder), f.path.clone());
        if marker == "/steamapps/common/" {
            if let Some((n, id)) = steam_manifest(&f.path[..i + "/steamapps".len()], folder) {
                name = clean(&n);
                launch = format!("steam://rungameid/{id}");
            }
        }
        return Some(GameHit { key, name, launch });
    }
    if known.contains(&lower) || f.fullscreen {
        let title = clean(&window_title(f.hwnd));
        let name = if (2..=48).contains(&title.chars().count()) { title } else { app_name(&f.path) };
        return Some(GameHit { key: lower, name, launch: f.path.clone() });
    }
    None
}

/// Library key for a game folder or program: the game's folder when it sits in a
/// known game library (same key `classify` uses), otherwise the lowercased path.
pub fn game_key(path: &str) -> String {
    let lower = path.trim_end_matches('/').to_lowercase();
    for &marker in LIBRARIES {
        let Some(i) = lower.find(marker) else { continue };
        let folder = lower[i + marker.len()..].split('/').next().unwrap_or("");
        if !folder.is_empty() {
            return lower[..i + marker.len() + folder.len()].to_string();
        }
    }
    lower
}

/// (name, appid) from the Steam library's appmanifest_*.acf whose installdir is `folder`.
pub fn steam_manifest(steamapps: &str, folder: &str) -> Option<(String, String)> {
    for entry in std::fs::read_dir(steamapps).ok()?.flatten() {
        let fname = entry.file_name().to_string_lossy().to_lowercase();
        if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        let (mut id, mut name, mut dir) = (None, None, None);
        for line in text.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() < 4 {
                continue;
            }
            match parts[1] {
                "appid" if id.is_none() => id = Some(parts[3].to_string()),
                "name" if name.is_none() => name = Some(parts[3].to_string()),
                "installdir" if dir.is_none() => dir = Some(parts[3].to_string()),
                _ => {}
            }
        }
        if dir.is_some_and(|d| d.eq_ignore_ascii_case(folder)) {
            return Some((name?, id?));
        }
    }
    None
}

/// Windows' own game list has no Linux counterpart.
pub fn windows_known_games() -> HashSet<String> {
    HashSet::new()
}
