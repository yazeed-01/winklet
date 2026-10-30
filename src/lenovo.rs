//! Lenovo performance mode (Fn+Q: Quiet / Auto / Performance).
//!
//! Reading the mode directly (LENOVO_GAMEZONE_DATA.GetSmartFanMode) needs admin,
//! but the change *event* is delivered to normal users, so we listen for it on a
//! thread that sleeps inside WMI until Fn+Q is pressed.

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, EOAC_NONE,
    RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE, WbemLocator,
};
use windows::core::{BSTR, w};

/// Calls `on_mode` (1 quiet, 2 auto, 3 performance) every time the mode changes.
/// Does nothing on machines without Lenovo GameZone.
pub fn watch(on_mode: impl Fn(i32) + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("lenovo-mode".into())
        .stack_size(128 * 1024)
        .spawn(move || {
            let _ = listen(&on_mode);
        });
}

fn listen(on_mode: &dyn Fn(i32)) -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let services =
            locator.ConnectServer(&BSTR::from("ROOT\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            None,
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?;
        let events = services.ExecNotificationQuery(
            &BSTR::from("WQL"),
            &BSTR::from("SELECT * FROM LENOVO_GAMEZONE_SMART_FAN_MODE_EVENT"),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )?;
        loop {
            let mut objs: [Option<IWbemClassObject>; 1] = [None];
            let mut got = 0u32;
            // blocks (no CPU) until the next Fn+Q
            events.Next(WBEM_INFINITE, &mut objs, &mut got).ok()?;
            if got == 0 {
                continue;
            }
            if let Some(obj) = &objs[0] {
                let mut v = VARIANT::default();
                if obj.Get(w!("mode"), 0, &mut v, None, None).is_ok() {
                    if let Ok(mode) = i32::try_from(&v) {
                        on_mode(mode);
                    }
                }
            }
        }
    }
}
