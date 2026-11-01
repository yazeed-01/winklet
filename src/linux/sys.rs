//! The Linux side of `sys`: the island's window through X11 (a Wayland session runs Winklet on
//! XWayland, where a window may place itself), and the system numbers from /proc and /sys.
//! Same functions and names as the Windows `sys.rs`, so the rest of the app doesn't care.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::screensaver::ConnectionExt as _;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, EventMask, KeyButMask, PropMode, StackMode, Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// An X11 window id (0 = none). Named like the Windows handle so the app code is shared.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct HWND(pub u32);

impl HWND {
    pub fn is_invalid(&self) -> bool {
        self.0 == 0
    }
}

struct X {
    conn: RustConnection,
    root: Window,
    atoms: Mutex<HashMap<&'static str, Atom>>,
}

/// The X server connection, made on first use (None without a display).
fn x() -> Option<&'static X> {
    static X_CONN: OnceLock<Option<X>> = OnceLock::new();
    X_CONN
        .get_or_init(|| {
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots[screen].root;
            Some(X { conn, root, atoms: Mutex::new(HashMap::new()) })
        })
        .as_ref()
}

impl X {
    fn atom(&self, name: &'static str) -> Atom {
        if let Some(a) = self.atoms.lock().unwrap().get(name) {
            return *a;
        }
        let a = self.conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom).unwrap_or(0);
        self.atoms.lock().unwrap().insert(name, a);
        a
    }

    fn prop32(&self, win: Window, name: &'static str) -> Vec<u32> {
        let atom = self.atom(name);
        self.conn
            .get_property(false, win, atom, AtomEnum::ANY, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    /// An EWMH request to the window manager about `win`.
    fn tell_wm(&self, win: Window, msg: &'static str, data: [u32; 5]) {
        let ev = ClientMessageEvent::new(32, win, self.atom(msg), data);
        let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev);
        let _ = self.conn.flush();
    }
}

thread_local! {
    static UI: RefCell<Option<slint::Weak<crate::AppWindow>>> = const { RefCell::new(None) };
}

/// The island's window, for the calls below that need Slint's side of it.
pub fn set_ui(ui: slint::Weak<crate::AppWindow>) {
    UI.with(|u| *u.borrow_mut() = Some(ui));
}

pub fn hwnd_of(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(HWND(h.window as u32)),
        RawWindowHandle::Xcb(h) => Some(HWND(h.window.get())),
        // native Wayland: no window id, but the window exists
        _ => Some(HWND(u32::MAX)),
    }
}

/// Keeps the island out of the taskbar and the window switcher, above other windows, on every desktop.
pub fn make_tool_window(hwnd: HWND) {
    let Some(x) = x() else { return };
    let states = ["_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER", "_NET_WM_STATE_ABOVE", "_NET_WM_STATE_STICKY"];
    // the property counts while the window is unmapped (set_visible maps it again next);
    // the messages count while it is mapped
    let atoms: Vec<u32> = states.iter().map(|s| x.atom(s)).collect();
    let _ = x.conn.change_property32(PropMode::REPLACE, hwnd.0, x.atom("_NET_WM_STATE"), AtomEnum::ATOM, &atoms);
    let _ = x.conn.change_property32(PropMode::REPLACE, hwnd.0, x.atom("_NET_WM_DESKTOP"), AtomEnum::CARDINAL, &[u32::MAX]);
    for a in atoms {
        x.tell_wm(hwnd.0, "_NET_WM_STATE", [1, a, 0, 1, 0]);
    }
    x.tell_wm(hwnd.0, "_NET_WM_DESKTOP", [u32::MAX, 1, 0, 0, 0]);
}

/// The display scale (1.25 at 120 dpi), from Xft.dpi like GTK and Qt apps read it.
pub fn dpi_scale(_hwnd: HWND) -> f32 {
    if let Some(s) = std::env::var("WINKLET_SCALE").ok().and_then(|s| s.parse::<f32>().ok()) {
        return s.clamp(0.5, 4.0);
    }
    let Some(x) = x() else { return 1.0 };
    let res = x
        .conn
        .get_property(false, x.root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 1 << 16)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
        .unwrap_or_default();
    res.lines()
        .find_map(|l| l.strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|dpi| (dpi / 96.0).clamp(0.5, 4.0))
        .unwrap_or(1.0)
}

/// Screen edge the island sits on.
pub const EDGE_TOP: i32 = 0;
pub const EDGE_BOTTOM: i32 = 1;
pub const EDGE_LEFT: i32 = 2;
pub const EDGE_RIGHT: i32 = 3;
pub const EDGE_FLOAT: i32 = 4;

#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// (monitor rect, work area) of the monitor holding `p`, or the primary one.
fn monitor_rects(p: Option<(i32, i32)>) -> (Rect, Rect) {
    let fallback = Rect { left: 0, top: 0, right: 1920, bottom: 1080 };
    let Some(x) = x() else { return (fallback, fallback) };
    let mons: Vec<(Rect, bool)> = x
        .conn
        .randr_get_monitors(x.root, true)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| {
            r.monitors
                .iter()
                .map(|m| {
                    let (l, t) = (m.x as i32, m.y as i32);
                    (Rect { left: l, top: t, right: l + m.width as i32, bottom: t + m.height as i32 }, m.primary)
                })
                .collect()
        })
        .unwrap_or_default();
    let m = match p {
        Some((px, py)) => mons
            .iter()
            .min_by_key(|(r, _)| {
                let dx = (r.left - px).max(px - r.right).max(0);
                let dy = (r.top - py).max(py - r.bottom).max(0);
                dx * dx + dy * dy
            })
            .map(|(r, _)| *r),
        None => mons.iter().find(|(_, primary)| *primary).or(mons.first()).map(|(r, _)| *r),
    };
    let Some(m) = m else {
        let s = &x.conn.setup().roots[0];
        let r = Rect { left: 0, top: 0, right: s.width_in_pixels as i32, bottom: s.height_in_pixels as i32 };
        return (r, r);
    };
    // _NET_WORKAREA spans every monitor of the desktop; the part over this one is its work area
    let wa = x.prop32(x.root, "_NET_WORKAREA");
    let work = if wa.len() >= 4 {
        let (l, t) = (wa[0] as i32, wa[1] as i32);
        let w = Rect { left: m.left.max(l), top: m.top.max(t), right: m.right.min(l + wa[2] as i32), bottom: m.bottom.min(t + wa[3] as i32) };
        if w.right > w.left && w.bottom > w.top { w } else { m }
    } else {
        m
    };
    (m, work)
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

/// Where the island window sits (physical px).
pub struct IslandPlace {
    /// the resting pill's window, relative to the window being placed
    pub rest_x: i32,
    pub rest_y: i32,
    /// floating too low for the panel to fit under the pill: it opens upwards
    pub open_up: bool,
    /// room from the top of the resting pill to the bottom of the work area
    pub below: i32,
}

/// Places the island window: the same layout as on Windows (see the Windows `sys.rs`).
pub fn place_island(hwnd: HWND, (w, h): (i32, i32), rest: (i32, i32), big_h: i32, anchor: Option<(i32, i32)>, edge: i32, scale: f32) -> IslandPlace {
    let (cx, cy) = anchor.unwrap_or_else(|| default_anchor(scale));
    let (m, wk) = monitor_rects(Some((cx, cy)));
    let mid = (m.left + m.right) / 2;
    let snap = (28.0 * scale) as i32;
    let ccx = if (cx - mid).abs() < snap { mid } else { cx };
    let clamp_x = |x: i32, w: i32| x.clamp(wk.left, (wk.right - w).max(wk.left));
    let clamp_y = |y: i32, h: i32| y.clamp(wk.top, (wk.bottom - h).max(wk.top));
    let (rw, rh) = rest;
    let (rx, ry) = match edge {
        EDGE_TOP => (ccx - rw / 2, m.top),
        EDGE_BOTTOM => (ccx - rw / 2, wk.bottom - rh),
        EDGE_LEFT => (wk.left, cy - rh / 2),
        EDGE_RIGHT => (wk.right - rw, cy - rh / 2),
        _ => (cx - rw / 2, cy - (17.0 * scale) as i32),
    };
    let rx = clamp_x(rx, rw);
    let ry = if edge == EDGE_TOP { ry } else { clamp_y(ry, rh) };
    let open_up = edge == EDGE_FLOAT && ry + big_h > wk.bottom;
    let x = match edge {
        EDGE_LEFT => wk.left,
        EDGE_RIGHT => wk.right - w,
        _ => clamp_x(rx + rw / 2 - w / 2, w),
    };
    let y = match edge {
        EDGE_TOP => m.top,
        EDGE_BOTTOM => wk.bottom - h,
        EDGE_LEFT | EDGE_RIGHT => clamp_y(ry + rh / 2 - h / 2, h),
        _ if open_up => clamp_y(ry + rh - h, h),
        _ => clamp_y(ry, h),
    };
    match x_window(hwnd) {
        Some(xc) => {
            let aux = ConfigureWindowAux::new().x(x).y(y).width(w.max(1) as u32).height(h.max(1) as u32).stack_mode(StackMode::ABOVE);
            let _ = xc.conn.configure_window(hwnd.0, &aux);
            let _ = xc.conn.flush();
        }
        // native Wayland: only the size can be asked for
        None => UI.with(|u| {
            if let Some(ui) = u.borrow().as_ref().and_then(|w| w.upgrade()) {
                use slint::ComponentHandle;
                ui.window().set_size(slint::PhysicalSize::new(w.max(1) as u32, h.max(1) as u32));
            }
        }),
    }
    IslandPlace { rest_x: rx - x, rest_y: ry - y, open_up, below: wk.bottom - ry }
}

/// The X connection, when `hwnd` is an X window.
fn x_window(hwnd: HWND) -> Option<&'static X> {
    (hwnd.0 != 0 && hwnd.0 != u32::MAX).then(x).flatten()
}

fn pointer() -> Option<(i32, i32, u16)> {
    let x = x()?;
    let r = x.conn.query_pointer(x.root).ok()?.reply().ok()?;
    Some((r.root_x as i32, r.root_y as i32, u16::from(r.mask)))
}

pub fn cursor_pos() -> (i32, i32) {
    pointer().map(|(x, y, _)| (x, y)).unwrap_or((0, 0))
}

pub fn left_button_down() -> bool {
    pointer().is_some_and(|(_, _, m)| m & u16::from(KeyButMask::BUTTON1) != 0)
}

pub fn escape_down() -> bool {
    // keycode 9 is Escape with the evdev keymap every current X server uses
    x().and_then(|x| x.conn.query_keymap().ok()?.reply().ok()).is_some_and(|r| r.keys[1] & 0x02 != 0)
}

pub fn right_button_down() -> bool {
    pointer().is_some_and(|(_, _, m)| m & u16::from(KeyButMask::BUTTON3) != 0)
}

pub fn set_visible(hwnd: HWND, visible: bool) {
    let Some(x) = x_window(hwnd) else { return };
    let _ = if visible { x.conn.map_window(hwnd.0) } else { x.conn.unmap_window(hwnd.0) };
    let _ = x.conn.flush();
}

pub const FS_NONE: i32 = 0;
/// a fullscreen window (borderless game, video, browser) or a presentation
pub const FS_WINDOW: i32 = 1;
/// exclusive fullscreen: never reported on Linux (any window may sit on top of a game)
pub const FS_EXCLUSIVE: i32 = 2;

/// What owns the screen right now.
pub fn fullscreen_state() -> i32 {
    let Some(x) = x() else { return FS_NONE };
    let active = foreground();
    if active.is_invalid() {
        return FS_NONE;
    }
    let fs = x.atom("_NET_WM_STATE_FULLSCREEN");
    if x.prop32(active.0, "_NET_WM_STATE").contains(&fs) { FS_WINDOW } else { FS_NONE }
}

fn read(path: impl AsRef<std::path::Path>) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn power_supplies() -> Vec<PathBuf> {
    std::fs::read_dir("/sys/class/power_supply").map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

fn battery_dir() -> Option<PathBuf> {
    power_supplies().into_iter().find(|p| read(p.join("type")).as_deref() == Some("Battery") && read(p.join("scope")).as_deref() != Some("Device"))
}

/// (percent or -1 when there is no battery, plugged in)
pub fn battery() -> (i32, bool) {
    let Some(bat) = battery_dir() else { return (-1, false) };
    let Some(pct) = read(bat.join("capacity")).and_then(|c| c.parse::<i32>().ok()) else { return (-1, false) };
    let ac = power_supplies().iter().any(|p| read(p.join("type")).as_deref() == Some("Mains") && read(p.join("online")).as_deref() == Some("1"));
    let plugged = ac || read(bat.join("status")).is_some_and(|s| s != "Discharging");
    (pct.clamp(0, 100), plugged)
}

/// Battery time left in seconds, from the current drain (on battery only).
pub fn battery_secs_left() -> Option<u32> {
    let bat = battery_dir()?;
    if read(bat.join("status")).as_deref() != Some("Discharging") {
        return None;
    }
    let num = |n: &str| read(bat.join(n)).and_then(|v| v.parse::<f64>().ok());
    let (left, rate) = num("energy_now").zip(num("power_now")).or_else(|| num("charge_now").zip(num("current_now")))?;
    (rate > 0.0).then(|| (left / rate * 3600.0) as u32)
}

/// Puts the island back on top (fullscreen windows can cover it).
pub fn keep_on_top(hwnd: HWND) {
    let Some(x) = x_window(hwnd) else { return };
    let _ = x.conn.configure_window(hwnd.0, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    let _ = x.conn.flush();
}

pub fn idle_seconds() -> u32 {
    x().and_then(|x| x.conn.screensaver_query_info(x.root).ok()?.reply().ok()).map(|r| r.ms_since_user_input / 1000).unwrap_or(0)
}

thread_local! {
    static LAST_CPU: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    /// (when, value) of the last CPU reading
    static CPU_READING: Cell<Option<(Instant, f32)>> = const { Cell::new(None) };
}

/// System-wide CPU usage (0..1) since the previous call (readings kept for half a second,
/// like on Windows, so several views asking close together all see a real number).
pub fn cpu_usage() -> f32 {
    let last = CPU_READING.with(|c| c.get());
    if let Some((at, v)) = last
        && at.elapsed().as_millis() < 500
    {
        return v;
    }
    let last = last.map(|(_, v)| v).unwrap_or(0.0);
    let Some(line) = std::fs::read_to_string("/proc/stat").ok().and_then(|s| s.lines().next().map(String::from)) else { return last };
    // cpu  user nice system idle iowait irq softirq steal
    let f: Vec<u64> = line.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
    if f.len() < 4 {
        return last;
    }
    let total: u64 = f.iter().take(8).sum();
    let idle = f[3] + f.get(4).copied().unwrap_or(0);
    let (pi, pt) = LAST_CPU.with(|c| c.replace((idle, total)));
    let (di, dt) = (idle.saturating_sub(pi), total.saturating_sub(pt));
    if pt == 0 || dt == 0 {
        return last;
    }
    let v = (1.0 - di as f32 / dt as f32).clamp(0.0, 1.0);
    CPU_READING.with(|c| c.set(Some((Instant::now(), v))));
    v
}

/// (load 0..1, used GB, total GB)
pub fn memory() -> (f32, f64, f64) {
    let text = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let kb = |key: &str| {
        text.lines().find_map(|l| l.strip_prefix(key)).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok()).unwrap_or(0.0)
    };
    let (total, avail) = (kb("MemTotal:"), kb("MemAvailable:"));
    if total <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let gb = |k: f64| k / 1024f64.powi(2);
    (((total - avail) / total) as f32, gb(total - avail), gb(total))
}

pub fn uptime() -> String {
    let s = read("/proc/uptime").and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok()).unwrap_or(0.0) as u64;
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
    crate::clip::set_text(text)
}

/// Starts a program quietly (no output, not waited for).
fn spawn(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok()
}

/// Opens a link, file or folder in its app, or starts a program (`args` split on spaces).
pub fn open(target: &str, args: &str) -> bool {
    let t = target.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();
    if let Some(ok) = open_windows_tool(&lower, args) {
        return ok;
    }
    // other Windows-only targets (ms-settings:, C:\…) have nothing to open here
    if lower.ends_with(".exe") || lower.starts_with("ms-") || lower.contains(":\\") {
        return false;
    }
    let path = std::path::Path::new(t);
    if args.trim().is_empty() && (t.contains("://") || t.starts_with("mailto:") || (path.exists() && !is_program(path))) {
        return spawn("xdg-open", &[t]);
    }
    if !args.trim().is_empty() || is_program(path) || which(t) {
        return spawn(t, &args.split_whitespace().collect::<Vec<_>>());
    }
    // a link with an app's own scheme (spotify:…)
    t.contains(':') && spawn("xdg-open", &[t])
}

/// The first of `programs` that is installed, started with `args`.
fn first_of(programs: &[&str], args: &[&str]) -> bool {
    programs.iter().any(|p| which(p) && spawn(p, args))
}

/// The Linux stand-in for a Windows tool the app opens by name (terminal, task manager,
/// Explorer, Notepad, Calculator). None when `target` isn't one of them.
fn open_windows_tool(target: &str, args: &str) -> Option<bool> {
    const TERMINALS: [&str; 8] = ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "kgx", "kitty", "alacritty", "xterm"];
    let exe = target.rsplit(['\\', '/']).next().unwrap_or(target);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    Some(match exe {
        "wt.exe" | "cmd.exe" | "powershell.exe" => first_of(&TERMINALS, &[]),
        "taskmgr.exe" => first_of(&["gnome-system-monitor", "plasma-systemmonitor", "ksysguard", "xfce4-taskmanager", "mate-system-monitor"], &[]),
        "calc.exe" => first_of(&["gnome-calculator", "kcalc", "galculator", "mate-calc", "qalculate-gtk"], &[]),
        "notepad.exe" => {
            let file = if args.to_lowercase().ends_with(r"drivers\etc\hosts") { "/etc/hosts".to_string() } else { args.trim_matches('"').to_string() };
            spawn("xdg-open", &[&file])
        }
        // explorer /select,"C:\…\file": the folder holding it
        "explorer.exe" => {
            let path = args.strip_prefix("/select,").map(|p| p.trim_matches('"')).unwrap_or(args.trim());
            let dir = if path.is_empty() {
                home
            } else {
                std::path::Path::new(path).parent().filter(|_| !std::path::Path::new(path).is_dir()).map(|d| d.display().to_string()).unwrap_or_else(|| path.to_string())
            };
            spawn("xdg-open", &[&dir])
        }
        _ => return None,
    })
}

fn is_program(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file() && std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

fn which(name: &str) -> bool {
    !name.contains('/') && std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| is_program(&d.join(name))))
}

/// Output of a command, when it ran and succeeded.
fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether an app handles links like `spotify:…`.
pub fn protocol_registered(scheme: &str) -> bool {
    output("xdg-mime", &["query", "default", &format!("x-scheme-handler/{scheme}")]).is_some_and(|s| !s.is_empty())
}

pub fn foreground() -> HWND {
    x().and_then(|x| x.prop32(x.root, "_NET_ACTIVE_WINDOW").first().copied()).map(HWND).unwrap_or_default()
}

/// Hands keyboard focus back to the app the user was in before clicking the island.
pub fn restore_foreground(ours: HWND, prev: HWND) {
    let Some(x) = x() else { return };
    if !prev.is_invalid() && foreground() == ours {
        // source 2: a pager / taskbar asking, which window managers always honour
        x.tell_wm(prev.0, "_NET_ACTIVE_WINDOW", [2, 0, 0, 0, 0]);
    }
}

/// Plays a desktop sound for one of the Windows sound names the app uses.
pub fn play_sound(alias: &str) {
    let id = match alias {
        "Notification.Looping.Alarm" => "alarm-clock-elapsed",
        "Notification.Reminder" => "message-new-instant",
        "DeviceConnect" => "device-added",
        "DeviceDisconnect" => "device-removed",
        _ => "message",
    };
    if !spawn("canberra-gtk-play", &["-i", id]) {
        let file = format!("/usr/share/sounds/freedesktop/stereo/{id}.oga");
        let file = if std::path::Path::new(&file).exists() { file } else { "/usr/share/sounds/freedesktop/stereo/message.oga".into() };
        spawn("paplay", &[&file]);
    }
}

fn autostart_file() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    config.join("autostart").join("winklet.desktop")
}

/// Nothing to carry over on Linux (the old DyIs name never shipped here).
pub fn migrate_autostart() {}

pub fn autostart_enabled() -> bool {
    autostart_file().exists()
}

pub fn set_autostart(on: bool) {
    let file = autostart_file();
    if !on {
        let _ = std::fs::remove_file(&file);
        return;
    }
    // inside an AppImage, start the AppImage itself (current_exe is in a temporary mount)
    let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok());
    let Some(exe) = exe else { return };
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=Winklet\nComment=A tiny winking Dynamic Island\nExec=\"{}\"\nIcon=winklet\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&file, text);
}

/// Returns false if another Winklet is already running (an abstract socket only one
/// process can hold; it goes away with the process, even after a crash).
pub fn single_instance() -> bool {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener};
    let Ok(addr) = SocketAddr::from_abstract_name(b"winklet.single-instance") else { return true };
    match UnixListener::bind_addr(&addr) {
        Ok(l) => {
            // held for the life of the process
            std::mem::forget(l);
            true
        }
        Err(e) => e.kind() != std::io::ErrorKind::AddrInUse,
    }
}

/// Raising priority needs root on Linux; the island stays at the normal level.
pub fn boost_priority() {}

/// The allocator hands memory back on its own here.
pub fn trim_memory() {}

/// The user's Downloads folder (XDG user dirs), else ~/Downloads.
pub fn downloads_dir() -> String {
    output("xdg-user-dir", &["DOWNLOAD"])
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| format!("{}/Downloads", std::env::var("HOME").unwrap_or_default()))
}

/// "Open" dialog. `folders` switches it into folder-picking mode. Multi-select.
pub fn pick_paths(owner: HWND, folders: bool) -> Vec<PathBuf> {
    pick_paths_titled(owner, folders, if folders { "Add folders to the shelf" } else { "Add files to the shelf" })
}

/// Same picker with its own title: zenity (GNOME and most desktops), else kdialog (KDE).
pub fn pick_paths_titled(_owner: HWND, folders: bool, title: &str) -> Vec<PathBuf> {
    let mut z = vec!["--file-selection", "--multiple", "--separator=\n", "--title", title];
    if folders {
        z.push("--directory");
    }
    let picked = output("zenity", &z).or_else(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        let k = if folders {
            vec!["--title", title, "--getexistingdirectory", home.as_str()]
        } else {
            vec!["--title", title, "--multiple", "--separate-output", "--getopenfilename", home.as_str()]
        };
        output("kdialog", &k)
    });
    picked.unwrap_or_default().lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

pub const HOTKEY_SCREENSHOT: i32 = 1;
pub const HOTKEY_MIC: i32 = 2;
pub const HOTKEY_ISLAND: i32 = 3;
pub const HOTKEY_GEAR_UP: i32 = 4;
pub const HOTKEY_GEAR_DOWN: i32 = 5;
pub const HOTKEY_GAME: i32 = 6;
pub const HOTKEY_CLIP: i32 = 7;

/// Global hotkeys (the same Ctrl+Alt combos as on Windows), grabbed on the X root window by a
/// thread of their own that sleeps until one is pressed.
pub fn spawn_hotkeys(on_hotkey: impl Fn(i32) + Send + 'static) {
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{GrabMode, ModMask};
    // keysyms: s, m, space, Prior, Next, g, v
    const KEYS: [(u32, i32); 7] = [
        (0x73, HOTKEY_SCREENSHOT),
        (0x6d, HOTKEY_MIC),
        (0x20, HOTKEY_ISLAND),
        (0xff55, HOTKEY_GEAR_UP),
        (0xff56, HOTKEY_GEAR_DOWN),
        (0x67, HOTKEY_GAME),
        (0x76, HOTKEY_CLIP),
    ];
    let _ = std::thread::Builder::new().name("hotkeys".into()).stack_size(128 * 1024).spawn(move || {
        let Ok((conn, screen)) = x11rb::connect(None) else { return };
        let setup = conn.setup();
        let root = setup.roots[screen].root;
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let Some(map) = conn.get_keyboard_mapping(min, max - min + 1).ok().and_then(|c| c.reply().ok()) else { return };
        let per = map.keysyms_per_keycode.max(1) as usize;
        let mut codes: HashMap<u8, i32> = HashMap::new();
        for (i, syms) in map.keysyms.chunks(per).enumerate() {
            if let Some(&(_, id)) = KEYS.iter().find(|(sym, _)| syms.first() == Some(sym)) {
                codes.entry(min + i as u8).or_insert(id);
            }
        }
        let base = ModMask::CONTROL | ModMask::M1;
        // also with Num Lock (M2) and Caps Lock on, or the grab misses them
        let extras = [ModMask::from(0u16), ModMask::M2, ModMask::LOCK, ModMask::M2 | ModMask::LOCK];
        for &code in codes.keys() {
            for &e in &extras {
                let _ = conn.grab_key(true, root, base | e, code, GrabMode::ASYNC, GrabMode::ASYNC);
            }
        }
        let _ = conn.flush();
        while let Ok(ev) = conn.wait_for_event() {
            if let Event::KeyPress(k) = ev
                && let Some(&id) = codes.get(&k.detail)
            {
                on_hotkey(id);
            }
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    Efficiency,
    Balanced,
    Performance,
}

/// Sets the power profile through power-profiles-daemon (GNOME, KDE and most distros).
pub fn set_power_mode(mode: PowerMode) -> bool {
    let p = match mode {
        PowerMode::Efficiency => "power-saver",
        PowerMode::Balanced => "balanced",
        PowerMode::Performance => "performance",
    };
    output("powerprofilesctl", &["set", p]).is_some()
}

/// Total bytes received / sent by physical network adapters since boot (virtual ones such
/// as Docker bridges, VPN tunnels and loopback have no device behind them, so they're skipped).
pub fn net_bytes() -> (u64, u64) {
    let text = std::fs::read_to_string("/proc/net/dev").unwrap_or_default();
    let (mut rx, mut tx) = (0u64, 0u64);
    for line in text.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let name = name.trim();
        if !std::path::Path::new(&format!("/sys/class/net/{name}/device")).exists() {
            continue;
        }
        let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        if f.len() >= 9 {
            rx += f[0];
            tx += f[8];
        }
    }
    (rx, tx)
}

/// The Power tab: what the PC should do.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PowerAction {
    Lock,
    Sleep,
    Restart,
    SignOut,
    ShutDown,
}

impl PowerAction {
    /// The order of the tiles in the Power tab.
    pub fn from_index(i: i32) -> Option<Self> {
        [Self::Lock, Self::Sleep, Self::Restart, Self::SignOut, Self::ShutDown].get(usize::try_from(i).ok()?).copied()
    }
}

/// Locks, sleeps, restarts, signs out or shuts down through systemd-logind, which asks
/// the desktop first, so apps with unsaved work still get their say.
pub fn power(action: PowerAction) -> bool {
    let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    match action {
        PowerAction::Lock => output("loginctl", &["lock-session"]).is_some() || spawn("xdg-screensaver", &["lock"]),
        PowerAction::Sleep => spawn("systemctl", &["suspend"]),
        PowerAction::Restart => spawn("systemctl", &["reboot"]),
        PowerAction::ShutDown => spawn("systemctl", &["poweroff"]),
        PowerAction::SignOut if !session.is_empty() => spawn("loginctl", &["terminate-session", &session]),
        PowerAction::SignOut => spawn("loginctl", &["terminate-user", &std::env::var("USER").unwrap_or_default()]),
    }
}

// ------------------------------------------------------------ windows of other apps (for fg)

/// The process that owns an X window (_NET_WM_PID).
pub fn window_pid(hwnd: HWND) -> Option<u32> {
    let x = x_window(hwnd)?;
    x.prop32(hwnd.0, "_NET_WM_PID").first().copied().filter(|p| *p != 0)
}

/// An X window's title (_NET_WM_NAME, else WM_NAME).
pub fn window_title(hwnd: HWND) -> String {
    let Some(x) = x_window(hwnd) else { return String::new() };
    let get = |prop: Atom| {
        x.conn
            .get_property(false, hwnd.0, prop, AtomEnum::ANY, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .filter(|s| !s.is_empty())
    };
    get(x.atom("_NET_WM_NAME")).or_else(|| get(AtomEnum::WM_NAME.into())).unwrap_or_default()
}

/// The desktop, docks and panels: never "the app in front".
pub fn is_shell_window(hwnd: HWND) -> bool {
    let Some(x) = x_window(hwnd) else { return true };
    let types = x.prop32(hwnd.0, "_NET_WM_WINDOW_TYPE");
    types.contains(&x.atom("_NET_WM_WINDOW_TYPE_DESKTOP")) || types.contains(&x.atom("_NET_WM_WINDOW_TYPE_DOCK"))
}

/// Whether a window is fullscreen (_NET_WM_STATE_FULLSCREEN).
pub fn is_fullscreen(hwnd: HWND) -> bool {
    let Some(x) = x_window(hwnd) else { return false };
    x.prop32(hwnd.0, "_NET_WM_STATE").contains(&x.atom("_NET_WM_STATE_FULLSCREEN"))
}

/// Every normal window the window manager lists (_NET_CLIENT_LIST), oldest first.
pub fn client_windows() -> Vec<HWND> {
    x().map(|x| x.prop32(x.root, "_NET_CLIENT_LIST").into_iter().map(HWND).collect()).unwrap_or_default()
}
