//! Synthetic keyboard input for Deck keys: hotkeys ("ctrl+shift+esc"), typed text and
//! media keys, through `xdotool` to whatever window has the focus.

use std::process::{Command, Stdio};

/// xdotool's name for one part of a combo ("ctrl", "a", "f5", "esc", "playpause" …).
fn keysym(name: &str) -> Option<String> {
    let n = name.trim().to_lowercase();
    let s = match n.as_str() {
        "ctrl" | "control" => "ctrl",
        "shift" => "shift",
        "alt" => "alt",
        "win" | "windows" | "super" => "super",
        "enter" | "return" => "Return",
        "esc" | "escape" => "Escape",
        "tab" => "Tab",
        "space" => "space",
        "backspace" => "BackSpace",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pgup" | "pageup" => "Prior",
        "pgdn" | "pagedown" => "Next",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "printscreen" | "prtsc" => "Print",
        "playpause" => "XF86AudioPlay",
        "next" => "XF86AudioNext",
        "prev" | "previous" => "XF86AudioPrev",
        "stop" => "XF86AudioStop",
        "mute" => "XF86AudioMute",
        "voldown" => "XF86AudioLowerVolume",
        "volup" => "XF86AudioRaiseVolume",
        "plus" => "plus",
        "minus" => "minus",
        _ => {
            let b = n.as_bytes();
            if b.len() == 1 && b[0].is_ascii_alphanumeric() {
                return Some(n);
            }
            if let Some(f) = n.strip_prefix('f').and_then(|f| f.parse::<u16>().ok()).filter(|f| (1..=24).contains(f)) {
                return Some(format!("F{f}"));
            }
            return None;
        }
    };
    Some(s.to_string())
}

fn xdotool(args: &[&str]) -> bool {
    Command::new("xdotool").args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Presses a combo like "ctrl+shift+esc" or "win+d". None if a part isn't a key it knows.
pub fn combo(text: &str) -> Option<bool> {
    let parts: Vec<String> = text.split('+').filter(|p| !p.trim().is_empty()).map(keysym).collect::<Option<_>>()?;
    if parts.is_empty() {
        return None;
    }
    Some(xdotool(&["key", "--clearmodifiers", &parts.join("+")]))
}

/// Whether every part of a combo is a known key (for the editor).
pub fn combo_ok(text: &str) -> bool {
    !text.trim().is_empty() && text.split('+').filter(|p| !p.trim().is_empty()).all(|p| keysym(p).is_some())
}

/// One tap of a single key (media keys, volume).
pub fn tap(name: &str) -> bool {
    combo(name).unwrap_or(false)
}

/// Types text as if on the keyboard (any language or emoji), optionally pressing Enter after.
pub fn type_text(text: &str, enter: bool) -> bool {
    let ok = text.is_empty() || xdotool(&["type", "--clearmodifiers", "--delay", "4", "--", text]);
    ok && (!enter || xdotool(&["key", "Return"]))
}
