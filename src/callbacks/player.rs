//! Media player buttons.

use crate::app::Shared;
use crate::{Bridge, media};

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_media_toggle, |a| { let _ = a.media_tx.send(media::Cmd::Toggle); });
    on!(b, app, on_media_next, |a| { let _ = a.media_tx.send(media::Cmd::Next); });
    on!(b, app, on_media_prev, |a| { let _ = a.media_tx.send(media::Cmd::Prev); });
    on!(b, app, on_playlist_add, |a, text| { a.playlist_add(&text); });
    on!(b, app, on_playlist_open, |a, id| { a.playlist_open(&id); });
    on!(b, app, on_playlist_remove, |a, id| { a.playlist_remove(&id); });
    on!(b, app, on_playlist_move, |a, from, to| { a.playlist_move(from, to); });
}
