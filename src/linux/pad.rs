//! Game controllers through the kernel's joystick devices (/dev/input/js*): connected or not.
//! Being used and battery aren't read on Linux yet.

use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Pads {
    connected: bool,
    next_scan: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub struct PadState {
    pub connected: bool,
    /// a button or stick moved since the last poll
    pub active: bool,
    /// -1 unknown/none, 0..3 = empty..full, 4 = wired
    pub battery: i32,
}

impl Pads {
    /// Looks for controllers every 10 s.
    pub fn poll(&mut self) -> PadState {
        if self.next_scan.is_none_or(|t| Instant::now() >= t) {
            self.next_scan = Some(Instant::now() + Duration::from_secs(10));
            self.connected = std::fs::read_dir("/dev/input")
                .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("js")))
                .unwrap_or(false);
        }
        PadState { connected: self.connected, active: false, battery: -1 }
    }
}
