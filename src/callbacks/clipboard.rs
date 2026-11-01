//! Clipboard tab: search, pick, copy or paste again, pin, delete, find in the preview.

use crate::app::{App, Shared, defer};
use crate::{Bridge, sys};

pub fn wire(b: &Bridge, app: &Shared) {
    on!(b, app, on_clip_search, |a, q| { a.clip_search(&q); });
    on!(b, app, on_clip_filter_set, |a, f| { a.clip_filter_set(f); });
    on!(b, app, on_clip_select, |a, id| { a.clip_select(&id); });
    on!(b, app, on_clip_copy, |a| { a.clip_copy(); });
    // collapses the island: let the click finish first
    b.on_clip_paste(|| defer(App::clip_paste));
    on!(b, app, on_clip_pin, |a| { a.clip_pin(); });
    on!(b, app, on_clip_delete, |a| { a.clip_delete(); });
    on!(b, app, on_clip_clear, |a| { a.clip_clear(); });
    on!(b, app, on_clip_pause_toggle, |a| { a.clip_pause_toggle(); });
    on!(b, app, on_clip_find, |a, q| { a.clip_find(&q); });
    on!(b, app, on_clip_find_step, |a, d| { a.clip_find_step(d); });
    on!(b, app, on_clip_cols, |a, c| { a.clip_cols(c); });
    on!(b, app, on_clip_open, |a, path| {
        let target = if path.is_empty() { a.clip_selected_text() } else { path.to_string() };
        let target = if target.starts_with("www.") { format!("https://{target}") } else { target };
        sys::open_document(target.trim());
    });
}
