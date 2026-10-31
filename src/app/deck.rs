//! The Deck tab: a grid of keys like a Stream Deck. A key opens an app, file, folder or link,
//! runs several of those at once, runs a command or a script, presses a hotkey, types text, or does one
//! of Winklet's own actions (media keys, mic, screenshot, lock …). Each key has its own
//! colour and either the program's icon or a glyph.

use super::{App, Shared, defer, s, schedule_collapse, schedule_shrink, take_screenshot, workspaces::item_name};
use crate::{Bridge, DeckSlot, keys, store, sys};
use slint::{Color, ComponentHandle, Image, ModelRc, Timer, VecModel};
use std::time::Duration;

pub const PER_PAGE: i32 = 24;
const MAX_PAGES: i32 = 6;

/// in the order of the editor's kind switch
const KINDS: [&str; 7] = ["open", "multi", "command", "script", "hotkey", "text", "action"];

/// in the order of the script editor's language switch
const LANGS: [&str; 2] = ["powershell", "batch"];

/// key colours: graphite, indigo, blue, cyan, green, lime, amber, orange, red, pink, purple, slate
pub const PALETTE: [&str; 12] =
    ["#2b2b33", "#4f46e5", "#2563eb", "#0891b2", "#059669", "#65a30d", "#ca8a04", "#ea580c", "#dc2626", "#db2777", "#9333ea", "#475569"];

pub fn hex_color(hex: &str) -> Color {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x2b2b33);
    Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn is_link(t: &str) -> bool {
    t.contains("://") || t.ends_with(':') || t.starts_with("ms-settings:") || t.starts_with("mailto:")
}

/// The glyph a key falls back to when it has no icon of its own.
fn kind_glyph(kind: &str, target: &str) -> &'static str {
    match kind {
        "open" if is_link(target) => "\u{E774}",
        "open" => "\u{E7B5}",
        "multi" => "\u{E8A9}",
        "command" => "\u{E756}",
        "script" => "\u{E943}",
        "hotkey" => "\u{E765}",
        "text" => "\u{E8D2}",
        _ => "\u{E945}",
    }
}

fn default_title(kind: &str, target: &str) -> String {
    let first = target.lines().next().unwrap_or("").trim().trim_start_matches('>').trim();
    match kind {
        "open" if !target.trim().is_empty() => item_name(target.trim()),
        "multi" => format!("{} items", target.lines().filter(|l| !l.trim().is_empty()).count()),
        "command" => first.split_whitespace().next().unwrap_or("Command").to_string(),
        "script" => "Script".into(),
        "hotkey" => target.trim().to_string(),
        "text" => first.chars().take(14).collect(),
        "action" => target.to_string(),
        _ => String::new(),
    }
}

/// Runs a command line: hidden, or in a console window that stays open to show the output.
fn run_command(cmd: &str, show: bool) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let mut c = std::process::Command::new("cmd");
    if show {
        c.arg("/k").raw_arg(cmd).creation_flags(CREATE_NEW_CONSOLE);
    } else {
        c.arg("/c").raw_arg(cmd).creation_flags(CREATE_NO_WINDOW);
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        c.current_dir(home);
    }
    c.spawn().is_ok()
}

/// Saves a script key's lines to %APPDATA%\Winklet\scripts and runs the file: hidden, or in a
/// console window that stays open to show the output.
fn run_script(id: &str, body: &str, lang: &str, show: bool) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let batch = lang == "batch";
    let dir = store::data_dir().join("scripts");
    let file = dir.join(format!("deck-{id}.{}", if batch { "cmd" } else { "ps1" }));
    let body = body.replace("\r\n", "\n").replace('\n', "\r\n");
    // Windows PowerShell reads a .ps1 without a BOM as ANSI; cmd needs the UTF-8 code page
    let text = if batch { format!("@chcp 65001 >nul\r\n{body}\r\n") } else { format!("\u{FEFF}{body}\r\n") };
    if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, text)).is_err() {
        return false;
    }
    let mut c = if batch {
        let mut c = std::process::Command::new("cmd");
        c.arg(if show { "/k" } else { "/c" }).arg(&file);
        c
    } else {
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass"]);
        if show {
            c.arg("-NoExit");
        }
        c.arg("-File").arg(&file);
        c
    };
    c.creation_flags(if show { CREATE_NEW_CONSOLE } else { CREATE_NO_WINDOW });
    if let Ok(home) = std::env::var("USERPROFILE") {
        c.current_dir(home);
    }
    c.spawn().is_ok()
}

impl App {
    /// The program's icon when the key shows one (an "open" key without a glyph).
    fn deck_icon(&mut self, k: &store::DeckKey) -> Image {
        if k.kind == "open" && k.glyph.is_empty() && !is_link(&k.target) { self.icon_for(k.target.trim()) } else { Image::default() }
    }

    pub fn refresh_deck(&mut self) {
        let page = self.ui().global::<Bridge>().get_deck_page().clamp(0, self.data.deck_pages - 1);
        let keys: Vec<store::DeckKey> = self.data.deck.iter().filter(|k| k.page == page).cloned().collect();
        let slots: Vec<DeckSlot> = (0..PER_PAGE)
            .map(|slot| match keys.iter().find(|k| k.slot == slot) {
                Some(k) => {
                    let icon = self.deck_icon(k);
                    let has_icon = icon.size().width > 0;
                    DeckSlot {
                        title: s(if k.title.is_empty() { default_title(&k.kind, &k.target) } else { k.title.clone() }),
                        glyph: s(if k.glyph.is_empty() { kind_glyph(&k.kind, &k.target) } else { k.glyph.as_str() }),
                        icon,
                        has_icon,
                        color: hex_color(&k.color),
                        empty: false,
                    }
                }
                None => DeckSlot { empty: true, ..Default::default() },
            })
            .collect();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_deck_page(page);
        b.set_deck_pages(self.data.deck_pages);
        b.set_deck_keys(ModelRc::new(VecModel::from(slots)));
        b.set_deck_palette(ModelRc::new(VecModel::from(PALETTE.iter().map(|c| hex_color(c)).collect::<Vec<_>>())));
    }

    fn deck_key(&self, slot: i32) -> Option<store::DeckKey> {
        let page = self.ui().global::<Bridge>().get_deck_page();
        self.data.deck.iter().find(|k| k.page == page && k.slot == slot).cloned()
    }

    /// A key was pressed. Hotkeys and text go to the app you were in, so the island steps aside first.
    pub fn deck_press(shared: &Shared, slot: i32) {
        let mut a = shared.borrow_mut();
        let Some(k) = a.deck_key(slot) else {
            drop(a);
            return with_editor(shared, slot);
        };
        let target = k.target.trim().to_string();
        let title = if k.title.is_empty() { default_title(&k.kind, &target) } else { k.title.clone() };
        match k.kind.as_str() {
            "open" => {
                if !sys::open(&target, "") {
                    a.toast(&format!("Couldn't open {title}"));
                }
            }
            "multi" => {
                let mut failed = 0;
                for line in target.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    let ok = match line.strip_prefix('>') {
                        Some(cmd) => run_command(cmd.trim(), false),
                        None => sys::open(line, ""),
                    };
                    failed += usize::from(!ok);
                }
                a.toast(&if failed == 0 { format!("{title} · opened") } else { format!("{title} · {failed} failed") });
            }
            "command" => {
                if !run_command(&target, k.flag) {
                    a.toast("Couldn't run the command");
                }
            }
            "script" => {
                if !run_script(&k.id, &target, &k.lang, k.flag) {
                    a.toast("Couldn't run the script");
                }
            }
            "hotkey" | "text" => {
                let (kind, flag) = (k.kind.clone(), k.flag);
                a.collapse();
                schedule_shrink(shared, &mut a);
                // give the app you were in the focus back before the keys arrive
                Timer::single_shot(Duration::from_millis(180), move || {
                    let ok = if kind == "text" { keys::type_text(&target, flag) } else { keys::combo(&target).unwrap_or(false) };
                    if !ok {
                        defer(|s| s.borrow_mut().toast("Couldn't send the keys"));
                    }
                });
            }
            "action" => {
                drop(a);
                return deck_action(shared, &target);
            }
            _ => {}
        }
    }

    /// Fills the editor from the key in `slot`, or with a fresh key.
    pub fn deck_edit(&mut self, slot: i32) {
        let k = self.deck_key(slot);
        let icon = k.as_ref().map(|k| self.deck_icon(k)).unwrap_or_default();
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let k = k.unwrap_or_else(|| store::DeckKey {
            kind: "open".into(),
            color: PALETTE[(slot as usize) % PALETTE.len()].into(),
            ..Default::default()
        });
        b.set_deck_edit_slot(slot);
        b.set_deck_edit_new(k.id.is_empty());
        b.set_deck_edit_kind(KINDS.iter().position(|x| *x == k.kind).unwrap_or(0) as i32);
        b.set_deck_edit_target(s(k.target.clone()));
        b.set_deck_edit_title(s(k.title.clone()));
        b.set_deck_edit_glyph(s(k.glyph.clone()));
        b.set_deck_edit_color(PALETTE.iter().position(|c| c.eq_ignore_ascii_case(&k.color)).unwrap_or(0) as i32);
        b.set_deck_edit_flag(k.flag);
        b.set_deck_edit_lang(LANGS.iter().position(|x| *x == k.lang).unwrap_or(0) as i32);
        b.set_deck_edit_has_icon(icon.size().width > 0);
        b.set_deck_edit_icon(icon);
        b.set_deck_edit_open(true);
    }

    /// The target changed in the editor: show the program's icon if it has one.
    pub fn deck_preview(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let target = b.get_deck_edit_target().trim().to_string();
        let icon = if b.get_deck_edit_kind() == 0 && !target.is_empty() && !is_link(&target) {
            self.icon_for(&target)
        } else {
            Image::default()
        };
        b.set_deck_edit_has_icon(icon.size().width > 0);
        b.set_deck_edit_icon(icon);
    }

    pub fn deck_save(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let kind = KINDS[b.get_deck_edit_kind().clamp(0, KINDS.len() as i32 - 1) as usize];
        let target = b.get_deck_edit_target().trim().trim_matches('"').to_string();
        if target.is_empty() {
            self.toast(match kind {
                "action" => "Pick an action",
                "hotkey" => "Type the keys, like ctrl+shift+esc",
                _ => "Add what the key should do",
            });
            return;
        }
        if kind == "hotkey" && !keys::combo_ok(&target) {
            self.toast("Unknown key: try ctrl+shift+esc, win+d, alt+f4");
            return;
        }
        let page = b.get_deck_page();
        let slot = b.get_deck_edit_slot();
        let color = PALETTE[b.get_deck_edit_color().clamp(0, PALETTE.len() as i32 - 1) as usize].to_string();
        let key = store::DeckKey {
            id: String::new(),
            page,
            slot,
            kind: kind.into(),
            target,
            title: b.get_deck_edit_title().trim().to_string(),
            glyph: b.get_deck_edit_glyph().to_string(),
            color,
            flag: b.get_deck_edit_flag(),
            lang: if kind == "script" { LANGS[b.get_deck_edit_lang().clamp(0, 1) as usize].into() } else { String::new() },
        };
        match self.data.deck.iter_mut().find(|k| k.page == page && k.slot == slot) {
            Some(k) => *k = store::DeckKey { id: k.id.clone(), ..key },
            None => self.data.deck.push(store::DeckKey { id: store::new_id(), ..key }),
        }
        self.data.save();
        b.set_deck_edit_open(false);
        self.refresh_deck();
    }

    pub fn deck_delete(&mut self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let (page, slot) = (b.get_deck_page(), b.get_deck_edit_slot());
        self.data.deck.retain(|k| !(k.page == page && k.slot == slot));
        self.data.save();
        b.set_deck_edit_open(false);
        self.refresh_deck();
    }

    /// Drag in edit mode: the key lands on `to`, and whatever was there takes its old place.
    pub fn deck_move(&mut self, from: i32, to: i32) {
        if from == to || !(0..PER_PAGE).contains(&to) {
            return;
        }
        let page = self.ui().global::<Bridge>().get_deck_page();
        for k in self.data.deck.iter_mut().filter(|k| k.page == page) {
            if k.slot == from {
                k.slot = to;
            } else if k.slot == to {
                k.slot = from;
            }
        }
        self.data.save();
        self.refresh_deck();
    }

    pub fn deck_page_set(&mut self, page: i32) {
        self.ui().global::<Bridge>().set_deck_page(page.clamp(0, self.data.deck_pages - 1));
        self.refresh_deck();
    }

    pub fn deck_page_add(&mut self) {
        if self.data.deck_pages >= MAX_PAGES {
            self.toast("That's the most pages");
            return;
        }
        self.data.deck_pages += 1;
        self.data.save();
        self.deck_page_set(self.data.deck_pages - 1);
    }

    /// Removes the page on screen and its keys (pages after it move up).
    pub fn deck_page_remove(&mut self) {
        if self.data.deck_pages <= 1 {
            return;
        }
        let page = self.ui().global::<Bridge>().get_deck_page();
        self.data.deck.retain(|k| k.page != page);
        for k in self.data.deck.iter_mut().filter(|k| k.page > page) {
            k.page -= 1;
        }
        self.data.deck_pages -= 1;
        self.data.save();
        self.deck_page_set(page.min(self.data.deck_pages - 1));
    }
}

fn with_editor(shared: &Shared, slot: i32) {
    shared.borrow_mut().deck_edit(slot);
}

/// Winklet's own actions for "action" keys.
fn deck_action(shared: &Shared, id: &str) {
    let mut a = shared.borrow_mut();
    match id {
        "playpause" | "next" | "prev" | "mute" | "volup" | "voldown" => {
            keys::tap(id);
        }
        "mic" => a.toggle_mic(),
        "screenshot" => {
            drop(a);
            return defer(take_screenshot);
        }
        "lock" => {
            sys::power(sys::PowerAction::Lock);
        }
        "sleep" => {
            sys::power(sys::PowerAction::Sleep);
        }
        "focus" => {
            a.focus.running = !a.focus.running;
            a.refresh_focus();
            let on = a.focus.running;
            a.toast(if on { "Focus timer started" } else { "Focus timer paused" });
        }
        "game" => {
            let on = !a.data.settings.game_mode;
            a.set_game_mode(on, false);
        }
        _ => {}
    }
}

/// "Browse…" in the editor: pick a program, file or folder. The dialog is modal, so the app
/// isn't borrowed meanwhile.
pub fn deck_browse(shared: &Shared, folders: bool) {
    let hwnd = {
        let mut a = shared.borrow_mut();
        a.hovering = true;
        a.collapse_timer.stop();
        a.hwnd
    };
    let picked = sys::pick_paths_titled(hwnd, folders, if folders { "Pick a folder for the key" } else { "Pick an app or file for the key" });
    let mut a = shared.borrow_mut();
    if let Some(p) = picked.into_iter().next() {
        let path = p.to_string_lossy().to_string();
        let ui = a.ui();
        let b = ui.global::<Bridge>();
        if b.get_deck_edit_kind() == 1 {
            // multi: add a line
            let cur = b.get_deck_edit_target().to_string();
            let sep = if cur.trim().is_empty() || cur.ends_with('\n') { "" } else { "\n" };
            b.set_deck_edit_target(s(format!("{cur}{sep}{path}")));
        } else {
            b.set_deck_edit_target(s(path.clone()));
            if b.get_deck_edit_title().is_empty() {
                b.set_deck_edit_title(s(item_name(&path)));
            }
            // a picked program shows its own icon
            b.set_deck_edit_glyph(s(""));
        }
        a.deck_preview();
    }
    a.hovering = false;
    schedule_collapse(shared, &mut a, 3000);
}
