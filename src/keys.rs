//! Synthetic keyboard input for Deck keys: hotkeys ("ctrl+shift+esc"), typed text and
//! media keys. Everything goes through SendInput to whatever window has the focus.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY,
};

fn key(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn send(inputs: &[INPUT]) -> bool {
    !inputs.is_empty() && unsafe { SendInput(inputs, size_of::<INPUT>() as i32) } as usize == inputs.len()
}

/// Virtual-key code for one part of a combo ("ctrl", "a", "f5", "esc", "playpause" …).
fn vk(name: &str) -> Option<u16> {
    let n = name.trim().to_lowercase();
    let code = match n.as_str() {
        "ctrl" | "control" => 0x11,
        "shift" => 0x10,
        "alt" => 0x12,
        "win" | "windows" | "super" => 0x5B,
        "enter" | "return" => 0x0D,
        "esc" | "escape" => 0x1B,
        "tab" => 0x09,
        "space" => 0x20,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pgup" | "pageup" => 0x21,
        "pgdn" | "pagedown" => 0x22,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "printscreen" | "prtsc" => 0x2C,
        "playpause" => 0xB3,
        "next" => 0xB0,
        "prev" | "previous" => 0xB1,
        "stop" => 0xB2,
        "mute" => 0xAD,
        "voldown" => 0xAE,
        "volup" => 0xAF,
        "plus" => 0xBB,
        "minus" => 0xBD,
        _ => {
            let b = n.as_bytes();
            if b.len() == 1 && (b[0].is_ascii_alphanumeric()) {
                return Some(b[0].to_ascii_uppercase() as u16);
            }
            if let Some(f) = n.strip_prefix('f').and_then(|f| f.parse::<u16>().ok()).filter(|f| (1..=24).contains(f)) {
                return Some(0x70 + f - 1);
            }
            return None;
        }
    };
    Some(code)
}

/// Presses a combo like "ctrl+shift+esc" or "win+d": modifiers down, the key, then all up.
/// None if a part isn't a key it knows.
pub fn combo(text: &str) -> Option<bool> {
    let codes: Vec<u16> = text.split('+').filter(|p| !p.trim().is_empty()).map(vk).collect::<Option<_>>()?;
    if codes.is_empty() {
        return None;
    }
    let mut inputs: Vec<INPUT> = codes.iter().map(|&c| key(c, 0, KEYBD_EVENT_FLAGS(0))).collect();
    inputs.extend(codes.iter().rev().map(|&c| key(c, 0, KEYEVENTF_KEYUP)));
    Some(send(&inputs))
}

/// Whether every part of a combo is a known key (for the editor).
pub fn combo_ok(text: &str) -> bool {
    !text.trim().is_empty() && text.split('+').filter(|p| !p.trim().is_empty()).all(|p| vk(p).is_some())
}

/// One tap of a single key (media keys, volume).
pub fn tap(name: &str) -> bool {
    combo(name).unwrap_or(false)
}

/// Types text as if on the keyboard (any language or emoji), optionally pressing Enter after.
pub fn type_text(text: &str, enter: bool) -> bool {
    let mut inputs = Vec::new();
    for u in text.replace("\r\n", "\n").encode_utf16() {
        if u == '\n' as u16 {
            inputs.push(key(0x0D, 0, KEYBD_EVENT_FLAGS(0)));
            inputs.push(key(0x0D, 0, KEYEVENTF_KEYUP));
            continue;
        }
        inputs.push(key(0, u, KEYEVENTF_UNICODE));
        inputs.push(key(0, u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    if enter {
        inputs.push(key(0x0D, 0, KEYBD_EVENT_FLAGS(0)));
        inputs.push(key(0x0D, 0, KEYEVENTF_KEYUP));
    }
    send(&inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos() {
        assert!(combo_ok("ctrl+shift+esc"));
        assert!(combo_ok("Win + D"));
        assert!(combo_ok("alt+f4"));
        assert!(!combo_ok("ctrl+banana"));
        assert!(!combo_ok(""));
        assert_eq!(vk("f12"), Some(0x7B));
        assert_eq!(vk("q"), Some(b'Q' as u16));
    }
}
