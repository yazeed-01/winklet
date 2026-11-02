//! Deck tab: pressing keys, the key editor and pages.

use crate::Bridge;
use crate::app::{App, Shared, deck_browse, defer};

pub fn wire(b: &Bridge, app: &Shared) {
    // pressing can collapse the island or open a dialog: let the click finish first
    b.on_deck_press(|slot| defer(move |s| App::deck_press(s, slot)));
    on!(b, app, on_deck_edit, |a, slot| { a.deck_edit(slot); });
    on!(b, app, on_deck_save, |a| { a.deck_save(); });
    on!(b, app, on_deck_delete, |a| { a.deck_delete(); });
    on!(b, app, on_deck_preview, |a| { a.deck_preview(); });
    on!(b, app, on_deck_move, |a, from, to| { a.deck_move(from, to); });
    on!(b, app, on_deck_page_set, |a, page| { a.deck_page_set(page); });
    on!(b, app, on_deck_page_add, |a| { a.deck_page_add(); });
    on!(b, app, on_deck_page_remove, |a| { a.deck_page_remove(); });
    b.on_deck_browse(|folders| defer(move |s| deck_browse(s, folders)));
    on!(b, app, on_deck_phone_toggle, |a| { a.phone_deck_toggle(); });
    on!(b, app, on_deck_phone_reset, |a| { a.phone_deck_reset(); });
    on!(b, app, on_deck_phone_refresh, |a| { a.refresh_phone_deck(); });
    on!(b, app, on_phone_copy_code, |a| { a.phone_copy_code(); });
    on!(b, app, on_phone_clear, |a| { a.phone_clear(); });
    on!(b, app, on_phone_copy, |a, code| { a.phone_copy(&code); });
}
