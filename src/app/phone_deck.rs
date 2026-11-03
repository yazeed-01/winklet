//! Deck → Phone: the Winklet Android app shows this Deck and presses its keys over the Wi-Fi.
//! The link (and the port, when it's free) stays the same, so a paired phone reconnects by
//! itself; "New link" makes a new secret and the old phone has to scan again.
//! The other way, a paired phone sends its notifications (peeks here, with a Copy button for
//! sign-in codes), its calls and its battery; the Phone tab shows them all.

use super::{App, deck, defer, s, send::qr_image, with_app};
use crate::deck_remote::{self, Remote, Snapshot};
use crate::{Bridge, PhoneCall, PhoneNote, fg, sys};
use chrono::{DateTime, Local};
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// the Phone tab
pub const TAB_PHONE: i32 = 17;
/// How many notifications and calls the Phone tab keeps (in memory only).
const KEEP: usize = 50;

pub struct Note {
    app: String,
    title: String,
    text: String,
    code: String,
    at: DateTime<Local>,
}

pub struct Call {
    who: String,
    /// 0 ringing, 1 answered, 2 missed
    kind: i32,
    at: DateTime<Local>,
}

/// What the phone said about itself in its last check-in.
#[derive(Default)]
pub struct Info {
    model: String,
    android: String,
    ringer: i64,
    steps: i64,
    screen_min: i64,
}

#[derive(Default)]
pub struct PhoneDeck {
    pub server: Option<Arc<Remote>>,
    /// program path -> its icon as PNG, so re-sending the deck doesn't redraw them
    pngs: HashMap<String, Option<Vec<u8>>>,
    version: u64,
    error: String,
    /// the phone's battery and whether it's charging, once it has said
    battery: Option<(i64, bool)>,
    /// the low-battery peek shows once per discharge
    low_warned: bool,
    /// the sign-in code on the peek, for its Copy button
    code: String,
    info: Option<Info>,
    notes: VecDeque<Note>,
    calls: VecDeque<Call>,
}

impl App {
    /// Starts serving the Deck when the phone deck is on (at launch, or when turned on).
    pub fn phone_deck_start(&mut self) {
        if !self.data.settings.phone_deck || self.phone.server.is_some() {
            return;
        }
        let st = &mut self.data.settings;
        if st.phone_deck_token.is_empty() || st.phone_deck_key.is_empty() {
            // a link from before the key (0.10 and older) gets a new one: the phone scans again
            st.phone_deck_token = uuid::Uuid::new_v4().simple().to_string();
            st.phone_deck_key = deck_remote::new_key();
            self.data.save();
        }
        let (token, key) = (self.data.settings.phone_deck_token.clone(), self.data.settings.phone_deck_key.clone());
        let res = deck_remote::start(
            &token,
            &key,
            self.data.settings.phone_deck_port,
            |id| {
                // pressing runs on the UI thread, like a click on the key
                let _ = slint::invoke_from_event_loop(move || with_app(|s| App::deck_press_id(s, &id)));
            },
            |json| {
                let _ = slint::invoke_from_event_loop(move || with_app(|s| s.borrow_mut().phone_event(&json)));
            },
        );
        match res {
            Ok(server) => {
                self.phone.error.clear();
                if server.port != self.data.settings.phone_deck_port {
                    self.data.settings.phone_deck_port = server.port;
                    self.data.save();
                }
                self.phone.server = Some(server);
                self.phone_deck_sync();
            }
            Err(e) => self.phone.error = e,
        }
        self.refresh_phone_deck();
    }

    pub fn phone_deck_stop(&mut self) {
        if let Some(server) = self.phone.server.take() {
            server.stop();
        }
        self.refresh_phone_deck();
    }

    pub fn phone_deck_toggle(&mut self) {
        self.data.settings.phone_deck = !self.data.settings.phone_deck;
        self.data.save();
        if self.data.settings.phone_deck { self.phone_deck_start() } else { self.phone_deck_stop() }
    }

    /// A new secret: phones paired with the old link stop working until they scan again.
    pub fn phone_deck_reset(&mut self) {
        self.data.settings.phone_deck_token.clear();
        self.data.settings.phone_deck_key.clear();
        self.data.save();
        self.phone_deck_stop();
        self.phone_deck_start();
        self.toast("New link: scan it again on the phone");
    }

    /// Sends the keys as they are now to the phone (after any change on the Deck).
    pub fn phone_deck_sync(&mut self) {
        let Some(server) = self.phone.server.clone() else { return };
        self.phone.version += 1;
        let mut icons = HashMap::new();
        let mut keys = Vec::new();
        for k in self.data.deck.clone() {
            let (title, glyph) = deck::face(&k);
            let icon = deck::shows_icon(&k).then(|| self.phone_png(k.target.trim())).flatten();
            if let Some(png) = icon {
                icons.insert(k.id.clone(), png);
            }
            keys.push(serde_json::json!({
                "id": k.id,
                "page": k.page,
                "slot": k.slot,
                "title": title,
                "glyph": glyph,
                "color": k.color,
                "icon": icons.contains_key(&k.id),
            }));
        }
        let json = serde_json::json!({
            "version": self.phone.version,
            "pages": self.data.deck_pages,
            "cols": 8,
            "rows": deck::PER_PAGE / 8,
            "keys": keys,
        });
        server.set(Snapshot { json: json.to_string(), icons });
    }

    fn phone_png(&mut self, path: &str) -> Option<Vec<u8>> {
        self.phone
            .pngs
            .entry(path.to_string())
            .or_insert_with(|| fg::icon_rgba(path).and_then(|(w, h, px)| png_bytes(w, h, &px)))
            .clone()
    }

    pub fn refresh_phone_deck(&mut self) {
        let on = self.data.settings.phone_deck;
        let server = self.phone.server.clone();
        let status = match (&server, on) {
            (_, false) => "Off: turn it on to use your phone as a Deck".to_string(),
            (None, true) => self.phone.error.clone(),
            (Some(srv), true) => match srv.seen_ago() {
                // the phone checks in every minute while it sends notifications
                Some(t) if t < 90 => "Phone connected".into(),
                Some(t) if t < 3600 => format!("Phone last seen {} min ago", t / 60 + 1),
                Some(_) => "Phone last seen over an hour ago".into(),
                None => "Scan this with the Winklet app: Settings → Connect PC deck".into(),
            },
        };
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_deck_phone_on(on && server.is_some());
        b.set_deck_phone_status(s(status));
        b.set_deck_phone_url(s(server.as_ref().map(|r| r.url.clone()).unwrap_or_default()));
        b.set_deck_phone_qr(server.as_ref().map(|r| qr_image(&r.url)).unwrap_or_else(Image::default));
        b.set_phone_battery(s(match self.phone.battery {
            Some((pct, true)) => format!("Phone battery {pct}% · charging"),
            Some((pct, false)) => format!("Phone battery {pct}%"),
            None => String::new(),
        }));
        self.refresh_phone_page();
    }

    /// The Phone tab: the phone's state, its notifications and its calls.
    fn refresh_phone_page(&self) {
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        let seen = self.phone.server.as_ref().and_then(|r| r.seen_ago());
        b.set_phone_live(seen.is_some_and(|t| t < 90));
        b.set_phone_seen(s(match seen {
            _ if !self.data.settings.phone_deck => "Phone link is off".to_string(),
            Some(t) if t < 90 => "Connected".into(),
            Some(t) if t < 3600 => format!("Seen {} min ago", t / 60 + 1),
            Some(t) if t < 86_400 => format!("Seen {} h ago", t / 3600),
            Some(_) => "Seen over a day ago".into(),
            None => "Not connected yet".into(),
        }));
        let (pct, charging) = self.phone.battery.unwrap_or((-1, false));
        b.set_phone_pct(pct as i32);
        b.set_phone_charging(charging);
        let info = self.phone.info.as_ref();
        b.set_phone_model(s(info.map(|i| i.model.clone()).unwrap_or_default()));
        b.set_phone_android(s(info.filter(|i| !i.android.is_empty()).map(|i| format!("Android {}", i.android)).unwrap_or_default()));
        b.set_phone_ringer(info.map(|i| i.ringer as i32).unwrap_or(-1));
        b.set_phone_steps(s(info.map(|i| thousands(i.steps)).unwrap_or_default()));
        b.set_phone_screen(s(info.map(|i| format!("{}h {:02}m", i.screen_min / 60, i.screen_min % 60)).unwrap_or_default()));

        let h24 = self.data.settings.clock_24h;
        let notes: Vec<PhoneNote> = self
            .phone
            .notes
            .iter()
            .map(|n| PhoneNote {
                app: s(n.app.clone()),
                letter: s(initial(&n.app)),
                tint: deck::hex_color(deck::PALETTE[1 + (hash(&n.app) % (deck::PALETTE.len() - 1))]),
                title: s(n.title.clone()),
                text: s(n.text.clone()),
                time: s(when(n.at, h24)),
                code: s(n.code.clone()),
            })
            .collect();
        let calls: Vec<PhoneCall> = self
            .phone
            .calls
            .iter()
            .map(|c| PhoneCall { who: s(c.who.clone()), letter: s(initial(&c.who)), kind: c.kind, time: s(when(c.at, h24)) })
            .collect();
        b.set_phone_missed(self.phone.calls.iter().filter(|c| c.kind == 2).count() as i32);
        b.set_phone_notes(ModelRc::new(VecModel::from(notes)));
        b.set_phone_calls(ModelRc::new(VecModel::from(calls)));
    }

    /// The Phone tab's Clear: empties the list on show (notifications or calls).
    pub fn phone_clear(&mut self) {
        if self.ui().global::<Bridge>().get_phone_view() == 1 {
            self.phone.calls.clear();
        } else {
            self.phone.notes.clear();
        }
        self.refresh_phone_page();
    }

    /// Copy on a notification with a sign-in code in the Phone tab.
    pub fn phone_copy(&mut self, code: &str) {
        if !code.is_empty() && sys::set_clipboard(code) {
            self.toast("Code copied");
        }
    }

    /// Something the phone sent: `{"type":"notif","app":..,"title":..,"text":..}` or
    /// `{"type":"status","battery":82,"charging":true}`.
    pub fn phone_event(&mut self, json: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        match v.get("type").and_then(|x| x.as_str()) {
            Some("status") => {
                let pct = v.get("battery").and_then(|x| x.as_i64()).unwrap_or(-1).clamp(-1, 100);
                let charging = v.get("charging").and_then(|x| x.as_bool()).unwrap_or(false);
                if pct < 0 {
                    return;
                }
                if charging || pct > 20 {
                    self.phone.low_warned = false;
                } else if pct <= 15 && !self.phone.low_warned {
                    self.phone.low_warned = true;
                    self.peek(19, "Phone battery low", &format!("{pct}% left · plug it in"), 6);
                }
                self.phone.battery = Some((pct, charging));
                let num = |k: &str| v.get(k).and_then(|x| x.as_i64()).unwrap_or(-1);
                if v.get("model").is_some() {
                    self.phone.info = Some(Info {
                        model: clip(&text("model"), 40),
                        android: clip(&text("android"), 12),
                        ringer: num("ringer"),
                        steps: num("steps").max(0),
                        screen_min: num("screen").max(0),
                    });
                }
                self.refresh_phone_deck();
            }
            Some("notif") => {
                let (app, title, body) = (text("app"), text("title"), text("text"));
                if title.is_empty() && body.is_empty() {
                    return;
                }
                let code = find_code(&format!("{title} {body}")).unwrap_or_default();
                self.phone.notes.push_front(Note {
                    app: clip(&app, 40),
                    title: clip(&title, 80),
                    text: clip(&body, 160),
                    code: code.clone(),
                    at: Local::now(),
                });
                self.phone.notes.truncate(KEEP);
                self.refresh_phone_deck();
                if !code.is_empty() {
                    self.phone.code = code.clone();
                    self.peek(20, &format!("Code {code}"), &clip(&format!("{app} · {body}"), 80), 20);
                    return;
                }
                let head = if title.is_empty() { app.clone() } else { title };
                let sub = if body.is_empty() { app } else { format!("{app} · {body}") };
                self.peek(19, &clip(&head, 48), &clip(&sub, 80), 6);
            }
            Some("call") => {
                let who = clip(&text("who"), 60);
                let who = if who.is_empty() { "Unknown caller".to_string() } else { who };
                // the call on top, if it's still the same caller from the last few minutes
                let same = self.phone.calls.front().is_some_and(|c| c.who == who && (Local::now() - c.at).num_minutes() < 5);
                let top = self.phone.calls.front().map(|c| c.kind);
                match v.get("state").and_then(|x| x.as_str()) {
                    Some("ringing") => {
                        if !(same && top == Some(0)) {
                            self.phone.calls.push_front(Call { who: who.clone(), kind: 0, at: Local::now() });
                        }
                        self.peek(22, &who, "Incoming call on your phone", 30);
                        self.eye_event(6, 2.0);
                    }
                    Some("active") => {
                        if same {
                            self.phone.calls[0].kind = 1;
                        } else {
                            self.phone.calls.push_front(Call { who, kind: 1, at: Local::now() });
                        }
                        self.dismiss_call_peek();
                    }
                    Some("missed") => {
                        if same && top == Some(0) {
                            self.phone.calls[0].kind = 2;
                        } else if !(same && top == Some(2)) {
                            self.phone.calls.push_front(Call { who: who.clone(), kind: 2, at: Local::now() });
                        }
                        self.dismiss_call_peek();
                        self.peek(19, "Missed call", &who, 8);
                    }
                    Some("ended") => self.dismiss_call_peek(),
                    _ => return,
                }
                self.phone.calls.truncate(KEEP);
                self.refresh_phone_deck();
            }
            _ => {}
        }
    }

    /// The call stopped ringing: take its peek away (after this borrow of the app ends, since
    /// dismissing borrows it again).
    fn dismiss_call_peek(&mut self) {
        defer(|s| {
            let ui = s.borrow().ui();
            let b = ui.global::<Bridge>();
            if b.get_peeking() && b.get_peek_kind() == 22 {
                b.invoke_dismiss_peek();
            }
        });
    }

    /// The Copy button on a sign-in code peek.
    pub fn phone_copy_code(&mut self) {
        if !self.phone.code.is_empty() && sys::set_clipboard(&self.phone.code) {
            self.toast("Code copied");
        }
    }
}

/// The first letter of a name, for its round avatar.
fn initial(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into())
}

fn hash(text: &str) -> usize {
    text.bytes().fold(5381usize, |h, b| h.wrapping_mul(33) ^ b as usize)
}

fn thousands(n: i64) -> String {
    if n >= 1000 { format!("{},{:03}", n / 1000, n % 1000) } else { n.to_string() }
}

/// "now", "5m", "14:03" today, "Yesterday", or "Nov 2".
fn when(at: DateTime<Local>, h24: bool) -> String {
    let now = Local::now();
    let mins = (now - at).num_minutes();
    if mins < 1 {
        "now".into()
    } else if mins < 60 {
        format!("{mins}m")
    } else if at.date_naive() == now.date_naive() {
        at.format(if h24 { "%H:%M" } else { "%-I:%M %p" }).to_string()
    } else if Some(at.date_naive()) == now.date_naive().pred_opt() {
        "Yesterday".into()
    } else {
        at.format("%b %-d").to_string()
    }
}

/// Shortens to `max` characters with an ellipsis, on one line.
fn clip(text: &str, max: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max - 1).collect::<String>().trim_end()) }
}

/// A sign-in code in a message that talks about one: 4 to 8 digits, maybe split like
/// "123 456" or "123-456".
pub fn find_code(text: &str) -> Option<String> {
    const WORDS: [&str; 12] =
        ["code", "otp", "verif", "passcode", "pin", "one-time", "2fa", "sign in", "login", "رمز", "كود", "تحقق"];
    let lower = text.to_lowercase();
    if !WORDS.iter().any(|w| lower.contains(w)) {
        return None;
    }
    let c: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < c.len() {
        // a number that starts on its own (not the end of "G-123" style ids or words)
        if !c[i].is_ascii_digit() || (i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '-')) {
            i += 1;
            continue;
        }
        let (mut j, mut digits, mut group) = (i, String::new(), 0);
        while j < c.len() {
            if c[j].is_ascii_digit() {
                digits.push(c[j]);
                group += 1;
                j += 1;
            } else if (c[j] == ' ' || c[j] == '-') && group >= 3 && c.get(j + 1).is_some_and(|d| d.is_ascii_digit()) {
                group = 0;
                j += 1;
            } else {
                break;
            }
        }
        let clean_end = c.get(j).is_none_or(|n| !n.is_alphanumeric());
        if clean_end && (4..=8).contains(&digits.len()) {
            return Some(digits);
        }
        i = j.max(i + 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::find_code;

    #[test]
    fn codes() {
        assert_eq!(find_code("Your verification code is 482913").as_deref(), Some("482913"));
        assert_eq!(find_code("G-551234 is your Google verification code"), None);
        assert_eq!(find_code("Use 123 456 to sign in").as_deref(), Some("123456"));
        assert_eq!(find_code("Login code: 123-456.").as_deref(), Some("123456"));
        assert_eq!(find_code("رمز التحقق الخاص بك 7731").as_deref(), Some("7731"));
        assert_eq!(find_code("Meet at 1730 tomorrow"), None);
        assert_eq!(find_code("Your code is 12"), None);
    }
}

fn png_bytes(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}
