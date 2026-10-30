//! Xbox-compatible controllers (XInput): connected, being used, battery.
//! PlayStation pads show up too when Steam Input or DS4Windows maps them to XInput.

use std::time::{Duration, Instant};
use windows::Win32::UI::Input::XboxController::{
    BATTERY_DEVTYPE_GAMEPAD, BATTERY_TYPE_DISCONNECTED, BATTERY_TYPE_WIRED, XINPUT_BATTERY_INFORMATION, XINPUT_STATE,
    XInputGetBatteryInformation, XInputGetState,
};

#[derive(Default)]
pub struct Pads {
    /// last packet number per slot; None = nothing plugged in there
    last: [Option<u32>; 4],
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
    /// Polls the controllers. Empty slots are slow to query, so they're only rescanned every 10 s.
    pub fn poll(&mut self) -> PadState {
        let rescan = self.next_scan.is_none_or(|t| Instant::now() >= t);
        if rescan {
            self.next_scan = Some(Instant::now() + Duration::from_secs(10));
        }
        let mut out = PadState { battery: -1, ..Default::default() };
        for i in 0..4u32 {
            let slot = i as usize;
            if self.last[slot].is_none() && !rescan {
                continue;
            }
            let mut st = XINPUT_STATE::default();
            if unsafe { XInputGetState(i, &mut st) } != 0 {
                self.last[slot] = None;
                continue;
            }
            if self.last[slot].is_some_and(|p| p != st.dwPacketNumber) {
                out.active = true;
            }
            self.last[slot] = Some(st.dwPacketNumber);
            if !out.connected {
                out.connected = true;
                let mut bi = XINPUT_BATTERY_INFORMATION::default();
                if unsafe { XInputGetBatteryInformation(i, BATTERY_DEVTYPE_GAMEPAD, &mut bi) } == 0 {
                    out.battery = if bi.BatteryType == BATTERY_TYPE_WIRED {
                        4
                    } else if bi.BatteryType == BATTERY_TYPE_DISCONNECTED {
                        -1
                    } else {
                        bi.BatteryLevel.0 as i32
                    };
                }
            }
        }
        out
    }
}
