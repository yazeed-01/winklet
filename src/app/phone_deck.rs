//! Deck → Phone: the Winklet Android app shows this Deck and presses its keys over the Wi-Fi.
//! The link (and the port, when it's free) stays the same, so a paired phone reconnects by
//! itself; "New link" makes a new secret and the old phone has to scan again.

use super::{App, deck, s, send::qr_image, with_app};
use crate::deck_remote::{self, Remote, Snapshot};
use crate::{Bridge, fg};
use slint::{ComponentHandle, Image};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub struct PhoneDeck {
    pub server: Option<Arc<Remote>>,
    /// program path -> its icon as PNG, so re-sending the deck doesn't redraw them
    pngs: HashMap<String, Option<Vec<u8>>>,
    version: u64,
    error: String,
}

impl App {
    /// Starts serving the Deck when the phone deck is on (at launch, or when turned on).
    pub fn phone_deck_start(&mut self) {
        if !self.data.settings.phone_deck || self.phone.server.is_some() {
            return;
        }
        if self.data.settings.phone_deck_token.is_empty() {
            self.data.settings.phone_deck_token = uuid::Uuid::new_v4().simple().to_string();
            self.data.save();
        }
        let token = self.data.settings.phone_deck_token.clone();
        let res = deck_remote::start(&token, self.data.settings.phone_deck_port, |id| {
            // pressing runs on the UI thread, like a click on the key
            let _ = slint::invoke_from_event_loop(move || with_app(|s| App::deck_press_id(s, &id)));
        });
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
                Some(t) if t < 15 => "Phone connected".into(),
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
