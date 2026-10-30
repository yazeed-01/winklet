//! The app in front: which program owns the foreground window, its friendly name,
//! its icon, and whether it looks like a game. Only the program is looked at,
//! never what's inside its windows or what you type.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW,
    GetObjectW, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, ReleaseDC,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, EnumChildWindows, GetClassNameW, GetForegroundWindow, GetIconInfo, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, ICONINFO,
};
use windows::core::{BOOL, HSTRING, PWSTR, w};

pub struct Fg {
    pub hwnd: HWND,
    pub pid: u32,
    /// full path of the program
    pub path: String,
    /// lower-case file name, e.g. "chrome.exe"
    pub exe: String,
    /// the window covers its whole monitor (fullscreen or borderless)
    pub fullscreen: bool,
}

impl Fg {
    /// The lock screen counts as nobody at the PC.
    pub fn is_lock(&self) -> bool {
        matches!(self.exe.as_str(), "lockapp.exe" | "logonui.exe") || self.exe.ends_with(".scr")
    }
}

thread_local! {
    static LAST: RefCell<(u32, String)> = const { RefCell::new((0, String::new())) };
    static NAMES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// The program that owns the foreground window (`None` for the desktop, the island itself, or nothing).
pub fn foreground(ours: HWND) -> Option<Fg> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || hwnd == ours {
            return None;
        }
        let class = class_name(hwnd);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let mut path = exe_path(pid)?;
        // Store apps live in a frame window owned by ApplicationFrameHost; the real app is a child.
        if path.to_lowercase().ends_with("\\applicationframehost.exe") {
            let mut found = (pid, 0u32);
            let _ = EnumChildWindows(Some(hwnd), Some(child_pid), LPARAM(&mut found as *mut (u32, u32) as isize));
            if found.1 != 0 {
                pid = found.1;
                path = exe_path(pid)?;
            }
        }
        let exe = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        Some(Fg { hwnd, pid, exe, fullscreen: covers_monitor(hwnd), path })
    }
}

unsafe extern "system" fn child_pid(child: HWND, lp: LPARAM) -> BOOL {
    unsafe {
        let found = &mut *(lp.0 as *mut (u32, u32));
        let mut pid = 0u32;
        GetWindowThreadProcessId(child, Some(&mut pid));
        if pid != 0 && pid != found.0 {
            found.1 = pid;
            return BOOL(0);
        }
        BOOL(1)
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn covers_monitor(hwnd: HWND) -> bool {
    unsafe {
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

/// Full path of a process (cached for the last pid asked about).
fn exe_path(pid: u32) -> Option<String> {
    if let Some(p) = LAST.with(|l| {
        let l = l.borrow();
        (l.0 == pid).then(|| l.1.clone())
    }) {
        return Some(p);
    }
    let path = unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        String::from_utf16_lossy(&buf[..len as usize])
    };
    LAST.with(|l| *l.borrow_mut() = (pid, path.clone()));
    Some(path)
}

/// Programs with a normal window open right now (for picking workspace apps), as (name, path).
pub fn open_apps(ours: HWND) -> Vec<(String, String)> {
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, IsWindowVisible, WS_EX_TOOLWINDOW,
    };
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        unsafe {
            let list = &mut *(lp.0 as *mut Vec<HWND>);
            let tool = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0;
            let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid());
            if IsWindowVisible(hwnd).as_bool() && !tool && !owned && !window_title(hwnd).is_empty() {
                list.push(hwnd);
            }
            BOOL(1)
        }
    }
    let mut wins: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut wins as *mut Vec<HWND> as isize));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for h in wins {
        if h == ours {
            continue;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let Some(path) = exe_path(pid) else { continue };
        let lower = path.to_lowercase();
        let exe = Path::new(&lower).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if matches!(exe.as_str(), "winklet.exe" | "applicationframehost.exe" | "textinputhost.exe" | "shellexperiencehost.exe")
            || !seen.insert(lower)
        {
            continue;
        }
        out.push((app_name(&path), path));
    }
    out.sort_by_key(|a| a.0.to_lowercase());
    out
}

/// Still running?
pub fn alive(pid: u32) -> bool {
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE;
        let _ = CloseHandle(h);
        ok
    }
}

pub fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// "Google Chrome" for chrome.exe: the file description from the program's version info,
/// falling back to the product name and then the file name. Cached.
pub fn app_name(path: &str) -> String {
    if let Some(n) = NAMES.with(|m| m.borrow().get(path).cloned()) {
        return n;
    }
    let name = version_string(path, "FileDescription")
        .or_else(|| version_string(path, "ProductName"))
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

fn version_string(path: &str, field: &str) -> Option<String> {
    unsafe {
        let file = HSTRING::from(path);
        let size = GetFileVersionInfoSizeW(&file, None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(&file, None, size, buf.as_mut_ptr() as *mut _).ok()?;
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let mut lang = "040904b0".to_string();
        if VerQueryValueW(buf.as_ptr() as *const _, w!("\\VarFileInfo\\Translation"), &mut p, &mut len).as_bool() && len >= 4 {
            let t = p as *const u16;
            lang = format!("{:04x}{:04x}", *t, *t.add(1));
        }
        let sub = HSTRING::from(format!("\\StringFileInfo\\{lang}\\{field}"));
        if !VerQueryValueW(buf.as_ptr() as *const _, &sub, &mut p, &mut len).as_bool() || len <= 1 || p.is_null() {
            return None;
        }
        let s = std::slice::from_raw_parts(p as *const u16, len as usize);
        Some(String::from_utf16_lossy(s).trim_end_matches('\0').trim().to_string())
    }
}

/// The program's 32 px icon as (width, height, RGBA pixels).
pub fn icon_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let mut sfi = SHFILEINFOW::default();
        let r = SHGetFileInfoW(
            &HSTRING::from(path),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || sfi.hIcon.is_invalid() {
            return None;
        }
        let mut info = ICONINFO::default();
        let got = GetIconInfo(sfi.hIcon, &mut info).is_ok();
        let _ = DestroyIcon(sfi.hIcon);
        if !got {
            return None;
        }
        let out = (|| {
            let mut bm = BITMAP::default();
            if info.hbmColor.is_invalid()
                || GetObjectW(HGDIOBJ(info.hbmColor.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0
            {
                return None;
            }
            let (w, h) = (bm.bmWidth.max(1) as u32, bm.bmHeight.max(1) as u32);
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = GetDC(None);
            let mut px = vec![0u8; (w * h * 4) as usize];
            let lines = GetDIBits(dc, info.hbmColor, 0, h, Some(px.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
            // old icons have no alpha channel: use the mask (black = opaque) instead
            let has_alpha = px.chunks(4).any(|p| p[3] != 0);
            if lines > 0 && !has_alpha && !info.hbmMask.is_invalid() {
                let mut mask = vec![0u8; (w * h * 4) as usize];
                if GetDIBits(dc, info.hbmMask, 0, h, Some(mask.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS) > 0 {
                    for (p, m) in px.chunks_mut(4).zip(mask.chunks(4)) {
                        p[3] = if m[0] == 0 { 255 } else { 0 };
                    }
                }
            }
            ReleaseDC(None, dc);
            if lines == 0 {
                return None;
            }
            for p in px.chunks_mut(4) {
                p.swap(0, 2); // BGRA -> RGBA
            }
            Some((w, h, px))
        })();
        let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        out
    }
}

// ------------------------------------------------------------------ games

/// Programs that are never games, even when fullscreen (browsers, players, tools, launchers).
const NOT_GAMES: &[&str] = &[
    "chrome", "msedge", "firefox", "brave", "opera", "opera_gx", "vivaldi", "arc", "iexplore", "zen",
    "vlc", "mpc-hc", "mpc-hc64", "mpc-be64", "potplayer", "potplayermini64", "mpv", "wmplayer", "video.ui",
    "microsoft.media.player", "powerpnt", "winword", "excel", "onenote", "outlook", "olk", "ms-teams", "teams",
    "zoom", "slack", "discord", "code", "cursor", "devenv", "idea64", "rider64", "pycharm64", "studio64",
    "windowsterminal", "cmd", "powershell", "pwsh", "conhost", "explorer", "searchhost", "shellexperiencehost",
    "startmenuexperiencehost", "textinputhost", "mstsc", "msrdc", "taskmgr", "systemsettings", "snippingtool",
    "steam", "steamwebhelper", "epicgameslauncher", "battle.net", "riotclientservices", "riotclientux",
    "eadesktop", "upc", "galaxyclient", "obs64", "winklet",
];

/// Helper programs that live inside game folders.
const HELPERS: &[&str] = &[
    "crash", "unins", "setup", "redist", "helper", "updater", "bootstrap", "launcher", "easyanticheat", "battleye",
    "report", "install",
];

/// Folders that only hold games. The folder right after the marker is the game.
const LIBRARIES: &[&str] = &[
    "\\steamapps\\common\\",
    "\\epic games\\",
    "\\xboxgames\\",
    "\\riot games\\",
    "\\gog galaxy\\games\\",
    "\\gog games\\",
    "\\ea games\\",
    "\\ubisoft game launcher\\games\\",
];

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

/// Is the program in front a game? `known` = programs Windows itself has flagged as games.
pub fn classify(f: &Fg, known: &HashSet<String>) -> Option<GameHit> {
    if is_not_game(&f.exe) || f.is_lock() {
        return None;
    }
    let lower = f.path.to_lowercase();
    for &marker in LIBRARIES {
        let Some(i) = lower.find(marker) else { continue };
        let rest = &f.path[i + marker.len()..];
        let folder = rest.split('\\').next().unwrap_or("");
        if folder.is_empty() || folder.eq_ignore_ascii_case("launcher") || folder.eq_ignore_ascii_case("riot client") {
            return None;
        }
        let key = lower[..i + marker.len() + folder.len()].to_string();
        let (mut name, mut launch) = (clean(folder), f.path.clone());
        if marker == "\\steamapps\\common\\" {
            if let Some((n, id)) = steam_manifest(&f.path[..i + "\\steamapps".len()], folder) {
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

/// (name, appid) from the Steam library's appmanifest_*.acf whose installdir is `folder`.
fn steam_manifest(steamapps: &str, folder: &str) -> Option<(String, String)> {
    for entry in std::fs::read_dir(steamapps).ok()?.flatten() {
        let fname = entry.file_name().to_string_lossy().to_lowercase();
        if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        let (mut id, mut name, mut dir) = (None, None, None);
        for line in text.lines() {
            // "key"		"value"
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

/// Program paths Windows' own game detection has flagged (HKCU\System\GameConfigStore\Children).
pub fn windows_known_games() -> HashSet<String> {
    let mut out = HashSet::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, w!("System\\GameConfigStore\\Children"), None, KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0..4096 {
            let mut name = [0u16; 128];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            let sub = HSTRING::from_wide(&name[..len as usize]);
            let mut buf = [0u16; 1024];
            let mut size = (buf.len() * 2) as u32;
            if RegGetValueW(
                key,
                &sub,
                w!("MatchedExeFullPath"),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
            .is_ok()
            {
                let n = (size as usize / 2).saturating_sub(1);
                out.insert(String::from_utf16_lossy(&buf[..n]).to_lowercase());
            }
        }
        let _ = RegCloseKey(key);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fg(path: &str, fullscreen: bool) -> Fg {
        let exe = Path::new(path).file_name().unwrap().to_string_lossy().to_lowercase();
        Fg { hwnd: HWND::default(), pid: 1, path: path.into(), exe, fullscreen }
    }

    #[test]
    fn library_games_use_their_folder() {
        let none = HashSet::new();
        let g = classify(&fg(r"C:\Riot Games\VALORANT\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe", false), &none).unwrap();
        assert_eq!(g.name, "VALORANT");
        assert_eq!(g.key, r"c:\riot games\valorant");
        let g = classify(&fg(r"D:\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe", false), &none).unwrap();
        assert_eq!(g.name, "Fortnite");
    }

    #[test]
    fn apps_and_launchers_are_not_games() {
        let none = HashSet::new();
        assert!(classify(&fg(r"C:\Program Files\Google\Chrome\Application\chrome.exe", true), &none).is_none());
        assert!(classify(&fg(r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe", false), &none).is_none());
        assert!(classify(&fg(r"C:\Riot Games\Riot Client\RiotClientServices.exe", false), &none).is_none());
        assert!(classify(&fg(r"C:\Tools\notepad.exe", false), &none).is_none());
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean("ELDEN RING™  "), "ELDEN RING");
        assert_eq!(stem_name(r"C:\x\my-game.exe"), "My-game");
    }
}

#[cfg(test)]
mod probe {
    /// `cargo test probe -- --ignored --nocapture` prints what this PC reports.
    #[test]
    #[ignore = "reads this machine's windows, GPU and controllers"]
    fn probe_this_pc() {
        let f = super::foreground(windows::Win32::Foundation::HWND::default()).expect("a foreground window");
        println!("front: {} ({}) fullscreen={} name={:?}", f.exe, f.pid, f.fullscreen, super::app_name(&f.path));
        println!("icon: {:?}", super::icon_rgba(&f.path).map(|(w, h, _)| (w, h)));
        println!("alive: {}", super::alive(f.pid));
        let known = super::windows_known_games();
        println!("windows-known games: {}", known.len());
        println!("explorer: {}", super::app_name(r"C:\Windows\explorer.exe"));
        println!("gpu: {:?} usage {:?}", crate::gpu_info::main_adapter(), { crate::gpu::usage(); std::thread::sleep(std::time::Duration::from_millis(500)); crate::gpu::usage() });
        println!("pad: {:?}", { let p = crate::pad::Pads::default().poll(); (p.connected, p.battery) });
        crate::fps::start();
        std::thread::sleep(std::time::Duration::from_millis(800));
        println!("fps trace state: {}", crate::fps::state());
        crate::fps::stop();
        println!("rtss: {:?}", crate::fps::rtss(f.pid));
    }
}
