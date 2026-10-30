//! Frames per second of the running game. Two sources, tried in order:
//! 1. RivaTuner Statistics Server (comes with MSI Afterburner) shared memory, when it's running.
//! 2. Windows' own present events (ETW: the DXGI and D3D9 providers PresentMon reads).
//!    Starting that trace needs admin rights or the "Performance Log Users" group,
//!    so it can be unavailable; the UI then shows "–" and offers to allow it.
//!
//! Vulkan and OpenGL games only show up through RTSS (or when the driver presents them through DXGI).

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_SUCCESS};
use windows::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_RECORD,
    EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE,
    EnableTraceEx2, OpenTraceW, PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, ProcessTrace,
    StartTraceW, TRACE_LEVEL_INFORMATION, WNODE_FLAG_TRACED_GUID,
};
use windows::Win32::System::Memory::{FILE_MAP_READ, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile};
use windows::core::{GUID, PWSTR, w};

const SESSION: &str = "Winklet FPS";
const DXGI: GUID = GUID::from_u128(0xca11c036_0102_4a2d_a6ad_f03cfed5d3c9);
const D3D9: GUID = GUID::from_u128(0x783aca0a_790e_4d7f_8451_aa850511c6b9);
/// DXGI Present_Start / PresentMultiplaneOverlay_Start, D3D9 Present_Start
const DXGI_PRESENT: [u16; 2] = [42, 55];
const D3D9_PRESENT: u16 = 1;

pub const OFF: u8 = 0;
pub const RUNNING: u8 = 1;
pub const DENIED: u8 = 2;
pub const FAILED: u8 = 3;

static STATE: AtomicU8 = AtomicU8::new(OFF);
static TARGET: AtomicU32 = AtomicU32::new(0);
static PRESENTS: AtomicU32 = AtomicU32::new(0);

pub fn state() -> u8 {
    STATE.load(Ordering::Relaxed)
}

/// Which process to count frames for (0 = none).
pub fn set_target(pid: u32) {
    if TARGET.swap(pid, Ordering::Relaxed) != pid {
        PRESENTS.store(0, Ordering::Relaxed);
    }
}

/// Frames presented by the target since the last call.
pub fn take() -> u32 {
    PRESENTS.swap(0, Ordering::Relaxed)
}

/// FPS of `pid` from RTSS, if RTSS is running and tracking it.
pub fn rtss(pid: u32) -> Option<f32> {
    unsafe {
        let h = OpenFileMappingW(FILE_MAP_READ.0, false, w!("RTSSSharedMemoryV2")).ok()?;
        let view = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0);
        let out = (|| {
            let base = view.Value as *const u8;
            if base.is_null() {
                return None;
            }
            let rd = |off: usize| std::ptr::read_unaligned(base.add(off) as *const u32);
            // header: signature 'RTSS', version, app entry size, app array offset, app array size
            if rd(0) != 0x5254_5353 || rd(4) < 0x0002_0000 {
                return None;
            }
            let (size, off, count) = (rd(8) as usize, rd(12) as usize, rd(16) as usize);
            if size < 284 || count > 512 {
                return None;
            }
            for i in 0..count {
                let e = off + i * size;
                if rd(e) != pid {
                    continue;
                }
                // after the pid: name[260], flags, time0, time1, frames
                let (t0, t1, frames) = (rd(e + 268), rd(e + 272), rd(e + 276));
                return (t1 > t0).then(|| 1000.0 * frames as f32 / (t1 - t0) as f32);
            }
            None
        })();
        if !view.Value.is_null() {
            let _ = UnmapViewOfFile(view);
        }
        let _ = CloseHandle(h);
        out
    }
}

fn props() -> (Vec<u64>, *mut EVENT_TRACE_PROPERTIES) {
    let bytes = size_of::<EVENT_TRACE_PROPERTIES>() + (SESSION.len() + 1) * 2;
    let mut buf = vec![0u64; bytes / 8 + 1];
    let p = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
    unsafe {
        (*p).Wnode.BufferSize = bytes as u32;
        (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*p).Wnode.ClientContext = 1; // QPC timestamps
        (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*p).FlushTimer = 1;
        (*p).LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
    }
    (buf, p)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Starts the present-event trace on its own thread (no-op if already running).
pub fn start() {
    if matches!(state(), RUNNING) {
        return;
    }
    STATE.store(RUNNING, Ordering::Relaxed);
    let spawned = std::thread::Builder::new().name("fps".into()).stack_size(128 * 1024).spawn(|| {
        let result = run();
        STATE.store(result, Ordering::Relaxed);
    });
    if spawned.is_err() {
        STATE.store(FAILED, Ordering::Relaxed);
    }
}

/// Stops the trace. Sessions outlive their process, so this also runs on quit.
pub fn stop() {
    let name = wide(SESSION);
    let (_buf, p) = props();
    unsafe {
        let _ = ControlTraceW(CONTROLTRACE_HANDLE::default(), windows::core::PCWSTR(name.as_ptr()), p, EVENT_TRACE_CONTROL_STOP);
    }
    if state() == RUNNING {
        STATE.store(OFF, Ordering::Relaxed);
    }
}

fn run() -> u8 {
    let mut name = wide(SESSION);
    unsafe {
        stop(); // a session left over from a crash would block ours
        let (_buf, p) = props();
        let mut handle = CONTROLTRACE_HANDLE::default();
        let err = StartTraceW(&mut handle, windows::core::PCWSTR(name.as_ptr()), p);
        if err != ERROR_SUCCESS {
            return if err == ERROR_ACCESS_DENIED { DENIED } else { FAILED };
        }
        for guid in [DXGI, D3D9] {
            let _ = EnableTraceEx2(
                handle,
                &guid,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                TRACE_LEVEL_INFORMATION as u8,
                u64::MAX,
                0,
                0,
                None,
            );
        }
        let mut log = EVENT_TRACE_LOGFILEW { LoggerName: PWSTR(name.as_mut_ptr()), ..Default::default() };
        log.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        log.Anonymous2.EventRecordCallback = Some(on_event);
        let trace = OpenTraceW(&mut log);
        if trace.Value == u64::MAX {
            stop();
            return FAILED;
        }
        STATE.store(RUNNING, Ordering::Relaxed);
        let _ = ProcessTrace(&[trace], None, None); // blocks until the session is stopped
        let _ = CloseTrace(trace);
    }
    OFF
}

unsafe extern "system" fn on_event(rec: *mut EVENT_RECORD) {
    let h = unsafe { &(*rec).EventHeader };
    let target = TARGET.load(Ordering::Relaxed);
    if target == 0 || h.ProcessId != target {
        return;
    }
    let id = h.EventDescriptor.Id;
    if (h.ProviderId == DXGI && DXGI_PRESENT.contains(&id)) || (h.ProviderId == D3D9 && id == D3D9_PRESENT) {
        PRESENTS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Adds this user to "Performance Log Users" (SID S-1-5-32-559) so the FPS trace can start
/// without running Winklet as admin. Shows one UAC prompt; takes effect after signing in again.
pub fn request_permission() -> bool {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let cmd = format!("-NoProfile -WindowStyle Hidden -Command \"Add-LocalGroupMember -SID 'S-1-5-32-559' -Member '{}'\"", user.replace('\'', "''"));
    let (exe, args) = (windows::core::HSTRING::from("powershell.exe"), windows::core::HSTRING::from(cmd));
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            w!("runas"),
            &exe,
            &args,
            windows::core::PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
        )
        .0 as isize
            > 32
    }
}
