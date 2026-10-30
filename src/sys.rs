//! Thin wrappers over the Win32 APIs Winklet needs. Everything here is cheap
//! enough to call from a 1 s timer.

use std::cell::Cell;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, FILETIME, GetLastError, HANDLE, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::System::SystemInformation::{GetTickCount, GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, GetSystemTimes};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO, VK_RBUTTON};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetCursorPos, GetWindowLongPtrW, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_NOACTIVATE,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{HSTRING, PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const APP_NAME: PCWSTR = w!("Winklet");
const OLD_APP_NAME: PCWSTR = w!("DyIs");

pub fn hwnd_of(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// Keeps the island out of the taskbar and Alt+Tab.
pub fn make_tool_window(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
    }
}

/// Screen edge the island sits on.
pub const EDGE_TOP: i32 = 0;
pub const EDGE_BOTTOM: i32 = 1;
pub const EDGE_LEFT: i32 = 2;
pub const EDGE_RIGHT: i32 = 3;
pub const EDGE_FLOAT: i32 = 4;

/// (monitor rect, work area) of the monitor holding `p`, or the primary one.
fn monitor_rects(p: Option<(i32, i32)>) -> (RECT, RECT) {
    unsafe {
        let mon = match p {
            Some((x, y)) => MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST),
            None => MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY),
        };
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        (mi.rcMonitor, mi.rcWork)
    }
}

/// Where the pill's centre sits at the top centre of the primary screen (physical px).
pub fn default_anchor(scale: f32) -> (i32, i32) {
    let (m, _) = monitor_rects(None);
    ((m.left + m.right) / 2, m.top + (17.0 * scale) as i32)
}

/// Which edge the island docks to when its centre is at `anchor`: the closest edge
/// within reach, else it floats. `None` = top centre.
pub fn island_edge(anchor: Option<(i32, i32)>, scale: f32) -> i32 {
    let Some((cx, cy)) = anchor else { return EDGE_TOP };
    let (_, wk) = monitor_rects(anchor);
    let reach = (44.0 * scale) as i32;
    [(cy - wk.top, EDGE_TOP), (wk.bottom - cy, EDGE_BOTTOM), (cx - wk.left, EDGE_LEFT), (wk.right - cx, EDGE_RIGHT)]
        .into_iter()
        .filter(|(d, _)| *d < reach)
        .min_by_key(|(d, _)| *d)
        .map(|(_, e)| e)
        .unwrap_or(EDGE_FLOAT)
}

/// Places the island window (size in physical px) flush against `edge`, centred on the
/// anchor along that edge (top and bottom snap to the middle when close). Floating, the
/// pill's centre sits on the anchor and the panel opens downwards.
pub fn place_island(hwnd: HWND, w: i32, h: i32, anchor: Option<(i32, i32)>, edge: i32, scale: f32) {
    let (cx, cy) = anchor.unwrap_or_else(|| default_anchor(scale));
    let (m, wk) = monitor_rects(Some((cx, cy)));
    let mid = (m.left + m.right) / 2;
    let snap = (28.0 * scale) as i32;
    let ccx = if (cx - mid).abs() < snap { mid } else { cx };
    let (x, y) = match edge {
        EDGE_TOP => (ccx - w / 2, m.top),
        EDGE_BOTTOM => (ccx - w / 2, wk.bottom - h),
        EDGE_LEFT => (wk.left, cy - h / 2),
        EDGE_RIGHT => (wk.right - w, cy - h / 2),
        _ => (cx - w / 2, cy - (17.0 * scale) as i32),
    };
    let x = x.clamp(wk.left, (wk.right - w).max(wk.left));
    let y = if edge == EDGE_TOP { y } else { y.clamp(wk.top, (wk.bottom - h).max(wk.top)) };
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

pub fn left_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_LBUTTON;
    unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000 != 0 }
}

pub fn escape_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
    unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000 != 0 }
}

pub fn right_button_down() -> bool {
    unsafe { GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000 != 0 }
}

pub fn set_visible(hwnd: HWND, visible: bool) {
    unsafe {
        let _ = ShowWindow(hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

pub const FS_NONE: i32 = 0;
/// a fullscreen window (borderless game, video, browser) or a presentation
pub const FS_WINDOW: i32 = 1;
/// exclusive fullscreen (Direct3D): any window on top would knock it out of fullscreen
pub const FS_EXCLUSIVE: i32 = 2;

/// What owns the screen right now.
pub fn fullscreen_state() -> i32 {
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) if s == QUNS_RUNNING_D3D_FULL_SCREEN => FS_EXCLUSIVE,
        Ok(s) if s == QUNS_BUSY || s == QUNS_PRESENTATION_MODE => FS_WINDOW,
        _ => FS_NONE,
    }
}

/// (percent or -1 when there is no battery, plugged in)
pub fn battery() -> (i32, bool) {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() || s.BatteryFlag & 128 != 0 || s.BatteryLifePercent > 100 {
        return (-1, false);
    }
    (s.BatteryLifePercent as i32, s.ACLineStatus == 1)
}

/// Battery time left in seconds, when Windows has an estimate (on battery only).
pub fn battery_secs_left() -> Option<u32> {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() || s.BatteryLifeTime == u32::MAX || s.BatteryLifeTime == 0 {
        return None;
    }
    Some(s.BatteryLifeTime)
}

/// Puts the island back on top of the topmost band (fullscreen games can cover it).
pub fn keep_on_top(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOMOVE, SWP_NOSIZE};
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub fn idle_seconds() -> u32 {
    let mut li = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if GetLastInputInfo(&mut li).as_bool() { GetTickCount().wrapping_sub(li.dwTime) / 1000 } else { 0 }
    }
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

thread_local! {
    static LAST_CPU: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    /// (tick ms, value) of the last CPU reading
    static CPU_READING: Cell<(u64, f32)> = const { Cell::new((0, 0.0)) };
}

/// System-wide CPU usage (0..1) since the previous call.
pub fn cpu_usage() -> f32 {
    // Several views ask for this (Dev, Dash, game stats). Each call measures since the
    // previous one, so two calls close together saw almost no time pass and read 0%:
    // keep the last reading until at least half a second has gone by.
    let now = unsafe { GetTickCount64() };
    let (at, last) = CPU_READING.with(|c| c.get());
    if at != 0 && now.saturating_sub(at) < 500 {
        return last;
    }
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
        return last;
    }
    let (idle, total) = (ft(idle), ft(kernel) + ft(user)); // kernel time includes idle
    let (pi, pt) = LAST_CPU.with(|c| c.replace((idle, total)));
    let (di, dt) = (idle.saturating_sub(pi), total.saturating_sub(pt));
    if pt == 0 || dt == 0 {
        return last;
    }
    let v = (1.0 - di as f32 / dt as f32).clamp(0.0, 1.0);
    CPU_READING.with(|c| c.set((now, v)));
    v
}

/// (load 0..1, used GB, total GB)
pub fn memory() -> (f32, f64, f64) {
    let mut m = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
        return (0.0, 0.0, 0.0);
    }
    let gb = |b: u64| b as f64 / 1024f64.powi(3);
    (m.dwMemoryLoad as f32 / 100.0, gb(m.ullTotalPhys - m.ullAvailPhys), gb(m.ullTotalPhys))
}

pub fn uptime() -> String {
    let s = unsafe { GetTickCount64() } / 1000;
    let (d, h, m) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    if d > 0 { format!("{d}d {h}h") } else { format!("{h}h {m}m") }
}

pub fn local_ip() -> String {
    // connect() on UDP sends nothing; it just picks the outgoing interface.
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("1.1.1.1:80").map(|_| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "offline".into())
}

pub fn set_clipboard(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let ok = (|| -> windows::core::Result<()> {
            EmptyClipboard()?;
            let h = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
            let p = GlobalLock(h) as *mut u16;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(h.0)))?;
            Ok(())
        })()
        .is_ok();
        let _ = CloseClipboard();
        ok
    }
}

pub fn open(target: &str, args: &str) -> bool {
    let (t, a) = (HSTRING::from(target), HSTRING::from(args));
    unsafe { ShellExecuteW(None, w!("open"), &t, &a, PCWSTR::null(), SW_SHOWNORMAL).0 as isize > 32 }
}

/// Whether an app handles links like `spotify:…` (HKEY_CLASSES_ROOT\<scheme>).
pub fn protocol_registered(scheme: &str) -> bool {
    use windows::Win32::System::Registry::{HKEY, HKEY_CLASSES_ROOT, KEY_READ, RegCloseKey, RegOpenKeyExW};
    let mut key = HKEY::default();
    let found = unsafe { RegOpenKeyExW(HKEY_CLASSES_ROOT, &HSTRING::from(scheme), None, KEY_READ, &mut key) }.is_ok();
    if found {
        let _ = unsafe { RegCloseKey(key) };
    }
    found
}

pub fn foreground() -> HWND {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }
}

/// Hands keyboard focus back to the app the user was in before clicking the island.
pub fn restore_foreground(ours: HWND, prev: HWND) {
    unsafe {
        if !prev.is_invalid() && foreground() == ours {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(prev);
        }
    }
}

pub fn play_sound(alias: &str) {
    let a = HSTRING::from(alias);
    unsafe {
        let _ = PlaySoundW(&a, None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT);
    }
}

/// The app used to be called DyIs: carry a "start with Windows" choice over to the new name.
pub fn migrate_autostart() {
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, OLD_APP_NAME, RRF_RT_REG_SZ, None, None, None).is_ok() {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, OLD_APP_NAME);
            set_autostart(true);
        }
    }
}

pub fn autostart_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, APP_NAME, RRF_RT_REG_SZ, None, None, None).is_ok() }
}

pub fn set_autostart(on: bool) {
    unsafe {
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            let cmd: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                APP_NAME,
                REG_SZ.0,
                Some(cmd.as_ptr() as *const _),
                (cmd.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, APP_NAME);
        }
    }
}

/// Returns false if another Winklet is already running.
pub fn single_instance() -> bool {
    unsafe {
        // The handle is intentionally leaked: it lives as long as the process.
        let _ = CreateMutexW(None, true, w!("Local\\Winklet.SingleInstance"));
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Runs the island above normal priority so its open / close animations stay smooth
/// while other programs keep the CPU busy (it is idle almost all the time, so this costs nothing).
pub fn boost_priority() {
    use windows::Win32::System::Threading::{ABOVE_NORMAL_PRIORITY_CLASS, SetPriorityClass};
    unsafe {
        let _ = SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS);
    }
}

/// Hands unused pages back to Windows while the island is just sitting there.
pub fn trim_memory() {
    unsafe {
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }
}

/// Native "Open" dialog. `folders` switches it into folder-picking mode. Multi-select.
pub fn pick_paths(owner: HWND, folders: bool) -> Vec<std::path::PathBuf> {
    pick_paths_titled(owner, folders, if folders { "Add folders to the shelf" } else { "Add files to the shelf" })
}

/// Same picker with its own title.
pub fn pick_paths_titled(owner: HWND, folders: bool, title: &str) -> Vec<std::path::PathBuf> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::{
        FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
        SIGDN_FILESYSPATH,
    };
    let run = || -> windows::core::Result<Vec<std::path::PathBuf>> {
        unsafe {
            let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let mut opts = dlg.GetOptions()? | FOS_ALLOWMULTISELECT | FOS_FORCEFILESYSTEM;
            if folders {
                opts |= FOS_PICKFOLDERS;
            }
            dlg.SetOptions(opts)?;
            dlg.SetTitle(&HSTRING::from(title))?;
            dlg.Show(Some(owner))?; // Err when cancelled
            let items = dlg.GetResults()?;
            let mut out = Vec::new();
            for i in 0..items.GetCount()? {
                let name = items.GetItemAt(i)?.GetDisplayName(SIGDN_FILESYSPATH)?;
                out.push(std::path::PathBuf::from(name.to_string().unwrap_or_default()));
                CoTaskMemFree(Some(name.0 as *const _));
            }
            Ok(out)
        }
    };
    run().unwrap_or_default()
}

pub const HOTKEY_SCREENSHOT: i32 = 1;
pub const HOTKEY_MIC: i32 = 2;
pub const HOTKEY_ISLAND: i32 = 3;
pub const HOTKEY_GEAR_UP: i32 = 4;
pub const HOTKEY_GEAR_DOWN: i32 = 5;
pub const HOTKEY_GAME: i32 = 6;

/// Global hotkeys on a dedicated thread that sleeps in GetMessage (0% CPU).
/// Ctrl+Alt+S screenshot, Ctrl+Alt+M mic mute, Ctrl+Alt+Space open the island,
/// Ctrl+Alt+PgUp / PgDn shift up / down. (Not Ctrl+Alt+digits: that's AltGr on many layouts.)
pub fn spawn_hotkeys(on_hotkey: impl Fn(i32) + Send + 'static) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, VK_NEXT, VK_PRIOR, VK_SPACE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
    std::thread::Builder::new()
        .name("hotkeys".into())
        .stack_size(64 * 1024)
        .spawn(move || unsafe {
            let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
            let _ = RegisterHotKey(None, HOTKEY_SCREENSHOT, mods, b'S' as u32);
            let _ = RegisterHotKey(None, HOTKEY_MIC, mods, b'M' as u32);
            let _ = RegisterHotKey(None, HOTKEY_ISLAND, mods, VK_SPACE.0 as u32);
            let _ = RegisterHotKey(None, HOTKEY_GEAR_UP, mods, VK_PRIOR.0 as u32);
            let _ = RegisterHotKey(None, HOTKEY_GEAR_DOWN, mods, VK_NEXT.0 as u32);
            let _ = RegisterHotKey(None, HOTKEY_GAME, mods, b'G' as u32);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_HOTKEY {
                    on_hotkey(msg.wParam.0 as i32);
                }
            }
        })
        .expect("spawn hotkey thread");
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    Efficiency,
    Balanced,
    Performance,
}

/// Sets the Windows 10/11 power mode slider (Settings > System > Power).
/// Uses `PowerSetActiveOverlayScheme` from powrprof.dll. It's undocumented, so it's
/// looked up at runtime and a missing export just returns false. No admin needed.
pub fn set_power_mode(mode: PowerMode) -> bool {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{GUID, s};
    let guid = match mode {
        PowerMode::Efficiency => GUID::from_u128(0x961cc777_2547_4f9d_8174_7d86181b8a7a),
        PowerMode::Balanced => GUID::zeroed(),
        PowerMode::Performance => GUID::from_u128(0xded574b5_45a0_4f42_8737_46345c09c238),
    };
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("powrprof.dll")) else { return false };
        let Some(f) = GetProcAddress(lib, s!("PowerSetActiveOverlayScheme")) else { return false };
        let set: unsafe extern "system" fn(GUID) -> u32 = std::mem::transmute(f);
        set(guid) == 0
    }
}

/// Total bytes received / sent by physical network adapters (Wi-Fi, Ethernet) since boot.
/// Virtual adapters (Hyper-V, WSL, VPN taps) are skipped so traffic isn't counted twice.
pub fn net_bytes() -> (u64, u64) {
    use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0);
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        let (mut rx, mut tx) = (0u64, 0u64);
        for r in rows {
            let hardware = r.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
            if hardware && r.OperStatus.0 == 1 {
                rx += r.InOctets;
                tx += r.OutOctets;
            }
        }
        FreeMibTable(table as *const _);
        (rx, tx)
    }
}