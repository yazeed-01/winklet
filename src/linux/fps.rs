//! Frames per second of the running game. Windows reads them from RTSS or ETW; neither exists
//! on Linux (MangoHud shows FPS inside the game instead), so the Game tab shows "–".

#[allow(dead_code)]
pub const OFF: u8 = 0;
pub const RUNNING: u8 = 1;
pub const DENIED: u8 = 2;
pub const FAILED: u8 = 3;

pub fn state() -> u8 {
    FAILED
}

pub fn set_target(_pid: u32) {}

pub fn take() -> u32 {
    0
}

pub fn rtss(_pid: u32) -> Option<f32> {
    None
}

pub fn start() {}

pub fn stop() {}

pub fn request_permission() -> bool {
    false
}
