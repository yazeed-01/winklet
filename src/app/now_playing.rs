//! Media session updates from the background watcher.

use super::{App, s, trace};
use crate::{Bridge, media};
use slint::ComponentHandle;

impl App {
    pub fn on_media(&mut self, info: media::Info) {
        trace(&format!("media has={} playing={} title={:?}", info.has, info.playing, info.title));
        let ui = self.ui();
        let b = ui.global::<Bridge>();
        b.set_has_media(info.has);
        b.set_media_playing(info.playing);
        b.set_media_title(s(info.title.clone()));
        b.set_media_artist(s(info.artist.clone()));
        b.set_media_app(s(info.app.clone()));
        let fmt = |x: f64| format!("{}:{:02}", x as i64 / 60, x as i64 % 60);
        b.set_media_progress(if info.len > 0.0 { (info.pos / info.len) as f32 } else { 0.0 });
        b.set_media_pos(s(if info.len > 0.0 { fmt(info.pos) } else { String::new() }));
        b.set_media_len(s(if info.len > 0.0 { fmt(info.len) } else { String::new() }));

        let track = format!("{} — {}", info.title, info.artist);
        if info.playing && !info.title.is_empty() && track != self.last_track {
            let first = self.last_track.is_empty();
            self.last_track = track;
            if !first && !b.get_expanded() && !self.focus.running {
                self.peek(7, &info.title, &info.artist, 3);
            }
        }
    }
}
